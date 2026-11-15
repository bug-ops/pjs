//! Opaque trained-dictionary blob exchanged through the dictionary port.

use crate::{DomainError, DomainResult};
use std::fmt;

/// Largest accepted trained dictionary in bytes (112 KiB).
///
/// Large dictionaries inflate resident memory for every session and slow
/// context initialisation; 112 KiB covers JSON-like payloads.
pub const MAX_TRAINED_DICTIONARY_BYTES: usize = 112 * 1024;

/// A non-empty, size-bounded compression dictionary treated as opaque bytes.
///
/// The domain does not know which codec produced the bytes; adapters convert
/// to and from their codec-specific dictionary types and apply any
/// format-specific checks (for example a magic header) themselves.
///
/// **Invariant:** `1 <= len() <= MAX_TRAINED_DICTIONARY_BYTES`.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::value_objects::TrainedDictionary;
///
/// let dictionary = TrainedDictionary::from_bytes(vec![1, 2, 3])?;
/// assert_eq!(dictionary.as_bytes(), [1, 2, 3]);
///
/// assert!(TrainedDictionary::from_bytes(Vec::new()).is_err());
/// # Ok::<(), pjson_rs_domain::DomainError>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct TrainedDictionary(Vec<u8>);

impl TrainedDictionary {
    /// Wraps `bytes` after checking the size invariant.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidInput`] if `bytes` is empty or longer
    /// than [`MAX_TRAINED_DICTIONARY_BYTES`].
    pub fn from_bytes(bytes: Vec<u8>) -> DomainResult<Self> {
        if bytes.is_empty() {
            return Err(DomainError::InvalidInput(
                "trained dictionary is empty".to_string(),
            ));
        }
        if bytes.len() > MAX_TRAINED_DICTIONARY_BYTES {
            return Err(DomainError::InvalidInput(format!(
                "trained dictionary size {} exceeds {MAX_TRAINED_DICTIONARY_BYTES} bytes",
                bytes.len()
            )));
        }
        Ok(Self(bytes))
    }

    /// The dictionary bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Size in bytes, always within `1..=MAX_TRAINED_DICTIONARY_BYTES`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`; present for the `len`/`is_empty` pairing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for TrainedDictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrainedDictionary")
            .field("len", &self.0.len())
            .finish()
    }
}
