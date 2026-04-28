//! HTTP authentication middleware for PJS endpoints.
//!
//! This module provides Tower [`Layer`] implementations for API key and (optionally) JWT
//! authentication. Both layers mirror the boilerplate pattern of [`RateLimitMiddleware`]
//! in `middleware.rs` so that they compose cleanly with the existing middleware stack.
//!
//! # Security design
//!
//! ## API key comparison
//!
//! Raw API keys are never stored. At construction time each configured key is tagged with
//! HMAC-SHA256 using a per-process random `hmac_key`. During authentication the candidate
//! token is tagged with the same key and the resulting 32-byte digest is compared against
//! every stored tag using [`ConstantTimeEq`].
//!
//! This design eliminates two side-channel classes:
//! - **Length leakage** — all tags are exactly 32 bytes regardless of original key length.
//! - **Key-index leakage** — the comparison always iterates the full tag list and accumulates
//!   results with bitwise OR; no early return is taken on a partial match.
//!
//! The unavoidable single bit — "any key matched vs. none matched" — leaks via the HTTP
//! response code (200 vs 401). This is acceptable per industry practice and inherent to
//! the protocol.
//!
//! ## CORS preflight
//!
//! `OPTIONS` requests bypass authentication unconditionally. Browsers do not attach
//! credentials on preflight (Fetch spec §3.7), so challenging them would silently break
//! every cross-origin `POST` from a browser client. The CORS layer is applied outside
//! both routers in `apply_common_layers`, providing belt-and-suspenders coverage.
//!
//! # Feature gates
//!
//! The entire module is gated behind `#[cfg(feature = "http-server")]`.
//! [`JwtAuthLayer`] is additionally gated behind `#[cfg(feature = "http-auth-jwt")]`.

#[cfg(feature = "http-server")]
mod inner {
    use axum::{
        body::Body,
        http::{Method, Request, Response, StatusCode, header},
    };
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::{
        future::Future,
        pin::Pin,
        sync::Arc,
        task::{Context, Poll},
    };
    use subtle::ConstantTimeEq;
    use tower::{Layer, Service};

    type HmacSha256 = Hmac<Sha256>;

    // ── Error type ──────────────────────────────────────────────────────────────

    /// Errors that can occur when constructing an auth layer from a key list.
    #[derive(Debug, thiserror::Error)]
    pub enum AuthConfigError {
        /// Returned when the key list passed to the constructor is empty.
        #[error("API key list must not be empty")]
        EmptyKeyList,
        /// Returned when any key contains ASCII whitespace characters.
        ///
        /// Keys with leading or trailing whitespace are almost always a configuration
        /// error (copy-paste of a quoted value, trailing newline, etc.). Rejecting them
        /// at construction time prevents silent authentication failures later.
        #[error("API key must not contain whitespace")]
        WhitespaceInKey,
        /// Returned when the system RNG fails to seed the per-process HMAC key.
        #[error("failed to seed HMAC key from system RNG: {0}")]
        RngFailure(getrandom::Error),
    }

    // ── Internal state ───────────────────────────────────────────────────────────

    /// Shared, reference-counted state stored inside each cloned layer/service.
    struct ApiKeyState {
        /// Per-process random seed. Used only for tag derivation — never exported or logged.
        hmac_key: [u8; 32],
        /// Pre-computed 32-byte HMAC-SHA256 tags of every configured API key.
        ///
        /// Fixed width regardless of original key length, so comparison cannot leak
        /// length classes.
        tags: Vec<[u8; 32]>,
    }

    // ── ApiKeyConfig (held by callers, consumed by ApiKeyAuthLayer::new) ────────

    /// Configuration for [`ApiKeyAuthLayer`].
    ///
    /// Contains the pre-processed HMAC tags of all accepted API keys plus the
    /// per-process random seed used to derive those tags.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use pjson_rs::infrastructure::http::auth::ApiKeyConfig;
    ///
    /// let config = ApiKeyConfig::new(&["secret-key-1", "secret-key-2"])?;
    /// let layer = ApiKeyAuthLayer::new(config);
    /// ```
    // NOTE: Debug is derived but the fields contain HMAC key material. The output is
    // formatted as arrays of integers which is not secret-safe in logs, but is needed for
    // test assertions. Do not log ApiKeyConfig instances in production code.
    #[derive(Debug)]
    pub struct ApiKeyConfig {
        /// HMAC-SHA256 tags of all configured API keys.
        pub(crate) keys: Vec<[u8; 32]>,
        /// Per-process seed used to derive tags. Never exported.
        pub(crate) hmac_key: [u8; 32],
    }

    impl ApiKeyConfig {
        /// Construct from a slice of raw API key strings.
        ///
        /// # Errors
        ///
        /// - [`AuthConfigError::EmptyKeyList`] — `raw_keys` is empty.
        /// - [`AuthConfigError::WhitespaceInKey`] — any key contains ASCII whitespace.
        /// - [`AuthConfigError::RngFailure`] — the system RNG could not generate the HMAC seed.
        pub fn new(raw_keys: &[&str]) -> Result<Self, AuthConfigError> {
            if raw_keys.is_empty() {
                return Err(AuthConfigError::EmptyKeyList);
            }
            if raw_keys
                .iter()
                .any(|k| k.bytes().any(|b| b.is_ascii_whitespace()))
            {
                return Err(AuthConfigError::WhitespaceInKey);
            }

            let mut hmac_key = [0u8; 32];
            getrandom::fill(&mut hmac_key).map_err(AuthConfigError::RngFailure)?;

            let keys = raw_keys
                .iter()
                .map(|k| hmac_tag(&hmac_key, k.as_bytes()))
                .collect();
            Ok(Self { keys, hmac_key })
        }
    }

    // ── Layer ────────────────────────────────────────────────────────────────────

    /// Tower [`Layer`] that enforces API key authentication on every non-OPTIONS request.
    ///
    /// Wrap only the routes that need protection. Public routes (e.g. `/pjs/health`)
    /// should live in a separate router that is merged **without** this layer:
    ///
    /// ```rust,ignore
    /// let protected = protected_routes().layer(ApiKeyAuthLayer::new(config));
    /// let router = Router::new()
    ///     .merge(public_routes())
    ///     .merge(protected)
    ///     .layer(apply_common_layers());
    /// ```
    ///
    /// Authentication accepts tokens from two header sources (first match wins):
    /// 1. `Authorization: Bearer <token>`
    /// 2. `X-PJS-API-Key: <token>`
    ///
    /// On failure the layer returns `401 Unauthorized` with a JSON body
    /// `{"error":"Unauthorized"}`.
    #[derive(Clone)]
    pub struct ApiKeyAuthLayer {
        inner: Arc<ApiKeyState>,
    }

    impl ApiKeyAuthLayer {
        /// Construct from a pre-built [`ApiKeyConfig`].
        pub fn new(config: ApiKeyConfig) -> Self {
            Self {
                inner: Arc::new(ApiKeyState {
                    hmac_key: config.hmac_key,
                    tags: config.keys,
                }),
            }
        }
    }

    impl<S> Layer<S> for ApiKeyAuthLayer {
        type Service = ApiKeyAuthService<S>;

        fn layer(&self, inner: S) -> Self::Service {
            ApiKeyAuthService {
                inner,
                state: self.inner.clone(),
            }
        }
    }

    // ── Service ───────────────────────────────────────────────────────────────────

    /// The [`Service`] produced by [`ApiKeyAuthLayer`].
    #[derive(Clone)]
    pub struct ApiKeyAuthService<S> {
        inner: S,
        state: Arc<ApiKeyState>,
    }

    impl<S> Service<Request<Body>> for ApiKeyAuthService<S>
    where
        S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
        S::Future: Send + 'static,
    {
        type Response = Response<Body>;
        type Error = S::Error;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, req: Request<Body>) -> Self::Future {
            // CORS preflight bypass: browsers do not send Authorization on OPTIONS
            // (Fetch spec §3.7). Even with CORS layered outside auth, this is a
            // defensive bypass for direct OPTIONS hits and non-browser preflight.
            if req.method() == Method::OPTIONS {
                let fut = self.inner.call(req);
                return Box::pin(fut);
            }

            let state = self.state.clone();
            let mut inner = self.inner.clone();

            Box::pin(async move {
                match extract_token(&req) {
                    Some(candidate) if matches_any(&state, candidate) => inner.call(req).await,
                    _ => Ok(unauthorized_response()),
                }
            })
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────────

    /// Compute an HMAC-SHA256 tag for `data` under `key`.
    fn hmac_tag(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(key)
            // PANIC: new_from_slice only fails for HMAC types that reject certain key lengths;
            // HMAC-SHA256 accepts any key length, so this cannot fail.
            .expect("HMAC-SHA256 accepts any key length");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    /// Constant-time membership test.
    ///
    /// Tags the candidate with the stored HMAC key and compares against every stored tag
    /// using [`ConstantTimeEq`]. The full list is always iterated — no early return —
    /// and results are accumulated with bitwise OR to prevent timing-based key-index
    /// discovery.
    fn matches_any(state: &ApiKeyState, candidate: &[u8]) -> bool {
        let candidate_tag = hmac_tag(&state.hmac_key, candidate);
        let mut acc: u8 = 0;
        for tag in &state.tags {
            // Both sides are 32-byte arrays; no length branch, no length leakage.
            acc |= tag.ct_eq(&candidate_tag).unwrap_u8();
        }
        // Single branch on the aggregate result — leaks only the unavoidable
        // "any match vs. no match" bit, which is inherent to the 200/401 response split.
        acc == 1
    }

    /// Extract the bearer token or API key from the request headers.
    ///
    /// Returns a borrow of the raw bytes from the header value — no allocation on the
    /// hot path. Returns `None` when neither expected header is present or parseable.
    ///
    /// Header sources (first match wins):
    /// 1. `Authorization: Bearer <token>`
    /// 2. `X-PJS-API-Key: <token>`
    fn extract_token(req: &Request<Body>) -> Option<&[u8]> {
        if let Some(v) = req.headers().get(header::AUTHORIZATION)
            && let Some(stripped) = v.as_bytes().strip_prefix(b"Bearer ")
        {
            return Some(trim_ascii(stripped));
        }
        if let Some(v) = req.headers().get("x-pjs-api-key") {
            return Some(trim_ascii(v.as_bytes()));
        }
        None
    }

    /// Strip leading and trailing ASCII whitespace from a byte slice.
    fn trim_ascii(bytes: &[u8]) -> &[u8] {
        let start = bytes
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(bytes.len());
        let end = bytes
            .iter()
            .rposition(|b| !b.is_ascii_whitespace())
            .map_or(start, |i| i + 1);
        &bytes[start..end]
    }

    /// Build a `401 Unauthorized` response with a JSON body.
    fn unauthorized_response() -> Response<Body> {
        let body = serde_json::json!({ "error": "Unauthorized" }).to_string();
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            // PANIC: the builder arguments are all static constants; this cannot fail.
            .expect("static unauthorized response is always valid")
    }

    // ── JWT layer (feature-gated) ─────────────────────────────────────────────────

    #[cfg(feature = "http-auth-jwt")]
    pub use jwt::{JwtAuthLayer, JwtAuthService, JwtConfig};

    #[cfg(feature = "http-auth-jwt")]
    mod jwt {
        //! JWT authentication layer.
        //!
        //! # Algorithm choice
        //!
        //! > **Performance note:** RS256 (RSA) verification throughput is approximately
        //! > 100× lower than HS256. At high request rates (≥ 1 000 req/s), RS256 will
        //! > saturate a CPU core. HS256 is recommended for production load profiles unless
        //! > asymmetric key distribution is a hard requirement.
        //!
        //! # Usage
        //!
        //! ```rust,ignore
        //! use pjson_rs::infrastructure::http::auth::{JwtAuthLayer, JwtConfig};
        //! use jsonwebtoken::{DecodingKey, Validation, Algorithm};
        //!
        //! let config = JwtConfig {
        //!     decoding_key: DecodingKey::from_secret(b"my-secret"),
        //!     validation: Validation::new(Algorithm::HS256),
        //! };
        //! let layer = JwtAuthLayer::<MyClaims>::new(config);
        //! ```

        use axum::{
            body::Body,
            http::{Method, Request, Response, StatusCode, header},
        };
        use jsonwebtoken::{DecodingKey, Validation};
        use serde::de::DeserializeOwned;
        use std::{
            future::Future,
            marker::PhantomData,
            pin::Pin,
            sync::Arc,
            task::{Context, Poll},
        };
        use tower::{Layer, Service};

        /// Configuration for [`JwtAuthLayer`].
        ///
        /// Callers are responsible for configuring the [`Validation`] struct to match
        /// their issuer, audience, and algorithm requirements before passing it here.
        pub struct JwtConfig {
            /// Key used to verify JWT signatures.
            pub decoding_key: DecodingKey,
            /// Validation parameters (algorithm, issuer, audience, expiry, etc.).
            pub validation: Validation,
        }

        /// Tower [`Layer`] that enforces JWT Bearer token authentication.
        ///
        /// Only `Authorization: Bearer <token>` is checked; `X-PJS-API-Key` is not
        /// accepted for JWT auth.
        ///
        /// Claims type `C` must be [`DeserializeOwned`] + [`Send`] + [`Sync`].
        /// The decoded claims are **discarded** — this layer only validates the token.
        /// If you need access to claims downstream, attach them via request extensions
        /// in a separate extractor layer.
        ///
        /// `OPTIONS` requests are passed through without authentication (same as
        /// [`ApiKeyAuthLayer`]).
        pub struct JwtAuthLayer<C> {
            inner: Arc<JwtState>,
            _claims: PhantomData<fn() -> C>,
        }

        impl<C> Clone for JwtAuthLayer<C> {
            fn clone(&self) -> Self {
                Self {
                    inner: self.inner.clone(),
                    _claims: PhantomData,
                }
            }
        }

        struct JwtState {
            decoding_key: DecodingKey,
            validation: Validation,
        }

        impl<C> JwtAuthLayer<C>
        where
            C: DeserializeOwned + Send + Sync + 'static,
        {
            /// Construct from a [`JwtConfig`].
            pub fn new(config: JwtConfig) -> Self {
                Self {
                    inner: Arc::new(JwtState {
                        decoding_key: config.decoding_key,
                        validation: config.validation,
                    }),
                    _claims: PhantomData,
                }
            }
        }

        impl<S, C> Layer<S> for JwtAuthLayer<C>
        where
            C: DeserializeOwned + Send + Sync + 'static,
        {
            type Service = JwtAuthService<S, C>;

            fn layer(&self, inner: S) -> Self::Service {
                JwtAuthService {
                    inner,
                    state: self.inner.clone(),
                    _claims: PhantomData,
                }
            }
        }

        /// The [`Service`] produced by [`JwtAuthLayer`].
        pub struct JwtAuthService<S, C> {
            inner: S,
            state: Arc<JwtState>,
            _claims: PhantomData<fn() -> C>,
        }

        impl<S, C> Clone for JwtAuthService<S, C>
        where
            S: Clone,
        {
            fn clone(&self) -> Self {
                Self {
                    inner: self.inner.clone(),
                    state: self.state.clone(),
                    _claims: PhantomData,
                }
            }
        }

        impl<S, C> Service<Request<Body>> for JwtAuthService<S, C>
        where
            S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
            S::Future: Send + 'static,
            C: DeserializeOwned + Send + Sync + 'static,
        {
            type Response = Response<Body>;
            type Error = S::Error;
            type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

            fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                self.inner.poll_ready(cx)
            }

            fn call(&mut self, req: Request<Body>) -> Self::Future {
                // CORS preflight bypass — see ApiKeyAuthService for rationale.
                if req.method() == Method::OPTIONS {
                    let fut = self.inner.call(req);
                    return Box::pin(fut);
                }

                let state = self.state.clone();
                let mut inner = self.inner.clone();

                Box::pin(async move {
                    let token = match extract_bearer(&req) {
                        Some(t) => t,
                        None => return Ok(jwt_unauthorized_response()),
                    };

                    let token_str = match std::str::from_utf8(token) {
                        Ok(s) => s,
                        Err(_) => return Ok(jwt_unauthorized_response()),
                    };

                    match jsonwebtoken::decode::<C>(
                        token_str,
                        &state.decoding_key,
                        &state.validation,
                    ) {
                        Ok(_) => inner.call(req).await,
                        Err(_) => Ok(jwt_unauthorized_response()),
                    }
                })
            }
        }

        /// Extract a Bearer token from the `Authorization` header.
        ///
        /// Returns borrowed bytes from the [`HeaderValue`] — no allocation.
        fn extract_bearer(req: &Request<Body>) -> Option<&[u8]> {
            req.headers()
                .get(header::AUTHORIZATION)?
                .as_bytes()
                .strip_prefix(b"Bearer ")
        }

        fn jwt_unauthorized_response() -> Response<Body> {
            let body = serde_json::json!({ "error": "Unauthorized" }).to_string();
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                // PANIC: static builder arguments; cannot fail.
                .expect("static unauthorized response is always valid")
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────────

    #[cfg(test)]
    mod tests {
        use super::*;
        use axum::{
            body::Body,
            http::{Method, Request, StatusCode},
        };
        use tower::{Service, ServiceExt};

        // ── ApiKeyConfig construction ────────────────────────────────────────────

        #[test]
        fn empty_key_list_is_rejected() {
            let err = ApiKeyConfig::new(&[]).unwrap_err();
            assert!(matches!(err, AuthConfigError::EmptyKeyList));
        }

        #[test]
        fn key_with_whitespace_is_rejected() {
            let err = ApiKeyConfig::new(&["valid-key", "bad key"]).unwrap_err();
            assert!(matches!(err, AuthConfigError::WhitespaceInKey));
        }

        #[test]
        fn key_with_leading_whitespace_is_rejected() {
            let err = ApiKeyConfig::new(&[" leading"]).unwrap_err();
            assert!(matches!(err, AuthConfigError::WhitespaceInKey));
        }

        #[test]
        fn key_with_trailing_whitespace_is_rejected() {
            let err = ApiKeyConfig::new(&["trailing "]).unwrap_err();
            assert!(matches!(err, AuthConfigError::WhitespaceInKey));
        }

        #[test]
        fn valid_single_key_is_accepted() {
            assert!(ApiKeyConfig::new(&["valid-key"]).is_ok());
        }

        #[test]
        fn valid_multiple_keys_are_accepted() {
            assert!(ApiKeyConfig::new(&["key-one", "key-two", "key-three"]).is_ok());
        }

        // ── Helper to build a test service ───────────────────────────────────────

        type OkFn = fn(
            Request<Body>,
        )
            -> std::future::Ready<Result<Response<Body>, std::convert::Infallible>>;
        type TestSvc = ApiKeyAuthService<tower::util::ServiceFn<OkFn>>;

        fn make_service(key: &str) -> TestSvc {
            let config = ApiKeyConfig::new(&[key]).expect("valid key");
            let layer = ApiKeyAuthLayer::new(config);
            layer.layer(tower::service_fn(|_req: Request<Body>| {
                std::future::ready(Ok::<_, std::convert::Infallible>(
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::empty())
                        .unwrap(),
                ))
            }))
        }

        // ── Authentication behaviour ─────────────────────────────────────────────

        #[tokio::test]
        async fn valid_bearer_token_returns_200() {
            let mut svc = make_service("my-secret-key");
            let req = Request::builder()
                .method(Method::GET)
                .header("Authorization", "Bearer my-secret-key")
                .body(Body::empty())
                .unwrap();
            let resp = svc.ready().await.unwrap().call(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn valid_x_pjs_api_key_returns_200() {
            let mut svc = make_service("my-secret-key");
            let req = Request::builder()
                .method(Method::GET)
                .header("X-PJS-API-Key", "my-secret-key")
                .body(Body::empty())
                .unwrap();
            let resp = svc.ready().await.unwrap().call(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn wrong_token_returns_401() {
            let mut svc = make_service("my-secret-key");
            let req = Request::builder()
                .method(Method::GET)
                .header("Authorization", "Bearer wrong-key")
                .body(Body::empty())
                .unwrap();
            let resp = svc.ready().await.unwrap().call(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn missing_header_returns_401() {
            let mut svc = make_service("my-secret-key");
            let req = Request::builder()
                .method(Method::GET)
                .body(Body::empty())
                .unwrap();
            let resp = svc.ready().await.unwrap().call(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn options_bypasses_auth() {
            // OPTIONS must pass through even with no auth header.
            let mut svc = make_service("my-secret-key");
            let req = Request::builder()
                .method(Method::OPTIONS)
                .body(Body::empty())
                .unwrap();
            let resp = svc.ready().await.unwrap().call(req).await.unwrap();
            // The inner handler returns 200; auth must not block OPTIONS.
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // ── matches_any timing correctness (structural, not wall-clock) ──────────

        #[test]
        fn matches_any_correct_single_key() {
            let config = ApiKeyConfig::new(&["secret"]).unwrap();
            let state = ApiKeyState {
                hmac_key: config.hmac_key,
                tags: config.keys,
            };
            assert!(matches_any(&state, b"secret"));
            assert!(!matches_any(&state, b"wrong"));
        }

        #[test]
        fn matches_any_correct_multiple_keys() {
            let config = ApiKeyConfig::new(&["key-a", "key-b", "key-c"]).unwrap();
            let state = ApiKeyState {
                hmac_key: config.hmac_key,
                tags: config.keys,
            };
            assert!(matches_any(&state, b"key-a"));
            assert!(matches_any(&state, b"key-b"));
            assert!(matches_any(&state, b"key-c"));
            assert!(!matches_any(&state, b"key-d"));
        }

        // ── extract_token ────────────────────────────────────────────────────────

        #[test]
        fn extract_token_bearer() {
            let req = Request::builder()
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap();
            assert_eq!(extract_token(&req), Some(b"test-token".as_slice()));
        }

        #[test]
        fn extract_token_x_pjs_api_key() {
            let req = Request::builder()
                .header("X-PJS-API-Key", "test-token")
                .body(Body::empty())
                .unwrap();
            assert_eq!(extract_token(&req), Some(b"test-token".as_slice()));
        }

        #[test]
        fn extract_token_none_when_absent() {
            let req = Request::builder().body(Body::empty()).unwrap();
            assert_eq!(extract_token(&req), None);
        }

        #[test]
        fn extract_token_bearer_preferred_over_x_pjs() {
            let req = Request::builder()
                .header("Authorization", "Bearer bearer-val")
                .header("X-PJS-API-Key", "api-key-val")
                .body(Body::empty())
                .unwrap();
            assert_eq!(extract_token(&req), Some(b"bearer-val".as_slice()));
        }
    }
}

// Re-export everything from the inner module under the feature gate.
#[cfg(feature = "http-server")]
pub use inner::{ApiKeyAuthLayer, ApiKeyAuthService, ApiKeyConfig, AuthConfigError};

#[cfg(all(feature = "http-server", feature = "http-auth-jwt"))]
pub use inner::{JwtAuthLayer, JwtAuthService, JwtConfig};
