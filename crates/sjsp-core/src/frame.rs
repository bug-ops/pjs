//! Frame format and utilities for SJSP protocol

use bytes::Bytes;
use bytemuck::{Pod, Zeroable};
use smallvec::SmallVec;

use crate::{Error, Result, SemanticMeta};

/// Unique identifier for schemas
pub type SchemaId = u32;

/// Zero-copy frame structure optimized for cache-line alignment
#[repr(C, align(64))]
#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame header with metadata
    pub header: FrameHeader,
    /// Payload as zero-copy bytes
    pub payload: Bytes,
    /// Optional semantic annotations for optimization hints
    pub semantics: Option<SemanticMeta>,
}

/// Packed frame header for wire format efficiency
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FrameHeader {
    /// Protocol version (currently 1)
    pub version: u8,
    /// Frame type and processing flags
    pub flags: FrameFlags,
    /// Sequence number for ordering and deduplication
    pub sequence: u64,
    /// Payload length in bytes
    pub length: u32,
    /// Optional schema ID for validation
    pub schema_id: u32, // 0 means no schema
    /// CRC32C checksum of payload (optional)
    pub checksum: u32, // 0 means no checksum
}

bitflags::bitflags! {
    /// Frame processing flags
    #[derive(Pod, Zeroable)]
    #[repr(transparent)]
    pub struct FrameFlags: u16 {
        /// Payload is compressed
        const COMPRESSED = 0b0000_0001;
        /// Payload is encrypted  
        const ENCRYPTED  = 0b0000_0010;
        /// Frame is part of chunked sequence
        const CHUNKED    = 0b0000_0100;
        /// Final frame in sequence
        const FINAL      = 0b0000_1000;
        /// Schema validation required
        const SCHEMA     = 0b0001_0000;
        /// Contains semantic hints for SIMD optimization
        const SIMD_HINT  = 0b0010_0000;
        /// Payload contains numeric array data
        const NUMERIC    = 0b0100_0000;
        /// Checksum present
        const CHECKSUM   = 0b1000_0000;
    }
}

impl Frame {
    /// Create a new frame with given payload
    pub fn new(payload: Bytes) -> Self {
        Self {
            header: FrameHeader {
                version: 1,
                flags: FrameFlags::empty(),
                sequence: 0,
                length: payload.len() as u32,
                schema_id: 0,
                checksum: 0,
            },
            payload,
            semantics: None,
        }
    }

    /// Create frame with semantic hints for optimization
    pub fn with_semantics(payload: Bytes, semantics: SemanticMeta) -> Self {
        let mut frame = Self::new(payload);
        frame.semantics = Some(semantics);
        frame.header.flags |= FrameFlags::SIMD_HINT;
        frame
    }

    /// Set sequence number
    pub fn with_sequence(mut self, sequence: u64) -> Self {
        self.header.sequence = sequence;
        self
    }

    /// Set schema ID for validation
    pub fn with_schema(mut self, schema_id: SchemaId) -> Self {
        self.header.schema_id = schema_id;
        self.header.flags |= FrameFlags::SCHEMA;
        self
    }

    /// Enable compression
    pub fn with_compression(mut self) -> Self {
        self.header.flags |= FrameFlags::COMPRESSED;
        self
    }

    /// Calculate and set checksum
    pub fn with_checksum(mut self) -> Self {
        self.header.checksum = crc32c(&self.payload);
        self.header.flags |= FrameFlags::CHECKSUM;
        self
    }

    /// Validate frame integrity
    pub fn validate(&self) -> Result<()> {
        // Check version
        if self.header.version != 1 {
            return Err(Error::invalid_frame(format!(
                "Unsupported version: {}", self.header.version
            )));
        }

        // Check length
        if self.header.length != self.payload.len() as u32 {
            return Err(Error::invalid_frame(format!(
                "Length mismatch: header={}, payload={}", 
                self.header.length, 
                self.payload.len()
            )));
        }

        // Verify checksum if present
        if self.header.flags.contains(FrameFlags::CHECKSUM) {
            let actual = crc32c(&self.payload);
            if actual != self.header.checksum {
                return Err(Error::invalid_frame(format!(
                    "Checksum mismatch: expected={:08x}, actual={:08x}",
                    self.header.checksum,
                    actual
                )));
            }
        }

        Ok(())
    }

    /// Serialize frame to wire format
    pub fn serialize(&self) -> Result<Bytes> {
        let header_size = std::mem::size_of::<FrameHeader>();
        let total_size = header_size + self.payload.len();
        
        let mut buffer = Vec::with_capacity(total_size);
        
        // Write header
        let header_bytes = bytemuck::bytes_of(&self.header);
        buffer.extend_from_slice(header_bytes);
        
        // Write payload
        buffer.extend_from_slice(&self.payload);
        
        Ok(Bytes::from(buffer))
    }

    /// Deserialize frame from wire format  
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let header_size = std::mem::size_of::<FrameHeader>();
        
        if data.len() < header_size {
            return Err(Error::invalid_frame("Insufficient data for header"));
        }

        // Parse header
        let header_bytes = &data[..header_size];
        let header: FrameHeader = *bytemuck::from_bytes(header_bytes);

        // Extract payload
        let payload_start = header_size;
        let payload_end = payload_start + header.length as usize;
        
        if data.len() < payload_end {
            return Err(Error::invalid_frame("Insufficient data for payload"));
        }

        let payload = Bytes::copy_from_slice(&data[payload_start..payload_end]);

        let frame = Frame {
            header,
            payload,
            semantics: None, // Semantics not serialized to wire
        };

        frame.validate()?;
        Ok(frame)
    }

    /// Check if frame contains numeric array data
    pub fn is_numeric(&self) -> bool {
        self.header.flags.contains(FrameFlags::NUMERIC)
    }

    /// Check if frame has semantic hints
    pub fn has_semantics(&self) -> bool {
        self.header.flags.contains(FrameFlags::SIMD_HINT)
    }
}

impl FrameHeader {
    /// Header size in bytes
    pub const SIZE: usize = std::mem::size_of::<Self>();
}

/// Fast CRC32C implementation for checksums
fn crc32c(data: &[u8]) -> u32 {
    // TODO: Use hardware CRC32C if available (SSE4.2)
    crc32c_sw(data)
}

/// Software fallback CRC32C
fn crc32c_sw(data: &[u8]) -> u32 {
    const CRC32C_POLY: u32 = 0x82F63B78;
    let mut crc = !0u32;
    
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ CRC32C_POLY
            } else {
                crc >> 1
            };
        }
    }
    
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_creation() {
        let payload = Bytes::from_static(b"Hello, SJSP!");
        let frame = Frame::new(payload.clone());
        
        assert_eq!(frame.header.version, 1);
        assert_eq!(frame.header.length, payload.len() as u32);
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn test_frame_serialization() {
        let payload = Bytes::from_static(b"test data");
        let frame = Frame::new(payload).with_sequence(42).with_checksum();
        
        let serialized = frame.serialize().unwrap();
        let deserialized = Frame::deserialize(&serialized).unwrap();
        
        assert_eq!(frame.header.sequence, deserialized.header.sequence);
        assert_eq!(frame.payload, deserialized.payload);
    }

    #[test]
    fn test_checksum_validation() {
        let payload = Bytes::from_static(b"checksum test");
        let frame = Frame::new(payload).with_checksum();
        
        frame.validate().unwrap();
        
        // Corrupt payload should fail validation
        let mut bad_frame = frame.clone();
        bad_frame.payload = Bytes::from_static(b"corrupted data");
        
        assert!(bad_frame.validate().is_err());
    }
}