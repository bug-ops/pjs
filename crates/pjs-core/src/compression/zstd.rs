//! Trained-dictionary zstd compression for PJS byte-level transport (Layer B).
//!
//! Provides [`ZstdDictionary`] (a validated opaque blob carrying the libzstd
//! dictionary) and [`ZstdDictCompressor`] (a stateless driver for training,
//! compression, and standalone decompression).
//!
//! The hot-path decompression used by [`crate::compression::secure::SecureCompressor`]
//! is intentionally **not** exposed here: it uses a streaming decoder routed
//! through `CompressionBombProtector` so the output-size guard still applies.
//! This module's `decompress` is only for callers that need a standalone,
//! non-bomb-protected path (e.g., tests or tools where the size is already known).
//!
//! Available only when `feature = "compression"` is enabled and the target is
//! not `wasm32`.

use crate::{Error, Result};

/// Maximum permitted dictionary size in bytes (112 KiB).
///
/// This is the **type invariant** of [`ZstdDictionary`]: any value of that type
/// satisfies `len() <= MAX_DICT_SIZE`. The constant is conservative — libzstd
/// can produce dictionaries up to 2 GiB, but large dicts inflate RSS on every
/// session and slow context initialisation. 112 KiB covers the sweet spot for
/// JSON-like payloads.
pub const MAX_DICT_SIZE: usize = 112 * 1024;

/// Number of training samples required before [`ZstdDictCompressor::train`] is
/// called.  Libzstd requires at least 8 samples; `N_TRAIN` is set to 32 so
/// the resulting dictionary captures representative variance across a session.
/// Below this threshold [`crate::domain::ports::dictionary_store::DictionaryStore::get_dictionary`]
/// returns `Ok(None)`.
pub const N_TRAIN: usize = 32;

/// Default zstd compression level used by [`ZstdDictCompressor::compress`].
///
/// Level 3 is the libzstd default: a good balance of speed and ratio for
/// repetitive JSON-like workloads. Pass an explicit level to
/// [`ZstdDictCompressor::compress_with_level`] if you need to tune it.
pub const DEFAULT_LEVEL: i32 = 3;

/// zstd dictionary magic bytes (little-endian `0xEC30A437`).
const ZSTD_MAGIC: [u8; 4] = [0x37, 0xA4, 0x30, 0xEC];

/// A validated, size-bounded zstd dictionary blob.
///
/// **Type invariant:** `self.len() <= MAX_DICT_SIZE` (112 KiB) and the first
/// four bytes are the zstd dictionary magic `0xEC30A437`. All public
/// constructors funnel through the private `new_checked` gate; callers outside
/// this module cannot construct an invalid value.
///
/// Sharing is performed once at the enum level via `Arc<ZstdDictionary>` in
/// [`crate::compression::secure::ByteCodec::ZstdDict`].  The inner `Vec<u8>`
/// is intentionally not wrapped in a second `Arc` — that would create
/// double indirection with no benefit.
///
/// # Examples
///
/// ```rust
/// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
/// # {
/// use pjson_rs::compression::zstd::{ZstdDictCompressor, ZstdDictionary, N_TRAIN};
///
/// // Build enough samples for training (at least 8 needed by libzstd; N_TRAIN = 32).
/// let item = b"{\"id\":1,\"name\":\"test\",\"value\":42,\"active\":true}";
/// let samples: Vec<Vec<u8>> = (0..N_TRAIN).map(|i| {
///     format!("{{\"id\":{i},\"name\":\"item\",\"value\":{},\"active\":true}}", i * 10)
///         .into_bytes()
/// }).collect();
///
/// let dict = ZstdDictCompressor::train(&samples, 65536).expect("training should succeed");
/// assert!(dict.len() <= 65536);
/// assert!(!dict.is_empty());
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZstdDictionary(Vec<u8>);

impl ZstdDictionary {
    /// Private constructor — the single enforcement point for the type invariant.
    fn new_checked(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::CompressionError("zstd: empty dictionary".into()));
        }
        if bytes.len() > MAX_DICT_SIZE {
            return Err(Error::CompressionError(format!(
                "zstd: dictionary size {} exceeds MAX_DICT_SIZE ({})",
                bytes.len(),
                MAX_DICT_SIZE
            )));
        }
        if bytes.len() < 4 || bytes[0..4] != ZSTD_MAGIC {
            return Err(Error::CompressionError(
                "zstd: invalid dictionary magic (expected 0xEC30A437)".into(),
            ));
        }
        Ok(Self(bytes))
    }

    /// Construct a [`ZstdDictionary`] from a raw byte blob produced by libzstd.
    ///
    /// Validates the magic header and the 112 KiB size cap.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CompressionError`] if:
    /// - `bytes` is empty
    /// - `bytes.len() > MAX_DICT_SIZE`
    /// - the first four bytes are not the zstd dictionary magic `0xEC30A437`
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use pjson_rs::compression::zstd::ZstdDictionary;
    ///
    /// // Empty bytes are rejected.
    /// assert!(ZstdDictionary::from_bytes(vec![]).is_err());
    ///
    /// // Bytes without the correct magic are rejected.
    /// assert!(ZstdDictionary::from_bytes(vec![0x00, 0x01, 0x02, 0x03]).is_err());
    ///
    /// // A blob larger than MAX_DICT_SIZE is rejected.
    /// use pjson_rs::compression::zstd::MAX_DICT_SIZE;
    /// let oversized = vec![0x37u8, 0xA4, 0x30, 0xEC]
    ///     .into_iter()
    ///     .chain(std::iter::repeat(0u8).take(MAX_DICT_SIZE))
    ///     .collect::<Vec<_>>();
    /// assert!(ZstdDictionary::from_bytes(oversized).is_err());
    /// # }
    /// ```
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::new_checked(bytes)
    }

    /// Returns the raw dictionary bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the dictionary size in bytes (always `<= MAX_DICT_SIZE`).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the dictionary has no bytes.
    ///
    /// This can never be `true` for a successfully constructed [`ZstdDictionary`]
    /// because `new_checked` rejects empty inputs. The method exists to satisfy
    /// Clippy's `len_without_is_empty` requirement.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Stateless driver for zstd dictionary operations.
///
/// All methods take the dictionary by reference. No internal state is retained
/// between calls; callers supply both the data and the dictionary each time.
///
/// The trained dictionary should be stored in
/// [`crate::infrastructure::repositories::InMemoryDictionaryStore`] (or a
/// custom [`crate::domain::ports::dictionary_store::DictionaryStore`] impl)
/// and shared via `Arc<ZstdDictionary>`.
///
/// # Examples
///
/// ```rust
/// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
/// # {
/// use pjson_rs::compression::zstd::{ZstdDictCompressor, N_TRAIN, MAX_DICT_SIZE};
///
/// let samples: Vec<Vec<u8>> = (0..N_TRAIN).map(|i| {
///     format!("{{\"id\":{i},\"key\":\"value\",\"score\":{}}}", i * 3).into_bytes()
/// }).collect();
///
/// let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
///
/// let data = b"{\"id\":99,\"key\":\"value\",\"score\":297}";
/// let compressed = ZstdDictCompressor::compress(data, &dict).unwrap();
/// let decompressed = ZstdDictCompressor::decompress(&compressed, &dict, data.len() * 2).unwrap();
/// assert_eq!(decompressed, data);
/// # }
/// ```
pub struct ZstdDictCompressor;

impl ZstdDictCompressor {
    /// Train a zstd dictionary from a corpus of sample byte strings.
    ///
    /// `max_dict_size` is **clamped** to [`MAX_DICT_SIZE`] before being passed to
    /// libzstd — even if the caller requests a larger dict, the type invariant of
    /// [`ZstdDictionary`] is always satisfied.
    ///
    /// Libzstd requires at least 8 samples; the PJS convention is to call this
    /// after accumulating [`N_TRAIN`] (32) samples for better dictionary quality.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CompressionError`] if:
    /// - `samples.len() < 8` (libzstd hard minimum)
    /// - libzstd training itself fails (e.g., samples too small or too uniform)
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use pjson_rs::compression::zstd::{ZstdDictCompressor, N_TRAIN, MAX_DICT_SIZE};
    ///
    /// let samples: Vec<Vec<u8>> = (0..N_TRAIN).map(|i| {
    ///     format!("{{\"seq\":{i},\"payload\":\"aaabbbccc{i}\"}}").into_bytes()
    /// }).collect();
    ///
    /// let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
    /// assert!(dict.len() <= MAX_DICT_SIZE);
    ///
    /// // Requesting a larger size is silently clamped.
    /// let dict2 = ZstdDictCompressor::train(&samples, usize::MAX).unwrap();
    /// assert!(dict2.len() <= MAX_DICT_SIZE);
    ///
    /// // Insufficient samples are rejected before calling libzstd.
    /// let few: Vec<Vec<u8>> = vec![b"data".to_vec(); 3];
    /// assert!(ZstdDictCompressor::train(&few, MAX_DICT_SIZE).is_err());
    /// # }
    /// ```
    pub fn train(samples: &[Vec<u8>], max_dict_size: usize) -> Result<ZstdDictionary> {
        // Libzstd requires ≥ 8 samples; reject early with a clear message.
        if samples.len() < 8 {
            return Err(Error::CompressionError(format!(
                "zstd: insufficient samples ({} provided, need >= 8)",
                samples.len()
            )));
        }
        let cap = max_dict_size.min(MAX_DICT_SIZE);
        let bytes = zstd::dict::from_samples(samples, cap)
            .map_err(|e| Error::CompressionError(format!("zstd: train: {e}")))?;
        // Defence-in-depth: re-check even if libzstd honoured the size cap.
        ZstdDictionary::new_checked(bytes)
    }

    /// Compress `data` using the dictionary at the default level ([`DEFAULT_LEVEL`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::CompressionError`] on libzstd failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use pjson_rs::compression::zstd::{ZstdDictCompressor, N_TRAIN, MAX_DICT_SIZE};
    ///
    /// let samples: Vec<Vec<u8>> = (0..N_TRAIN)
    ///     .map(|i| format!("{{\"n\":{i}}}").into_bytes())
    ///     .collect();
    /// let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
    /// let compressed = ZstdDictCompressor::compress(b"{\"n\":99}", &dict).unwrap();
    /// assert!(!compressed.is_empty());
    /// # }
    /// ```
    pub fn compress(data: &[u8], dict: &ZstdDictionary) -> Result<Vec<u8>> {
        Self::compress_with_level(data, dict, DEFAULT_LEVEL)
    }

    /// Compress `data` using the dictionary at an explicit compression level.
    ///
    /// Level must be in `[1, 22]`; libzstd clamps out-of-range values silently.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CompressionError`] on libzstd failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use pjson_rs::compression::zstd::{ZstdDictCompressor, N_TRAIN, MAX_DICT_SIZE};
    ///
    /// let samples: Vec<Vec<u8>> = (0..N_TRAIN)
    ///     .map(|i| format!("{{\"n\":{i}}}").into_bytes())
    ///     .collect();
    /// let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
    /// let compressed = ZstdDictCompressor::compress_with_level(b"{\"n\":99}", &dict, 1).unwrap();
    /// assert!(!compressed.is_empty());
    /// # }
    /// ```
    pub fn compress_with_level(data: &[u8], dict: &ZstdDictionary, level: i32) -> Result<Vec<u8>> {
        // TODO(#144 follow-up): per-session compressor cache once benchmarks justify it.
        let mut compressor = zstd::bulk::Compressor::with_dictionary(level, dict.as_bytes())
            .map_err(|e| Error::CompressionError(format!("zstd: compressor init: {e}")))?;
        compressor
            .compress(data)
            .map_err(|e| Error::CompressionError(format!("zstd: compress: {e}")))
    }

    /// Decompress `data` using the dictionary, capping output at `max_output` bytes.
    ///
    /// This is the **standalone** decompression path — for untrusted input routed
    /// through [`crate::compression::secure::SecureCompressor`], use
    /// [`crate::compression::secure::ByteCodec::ZstdDict`] instead, which passes the
    /// output through [`crate::security::CompressionBombDetector`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::CompressionError`] on libzstd failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use pjson_rs::compression::zstd::{ZstdDictCompressor, N_TRAIN, MAX_DICT_SIZE};
    ///
    /// let samples: Vec<Vec<u8>> = (0..N_TRAIN)
    ///     .map(|i| format!("{{\"n\":{i}}}").into_bytes())
    ///     .collect();
    /// let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
    /// let data = b"{\"n\":99}";
    /// let compressed = ZstdDictCompressor::compress(data, &dict).unwrap();
    /// let decompressed = ZstdDictCompressor::decompress(&compressed, &dict, 1024).unwrap();
    /// assert_eq!(decompressed.as_slice(), data.as_slice());
    /// # }
    /// ```
    pub fn decompress(data: &[u8], dict: &ZstdDictionary, max_output: usize) -> Result<Vec<u8>> {
        let mut decompressor = zstd::bulk::Decompressor::with_dictionary(dict.as_bytes())
            .map_err(|e| Error::CompressionError(format!("zstd: decompressor init: {e}")))?;
        decompressor
            .decompress(data, max_output)
            .map_err(|e| Error::CompressionError(format!("zstd: decompress: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a training corpus with `count` JSON samples.
    fn make_samples(count: usize) -> Vec<Vec<u8>> {
        (0..count)
            .map(|i| {
                format!(
                    r#"{{"id":{i},"name":"item-{i}","value":{val},"active":true}}"#,
                    val = i * 10
                )
                .into_bytes()
            })
            .collect()
    }

    // ~4 KiB of repetitive JSON — should compress well with a trained dict.
    fn repetitive_json() -> Vec<u8> {
        let item = br#"{"id":1,"name":"test","value":42,"active":true}"#;
        item.repeat(100)
    }

    #[test]
    fn test_train_compress_decompress_roundtrip() {
        let samples = make_samples(N_TRAIN);
        let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();

        let data = repetitive_json();
        let compressed = ZstdDictCompressor::compress(&data, &dict).unwrap();
        let decompressed =
            ZstdDictCompressor::decompress(&compressed, &dict, data.len() * 2).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_train_insufficient_samples_error() {
        let samples = make_samples(3);
        let err = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("insufficient samples"),
            "error should mention insufficient samples: {msg}"
        );
    }

    #[test]
    fn test_train_clamps_to_max_dict_size() {
        let samples = make_samples(N_TRAIN);
        // Requesting more than MAX_DICT_SIZE must still produce a valid (≤ cap) dict.
        let dict = ZstdDictCompressor::train(&samples, usize::MAX).unwrap();
        assert!(
            dict.len() <= MAX_DICT_SIZE,
            "dict size {} exceeds MAX_DICT_SIZE",
            dict.len()
        );
    }

    #[test]
    fn test_from_bytes_rejects_empty() {
        assert!(ZstdDictionary::from_bytes(vec![]).is_err());
    }

    #[test]
    fn test_from_bytes_rejects_invalid_magic() {
        assert!(ZstdDictionary::from_bytes(vec![0x00, 0x01, 0x02, 0x03]).is_err());
    }

    #[test]
    fn test_from_bytes_rejects_oversized() {
        let mut bytes = ZSTD_MAGIC.to_vec();
        bytes.extend(std::iter::repeat_n(0u8, MAX_DICT_SIZE));
        // Total length = 4 + MAX_DICT_SIZE > MAX_DICT_SIZE → must fail.
        assert!(ZstdDictionary::from_bytes(bytes).is_err());
    }

    #[test]
    fn test_compress_with_level() {
        let samples = make_samples(N_TRAIN);
        let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
        let data = repetitive_json();

        // Level 1 and level 9 must both produce valid compressed output.
        for level in [1, 9] {
            let c = ZstdDictCompressor::compress_with_level(&data, &dict, level).unwrap();
            let d = ZstdDictCompressor::decompress(&c, &dict, data.len() * 2).unwrap();
            assert_eq!(d, data, "level {level} roundtrip failed");
        }
    }

    #[test]
    fn test_dictionary_equality() {
        let samples = make_samples(N_TRAIN);
        let d1 = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
        let d2 = d1.clone();
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_is_empty_is_always_false_for_valid_dict() {
        let samples = make_samples(N_TRAIN);
        let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
        assert!(!dict.is_empty());
    }
}
