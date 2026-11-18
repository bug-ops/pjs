//! Shared helpers for the `/metrics` acceptance tests (tracks F1 and F2).
//!
//! The Prometheus recorder is a process-global singleton, so every test file
//! that scrapes it is its own test binary and keeps to metric labels no other
//! test in that binary produces.
#![allow(dead_code)]

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use tower::ServiceExt;

/// Sends `method uri` (no body) and returns the status.
pub async fn send(app: &Router, method: &str, uri: &str) -> StatusCode {
    send_with_body(app, method, uri, "").await.0
}

/// Sends `method uri` with a JSON body and returns the status and body text.
pub async fn send_with_body(
    app: &Router,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, String) {
    let request = Request::builder()
        .method(Method::from_bytes(method.as_bytes()).expect("valid method token"))
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("request builds");
    let response = app.clone().oneshot(request).await.expect("responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body reads");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// `GET /metrics` body.
pub async fn scrape(app: &Router) -> String {
    let (status, body) = send_with_body(app, "GET", "/metrics", "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// Scrapes until `check` accepts the body, for at most 5 seconds; returns the last body.
pub async fn scrape_until(app: &Router, check: impl Fn(&str) -> bool) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let body = scrape(app).await;
        if check(&body) || std::time::Instant::now() >= deadline {
            return body;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// Serves a WebSocket router backed by `transport` on a random localhost port.
#[cfg(all(feature = "websocket-server", feature = "websocket-client"))]
pub async fn spawn_ws_server(
    transport: pjson_rs::infrastructure::websocket::AxumWebSocketTransport,
) -> std::net::SocketAddr {
    use pjson_rs::infrastructure::websocket::server::create_websocket_router;

    let app = create_websocket_router().with_state(std::sync::Arc::new(transport));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    addr
}

/// Exposition lines of the series called `name` (labels present or not).
pub fn lines<'a>(body: &'a str, name: &str) -> Vec<&'a str> {
    body.lines()
        .filter(|line| {
            line.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('{') || rest.starts_with(' '))
        })
        .collect()
}

/// Value of the first `name` line carrying every `label="value"` pair.
pub fn value_of(body: &str, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
    lines(body, name)
        .into_iter()
        .find(|line| {
            labels
                .iter()
                .all(|(key, value)| line.contains(&format!(r#"{key}="{value}""#)))
        })
        .and_then(|line| line.rsplit(' ').next())
        .and_then(|value| value.parse().ok())
}

/// Every distinct value the label `key` takes across the `name` lines.
pub fn label_values(body: &str, name: &str, key: &str) -> Vec<String> {
    let needle = format!(r#"{key}=""#);
    let mut values: Vec<String> = lines(body, name)
        .into_iter()
        .filter_map(|line| {
            let start = line.find(&needle)? + needle.len();
            let end = line[start..].find('"')?;
            Some(line[start..start + end].to_owned())
        })
        .collect();
    values.sort();
    values.dedup();
    values
}
