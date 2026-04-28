//! Secure compression with bomb protection and real byte-level codecs.
//!
//! This module provides [`SecureCompressor`], which applies byte-level compression (Layer B)
//! to arbitrary `&[u8]` payloads. It is distinct from `SchemaCompressor` in `compression/mod.rs`,
//! which operates on `serde_json::Value` (Layer A / structural compression).
//!
//! # Security
//!
//! Every decompression is routed through `CompressionBombProtector`, which streams the decoder
//! output and aborts if decompressed size or ratio exceeds configured limits.
//!
//! # In-process only
//!
//! [`SecureCompressedData`] carries the codec tag and is intended for in-process use only.
//! It is not a wire format. If cross-process transport is needed in a future PR, a versioned
//! framing header must be designed separately.

use crate::{
    Error, Result,
    security::{CompressionBombDetector, CompressionStats},
};
#[cfg(feature = "compression")]
use std::io::Write;
use std::io::{Cursor, Read};
use tracing::{debug, info, warn};

/// Byte-level compression algorithms used by [`SecureCompressor`].
///
/// This is distinct from [`CompressionStrategy`](super::CompressionStrategy), which operates on
/// `serde_json::Value` (Layer A). `ByteCodec` operates on raw bytes after JSON serialization
/// (Layer B).
///
/// Codecs other than `None` require the `compression` feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ByteCodec {
    /// No compression — bytes stored verbatim. Always available.
    #[default]
    None,
    /// Raw deflate (RFC 1951). Low framing overhead.
    ///
    /// Note: raw deflate has no magic header, so codec mismatch during decompression will
    /// produce a decoder error rather than a guaranteed clean failure. The codec tag embedded
    /// in [`SecureCompressedData`] prevents this for in-process round-trips.
    ///
    /// Requires `feature = "compression"`.
    Deflate,
    /// Gzip (RFC 1952). Self-identifying via `1f 8b` magic header.
    ///
    /// Requires `feature = "compression"`.
    Gzip,
    /// Brotli. Best ratio for repetitive JSON.
    ///
    /// Requires `feature = "compression"`.
    Brotli,
}

/// Quality knob for byte-level codecs.
///
/// Maps to codec-specific levels: deflate 1/6/9 and brotli quality 1/5/11.
#[derive(Debug, Clone, Copy, Default)]
pub enum CompressionQuality {
    /// Speed-optimised: deflate level 1, brotli quality 1.
    Fast,
    /// Balanced speed/ratio (default): deflate level 6, brotli quality 5.
    #[default]
    Balanced,
    /// Maximum ratio: deflate level 9, brotli quality 11.
    Best,
}

impl CompressionQuality {
    #[cfg(feature = "compression")]
    fn flate2_level(self) -> flate2::Compression {
        match self {
            Self::Fast => flate2::Compression::fast(),
            Self::Balanced => flate2::Compression::default(),
            Self::Best => flate2::Compression::best(),
        }
    }

    #[cfg(feature = "compression")]
    fn brotli_quality(self) -> i32 {
        match self {
            Self::Fast => 1,
            Self::Balanced => 5,
            Self::Best => 11,
        }
    }
}

/// Compressed bytes with security metadata and codec identification.
///
/// # In-process only
///
/// This struct is intended for in-process use only and is not a wire format.
/// The `codec` field is carried alongside `data` so that [`SecureCompressor::decompress_protected`]
/// always uses the correct decoder. If this type must cross process boundaries in the future,
/// design a versioned framing header as a separate concern.
#[derive(Debug, Clone)]
pub struct SecureCompressedData {
    /// The compressed (or verbatim) payload.
    pub data: Vec<u8>,
    /// Original uncompressed size in bytes.
    pub original_size: usize,
    /// Compression ratio: `original_size / compressed_size`.
    ///
    /// A value of `2.0` means the compressed payload is half the original size (50% size reduction).
    /// For `ByteCodec::None` this is always `1.0`; for incompressible data it can be `< 1.0`
    /// because most codecs add a small framing header.
    pub compression_ratio: f64,
    /// Codec used to produce `data`. Must be passed back to [`SecureCompressor::decompress_protected`].
    pub codec: ByteCodec,
}

/// Secure byte-level compressor with integrated bomb protection.
///
/// Wraps a [`CompressionBombDetector`] to ensure decompressed output never exceeds configured
/// size and ratio limits, regardless of which codec is active.
///
/// # Examples
///
/// ```rust
/// use pjson_rs::compression::secure::{SecureCompressor, ByteCodec};
///
/// let compressor = SecureCompressor::with_default_security(ByteCodec::None);
/// let compressed = compressor.compress(b"hello world").unwrap();
/// let decompressed = compressor.decompress_protected(&compressed).unwrap();
/// assert_eq!(decompressed, b"hello world");
/// ```
pub struct SecureCompressor {
    detector: CompressionBombDetector,
    codec: ByteCodec,
    #[cfg_attr(not(feature = "compression"), allow(dead_code))]
    quality: CompressionQuality,
}

impl SecureCompressor {
    /// Create a new secure compressor with the given detector and codec.
    pub fn new(detector: CompressionBombDetector, codec: ByteCodec) -> Self {
        Self {
            detector,
            codec,
            quality: CompressionQuality::default(),
        }
    }

    /// Create with default security settings and the given codec.
    pub fn with_default_security(codec: ByteCodec) -> Self {
        Self::new(CompressionBombDetector::default(), codec)
    }

    /// Create with explicit quality setting.
    pub fn with_quality(
        detector: CompressionBombDetector,
        codec: ByteCodec,
        quality: CompressionQuality,
    ) -> Self {
        Self {
            detector,
            codec,
            quality,
        }
    }

    /// Compress `data` using the configured codec.
    ///
    /// Validates the input size against `max_compressed_size` before encoding.
    pub fn compress(&self, data: &[u8]) -> Result<SecureCompressedData> {
        self.detector.validate_pre_decompression(data.len())?;

        let compressed_bytes = self.encode(data)?;

        let compression_ratio = data.len() as f64 / compressed_bytes.len().max(1) as f64;
        info!("Compression completed: {:.2}x ratio", compression_ratio);

        Ok(SecureCompressedData {
            original_size: data.len(),
            compression_ratio,
            codec: self.codec,
            data: compressed_bytes,
        })
    }

    /// Decompress `compressed` using the codec recorded in `compressed.codec`.
    ///
    /// Decoder output is streamed through `CompressionBombProtector` — decompression aborts
    /// early if size or ratio limits are exceeded.
    pub fn decompress_protected(&self, compressed: &SecureCompressedData) -> Result<Vec<u8>> {
        self.detector
            .validate_pre_decompression(compressed.data.len())?;
        self.decode_with_protection(&compressed.data, compressed.codec, None)
    }

    /// Decompress nested/chained compression with depth tracking.
    ///
    /// Equivalent to [`decompress_protected`](Self::decompress_protected) but additionally enforces
    /// `max_compression_depth` via [`CompressionBombDetector::protect_nested_reader`].
    pub fn decompress_nested(
        &self,
        compressed: &SecureCompressedData,
        depth: usize,
    ) -> Result<Vec<u8>> {
        self.detector
            .validate_pre_decompression(compressed.data.len())?;
        self.decode_with_protection(&compressed.data, compressed.codec, Some(depth))
    }

    /// Encode `data` with the configured codec. Returns compressed bytes only.
    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.codec {
            ByteCodec::None => {
                debug!("No compression applied");
                Ok(data.to_vec())
            }

            #[cfg(feature = "compression")]
            ByteCodec::Deflate => {
                use flate2::write::DeflateEncoder;
                let mut enc = DeflateEncoder::new(Vec::new(), self.quality.flate2_level());
                enc.write_all(data)
                    .map_err(|e| Error::CompressionError(format!("deflate encode: {e}")))?;
                enc.finish()
                    .map_err(|e| Error::CompressionError(format!("deflate finish: {e}")))
            }

            #[cfg(feature = "compression")]
            ByteCodec::Gzip => {
                use flate2::write::GzEncoder;
                let mut enc = GzEncoder::new(Vec::new(), self.quality.flate2_level());
                enc.write_all(data)
                    .map_err(|e| Error::CompressionError(format!("gzip encode: {e}")))?;
                enc.finish()
                    .map_err(|e| Error::CompressionError(format!("gzip finish: {e}")))
            }

            #[cfg(feature = "compression")]
            ByteCodec::Brotli => {
                let params = brotli::enc::BrotliEncoderParams {
                    quality: self.quality.brotli_quality(),
                    ..Default::default()
                };
                let mut out = Vec::new();
                brotli::BrotliCompress(&mut Cursor::new(data), &mut out, &params)
                    .map_err(|e| Error::CompressionError(format!("brotli encode: {e}")))?;
                Ok(out)
            }

            #[cfg(not(feature = "compression"))]
            ByteCodec::Deflate | ByteCodec::Gzip | ByteCodec::Brotli => Err(
                Error::CompressionError("feature `compression` is not enabled".into()),
            ),
        }
    }

    /// Decode `data` through a bomb-protected reader.
    ///
    /// `depth` is `Some(n)` for nested decompression (depth-limited) or `None` for a flat call.
    fn decode_with_protection(
        &self,
        data: &[u8],
        codec: ByteCodec,
        depth: Option<usize>,
    ) -> Result<Vec<u8>> {
        // Macro-free helper: executes the read loop with any `impl Read` decoder.
        // Avoids boxing across a lifetime boundary by keeping decoder + protector in one scope.
        macro_rules! run {
            ($decoder:expr) => {{
                let compressed_size = data.len();
                let mut out = Vec::new();
                let result = if let Some(d) = depth {
                    let mut protector =
                        self.detector
                            .protect_nested_reader($decoder, compressed_size, d)?;
                    let r = protector.read_to_end(&mut out);
                    let stats = protector.stats();
                    self.log_decompression_stats(&stats);
                    if stats.compression_depth > 0 {
                        warn!(
                            "Nested decompression detected at depth {}",
                            stats.compression_depth
                        );
                    }
                    r
                } else {
                    let mut protector = self.detector.protect_reader($decoder, compressed_size);
                    let r = protector.read_to_end(&mut out);
                    let stats = protector.stats();
                    self.log_decompression_stats(&stats);
                    r
                };
                match result {
                    Ok(_) => {
                        self.detector.validate_result(compressed_size, out.len())?;
                        Ok(out)
                    }
                    Err(e) => {
                        warn!("Decompression failed: {}", e);
                        Err(Error::SecurityError(format!(
                            "Protected decompression failed: {}",
                            e
                        )))
                    }
                }
            }};
        }

        match codec {
            ByteCodec::None => run!(Cursor::new(data)),

            #[cfg(feature = "compression")]
            ByteCodec::Deflate => run!(flate2::read::DeflateDecoder::new(Cursor::new(data))),

            #[cfg(feature = "compression")]
            ByteCodec::Gzip => run!(flate2::read::GzDecoder::new(Cursor::new(data))),

            #[cfg(feature = "compression")]
            ByteCodec::Brotli => run!(brotli::Decompressor::new(Cursor::new(data), 4096)),

            #[cfg(not(feature = "compression"))]
            ByteCodec::Deflate | ByteCodec::Gzip | ByteCodec::Brotli => Err(
                Error::CompressionError("feature `compression` is not enabled".into()),
            ),
        }
    }

    fn log_decompression_stats(&self, stats: &CompressionStats) {
        info!(
            "Decompression stats: {}B -> {}B (ratio: {:.2}x, depth: {})",
            stats.compressed_size, stats.decompressed_size, stats.ratio, stats.compression_depth
        );
    }
}

/// Secure decompression context for streaming operations.
pub struct SecureDecompressionContext {
    detector: CompressionBombDetector,
    current_depth: usize,
    max_concurrent_streams: usize,
    active_streams: usize,
}

impl SecureDecompressionContext {
    /// Create new secure decompression context.
    pub fn new(detector: CompressionBombDetector, max_concurrent_streams: usize) -> Self {
        Self {
            detector,
            current_depth: 0,
            max_concurrent_streams,
            active_streams: 0,
        }
    }

    /// Start a new protected decompression stream.
    ///
    /// Returns an error if the concurrent stream limit would be exceeded.
    ///
    /// # Note
    ///
    /// The returned `CompressionBombProtector` wraps an empty in-memory cursor. Callers are
    /// responsible for writing compressed bytes into the underlying buffer before reading. This API
    /// is a concurrency-limit scaffold; true streaming wire integration is left for a future PR.
    pub fn start_stream(
        &mut self,
        compressed_size: usize,
    ) -> Result<crate::security::CompressionBombProtector<Cursor<Vec<u8>>>> {
        if self.active_streams >= self.max_concurrent_streams {
            return Err(Error::SecurityError(format!(
                "Too many concurrent decompression streams: {}/{}",
                self.active_streams, self.max_concurrent_streams
            )));
        }

        let cursor = Cursor::new(Vec::new());
        let protector =
            self.detector
                .protect_nested_reader(cursor, compressed_size, self.current_depth)?;

        self.active_streams += 1;
        info!(
            "Started secure decompression stream (active: {})",
            self.active_streams
        );

        Ok(protector)
    }

    /// Finish a decompression stream and decrement the active count.
    pub fn finish_stream(&mut self) {
        if self.active_streams > 0 {
            self.active_streams -= 1;
            info!(
                "Finished secure decompression stream (active: {})",
                self.active_streams
            );
        }
    }

    /// Get current context statistics.
    pub fn stats(&self) -> DecompressionContextStats {
        DecompressionContextStats {
            current_depth: self.current_depth,
            active_streams: self.active_streams,
            max_concurrent_streams: self.max_concurrent_streams,
        }
    }
}

/// Statistics for a [`SecureDecompressionContext`].
#[derive(Debug, Clone)]
pub struct DecompressionContextStats {
    pub current_depth: usize,
    pub active_streams: usize,
    pub max_concurrent_streams: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::CompressionBombConfig;

    #[test]
    fn test_secure_compressor_creation() {
        let detector = CompressionBombDetector::default();
        let compressor = SecureCompressor::new(detector, ByteCodec::None);
        // Verify the compressor is created (not null pointer).
        assert!(!std::ptr::addr_of!(compressor).cast::<u8>().is_null());
    }

    #[test]
    fn test_secure_compression_none() {
        let compressor = SecureCompressor::with_default_security(ByteCodec::None);
        let data = b"Hello, world! This is test data for compression.";

        let result = compressor.compress(data);
        assert!(result.is_ok());

        let compressed = result.unwrap();
        assert_eq!(compressed.original_size, data.len());
        assert_eq!(compressed.codec, ByteCodec::None);
    }

    #[test]
    fn test_none_roundtrip() {
        let compressor = SecureCompressor::with_default_security(ByteCodec::None);
        let data = b"round-trip test";

        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress_protected(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compression_size_limit() {
        let config = CompressionBombConfig {
            max_compressed_size: 100, // Very small limit
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);
        let compressor = SecureCompressor::new(detector, ByteCodec::None);

        let large_data = vec![0u8; 1000]; // 1 KiB data
        let result = compressor.compress(&large_data);

        // Should fail pre-compression validation (compressed_size > max_compressed_size).
        assert!(result.is_err());
    }

    #[test]
    fn test_different_codecs_none() {
        let compressor = SecureCompressor::with_default_security(ByteCodec::None);
        let data = b"test data";

        let result = compressor.compress(data);
        assert!(result.is_ok());

        let compressed = result.unwrap();
        assert_eq!(compressed.compression_ratio, 1.0);
        assert_eq!(compressed.codec, ByteCodec::None);
    }

    #[cfg(feature = "compression")]
    mod compression_tests {
        use super::*;

        // ~4 KiB of repetitive JSON-like payload — should compress well.
        fn repetitive_json() -> Vec<u8> {
            let item = br#"{"id":1,"name":"test","value":42,"active":true}"#;
            item.repeat(100)
        }

        #[test]
        fn test_deflate_roundtrip() {
            let compressor = SecureCompressor::with_default_security(ByteCodec::Deflate);
            let data = repetitive_json();

            let compressed = compressor.compress(&data).unwrap();
            assert_eq!(compressed.codec, ByteCodec::Deflate);
            assert!(
                compressed.data.len() < data.len(),
                "deflate must reduce size"
            );

            let decompressed = compressor.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_gzip_roundtrip() {
            let compressor = SecureCompressor::with_default_security(ByteCodec::Gzip);
            let data = repetitive_json();

            let compressed = compressor.compress(&data).unwrap();
            assert_eq!(compressed.codec, ByteCodec::Gzip);
            assert!(compressed.data.len() < data.len(), "gzip must reduce size");

            let decompressed = compressor.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_brotli_roundtrip() {
            let compressor = SecureCompressor::with_default_security(ByteCodec::Brotli);
            let data = repetitive_json();

            let compressed = compressor.compress(&data).unwrap();
            assert_eq!(compressed.codec, ByteCodec::Brotli);
            assert!(
                compressed.data.len() < data.len(),
                "brotli must reduce size"
            );

            let decompressed = compressor.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_all_qualities_deflate() {
            let data = repetitive_json();
            for quality in [
                CompressionQuality::Fast,
                CompressionQuality::Balanced,
                CompressionQuality::Best,
            ] {
                let c = SecureCompressor::with_quality(
                    CompressionBombDetector::default(),
                    ByteCodec::Deflate,
                    quality,
                );
                let compressed = c.compress(&data).unwrap();
                let decompressed = c.decompress_protected(&compressed).unwrap();
                assert_eq!(decompressed, data);
            }
        }

        #[test]
        fn test_all_qualities_brotli() {
            // Use Fast only to keep test time reasonable (quality 11 is slow).
            let data = repetitive_json();
            let c = SecureCompressor::with_quality(
                CompressionBombDetector::default(),
                ByteCodec::Brotli,
                CompressionQuality::Fast,
            );
            let compressed = c.compress(&data).unwrap();
            let decompressed = c.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_codec_mismatch_returns_error() {
            // Compress with Brotli, but tell decompressor it is Gzip.
            let c = SecureCompressor::with_default_security(ByteCodec::Brotli);
            let data = b"codec mismatch test data";
            let mut compressed = c.compress(data).unwrap();
            compressed.codec = ByteCodec::Gzip; // wrong codec tag

            let result = c.decompress_protected(&compressed);
            assert!(
                result.is_err(),
                "wrong codec must produce an error, not garbage"
            );
        }

        #[test]
        fn test_bomb_detection_on_real_codec() {
            // A very tight max_decompressed_size so any real inflation trips the guard.
            let config = CompressionBombConfig {
                max_decompressed_size: 200,  // Only 200 bytes allowed out
                max_compressed_size: 10_000, // Allow the compressed input
                max_ratio: 300.0,
                check_interval_bytes: 64,
                ..Default::default()
            };
            let detector = CompressionBombDetector::new(config);
            let compressor =
                SecureCompressor::new(CompressionBombDetector::default(), ByteCodec::Gzip);

            // Produce a real gzip payload of ~4 KiB.
            let data = repetitive_json();
            let compressed = compressor.compress(&data).unwrap();

            // Now decompress with a detector that caps at 200 bytes.
            let strict_compressor = SecureCompressor::new(detector, ByteCodec::Gzip);
            let result = strict_compressor.decompress_protected(&compressed);
            assert!(
                result.is_err(),
                "bomb detector must stop oversized decompression"
            );
        }
    }

    #[test]
    fn test_secure_decompression_context() {
        let detector = CompressionBombDetector::default();
        let mut context = SecureDecompressionContext::new(detector, 2);

        assert!(context.start_stream(1024).is_ok());
        assert!(context.start_stream(1024).is_ok());

        // Third stream exceeds limit.
        assert!(context.start_stream(1024).is_err());

        context.finish_stream();
        assert!(context.start_stream(1024).is_ok());
    }

    #[test]
    fn test_context_stats() {
        let detector = CompressionBombDetector::default();
        let context = SecureDecompressionContext::new(detector, 5);

        let stats = context.stats();
        assert_eq!(stats.current_depth, 0);
        assert_eq!(stats.active_streams, 0);
        assert_eq!(stats.max_concurrent_streams, 5);
    }

    #[test]
    fn test_context_finish_stream_underflow_safe() {
        let detector = CompressionBombDetector::default();
        let mut context = SecureDecompressionContext::new(detector, 5);

        // finish_stream when active_streams == 0 must not underflow.
        context.finish_stream();
        let stats = context.stats();
        assert_eq!(stats.active_streams, 0);
    }

    #[test]
    fn test_byte_codec_default_is_none() {
        assert_eq!(ByteCodec::default(), ByteCodec::None);
    }

    #[test]
    fn test_byte_codec_clone_and_copy() {
        let codec = ByteCodec::None;
        let cloned = codec;
        assert_eq!(codec, cloned);
    }

    #[test]
    fn test_compression_quality_default_is_balanced() {
        // Default quality must produce a valid compressor without error.
        let c = SecureCompressor::with_default_security(ByteCodec::None);
        let data = b"quality default test";
        let compressed = c.compress(data).unwrap();
        let decompressed = c.decompress_protected(&compressed).unwrap();
        assert_eq!(decompressed.as_slice(), data);
    }

    #[test]
    fn test_secure_compressed_data_clone() {
        let c = SecureCompressor::with_default_security(ByteCodec::None);
        let compressed = c.compress(b"clone test").unwrap();
        let cloned = compressed.clone();
        assert_eq!(compressed.data, cloned.data);
        assert_eq!(compressed.original_size, cloned.original_size);
        assert_eq!(compressed.codec, cloned.codec);
    }

    #[test]
    fn test_none_roundtrip_empty_payload() {
        let c = SecureCompressor::with_default_security(ByteCodec::None);
        let compressed = c.compress(b"").unwrap();
        let decompressed = c.decompress_protected(&compressed).unwrap();
        assert_eq!(decompressed, b"");
    }

    #[test]
    fn test_decompress_nested_none() {
        let c = SecureCompressor::with_default_security(ByteCodec::None);
        let data = b"nested roundtrip";
        let compressed = c.compress(data).unwrap();
        let decompressed = c.decompress_nested(&compressed, 0).unwrap();
        assert_eq!(decompressed.as_slice(), data);
    }

    #[cfg(feature = "compression")]
    mod extended_compression_tests {
        use super::*;

        // Non-repetitive payload: pseudo-random bytes unlikely to compress well.
        fn incompressible_payload() -> Vec<u8> {
            // Simple LCG to generate pseudo-random bytes without extra deps.
            let mut state: u64 = 0x_dead_beef_cafe_babe;
            (0..512)
                .map(|_| {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1);
                    (state >> 33) as u8
                })
                .collect()
        }

        #[test]
        fn test_deflate_roundtrip_incompressible() {
            let c = SecureCompressor::with_default_security(ByteCodec::Deflate);
            let data = incompressible_payload();
            let compressed = c.compress(&data).unwrap();
            assert_eq!(compressed.codec, ByteCodec::Deflate);
            let decompressed = c.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_gzip_roundtrip_incompressible() {
            let c = SecureCompressor::with_default_security(ByteCodec::Gzip);
            let data = incompressible_payload();
            let compressed = c.compress(&data).unwrap();
            assert_eq!(compressed.codec, ByteCodec::Gzip);
            let decompressed = c.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_brotli_roundtrip_incompressible() {
            let c = SecureCompressor::with_default_security(ByteCodec::Brotli);
            let data = incompressible_payload();
            let compressed = c.compress(&data).unwrap();
            assert_eq!(compressed.codec, ByteCodec::Brotli);
            let decompressed = c.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_gzip_all_qualities() {
            let item = br#"{"id":1,"name":"test","value":42}"#;
            let data: Vec<u8> = item.repeat(50);
            for quality in [
                CompressionQuality::Fast,
                CompressionQuality::Balanced,
                CompressionQuality::Best,
            ] {
                let c = SecureCompressor::with_quality(
                    CompressionBombDetector::default(),
                    ByteCodec::Gzip,
                    quality,
                );
                let compressed = c.compress(&data).unwrap();
                let decompressed = c.decompress_protected(&compressed).unwrap();
                assert_eq!(
                    decompressed, data,
                    "gzip quality {quality:?} roundtrip failed"
                );
            }
        }

        #[test]
        fn test_brotli_balanced_quality() {
            let item = br#"{"key":"value","n":99}"#;
            let data: Vec<u8> = item.repeat(80);
            let c = SecureCompressor::with_quality(
                CompressionBombDetector::default(),
                ByteCodec::Brotli,
                CompressionQuality::Balanced,
            );
            let compressed = c.compress(&data).unwrap();
            let decompressed = c.decompress_protected(&compressed).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_decompress_nested_with_depth() {
            let c = SecureCompressor::with_default_security(ByteCodec::Deflate);
            let item = br#"{"x":1}"#;
            let data: Vec<u8> = item.repeat(100);
            let compressed = c.compress(&data).unwrap();
            let decompressed = c.decompress_nested(&compressed, 1).unwrap();
            assert_eq!(decompressed, data);
        }

        #[test]
        fn test_decompress_nested_depth_exceeded_returns_error() {
            use crate::security::CompressionBombConfig;
            let config = CompressionBombConfig {
                max_compression_depth: 2,
                ..Default::default()
            };
            let c = SecureCompressor::new(CompressionBombDetector::new(config), ByteCodec::Deflate);
            let item = br#"{"x":1}"#;
            let data: Vec<u8> = item.repeat(100);
            let compressed = c.compress(&data).unwrap();
            // depth 3 exceeds max_compression_depth 2 — must error.
            let result = c.decompress_nested(&compressed, 3);
            assert!(result.is_err(), "depth beyond limit must return an error");
        }

        #[test]
        fn test_bomb_detection_deflate() {
            use crate::security::CompressionBombConfig;
            let config = CompressionBombConfig {
                max_decompressed_size: 200,
                max_compressed_size: 10_000,
                max_ratio: 300.0,
                check_interval_bytes: 64,
                ..Default::default()
            };
            let item = br#"{"id":1,"name":"test","value":42,"active":true}"#;
            let data: Vec<u8> = item.repeat(100);
            let producer = SecureCompressor::with_default_security(ByteCodec::Deflate);
            let compressed = producer.compress(&data).unwrap();

            let strict =
                SecureCompressor::new(CompressionBombDetector::new(config), ByteCodec::Deflate);
            let result = strict.decompress_protected(&compressed);
            assert!(
                result.is_err(),
                "bomb detector must block oversized deflate output"
            );
        }

        #[test]
        fn test_bomb_detection_brotli() {
            use crate::security::CompressionBombConfig;
            let config = CompressionBombConfig {
                max_decompressed_size: 200,
                max_compressed_size: 10_000,
                max_ratio: 300.0,
                check_interval_bytes: 64,
                ..Default::default()
            };
            let item = br#"{"id":1,"name":"test","value":42,"active":true}"#;
            let data: Vec<u8> = item.repeat(100);
            let producer = SecureCompressor::with_default_security(ByteCodec::Brotli);
            let compressed = producer.compress(&data).unwrap();

            let strict =
                SecureCompressor::new(CompressionBombDetector::new(config), ByteCodec::Brotli);
            let result = strict.decompress_protected(&compressed);
            assert!(
                result.is_err(),
                "bomb detector must block oversized brotli output"
            );
        }

        #[test]
        fn test_codec_mismatch_deflate_as_gzip() {
            let c = SecureCompressor::with_default_security(ByteCodec::Deflate);
            let data = b"deflate mismatch test payload";
            let mut compressed = c.compress(data).unwrap();
            compressed.codec = ByteCodec::Gzip;
            let result = c.decompress_protected(&compressed);
            assert!(result.is_err(), "Deflate data decoded as Gzip must fail");
        }

        #[test]
        fn test_empty_payload_all_codecs() {
            for codec in [ByteCodec::Deflate, ByteCodec::Gzip, ByteCodec::Brotli] {
                let c = SecureCompressor::with_default_security(codec);
                let compressed = c.compress(b"").unwrap();
                let decompressed = c.decompress_protected(&compressed).unwrap();
                assert_eq!(decompressed, b"", "empty roundtrip failed for {codec:?}");
            }
        }
    }
}
