//! JSON Path Value Object with validation
//!
//! Pure domain object for JSON path addressing.
//! Serialization is handled in the application layer via DTOs.
//!
//! TODO: Remove serde derives once all serialization uses DTOs

use crate::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Type-safe JSON Path for addressing nodes in JSON structures
///
/// This is a pure domain object. Serialization should be handled
/// in the application layer via DTOs, but serde is temporarily kept
/// for compatibility with existing code.
///
/// TODO: Remove Serialize, Deserialize derives once all serialization uses DTOs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JsonPath(String);

impl JsonPath {
    /// Create new JSON path with validation
    pub fn new(path: impl Into<String>) -> DomainResult<Self> {
        let path = path.into();
        Self::validate(&path)?;
        Ok(Self(path))
    }

    /// Create root path
    pub fn root() -> Self {
        // Safety: Root path is always valid
        Self("$".to_string())
    }

    /// Append a key to the path
    pub fn append_key(&self, key: &str) -> DomainResult<Self> {
        if key.is_empty() {
            return Err(DomainError::InvalidPath("Key cannot be empty".to_string()));
        }

        if key.contains('.') || key.contains('[') || key.contains(']') {
            return Err(DomainError::InvalidPath(format!(
                "Key '{key}' contains invalid characters"
            )));
        }

        let new_path = if self.0 == "$" {
            format!("$.{key}")
        } else {
            format!("{}.{key}", self.0)
        };

        Ok(Self(new_path))
    }

    /// Append an array index to the path
    pub fn append_index(&self, index: usize) -> Self {
        let new_path = format!("{}[{index}]", self.0);
        // Array index paths are always valid if base path is valid
        Self(new_path)
    }

    /// Get the path as a string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the parent path (if any)
    pub fn parent(&self) -> Option<Self> {
        if self.0 == "$" {
            return None;
        }

        // Find last separator
        if let Some(pos) = self.0.rfind('.') {
            if pos > 1 {
                // Ensure we don't return empty path
                return Some(Self(self.0[..pos].to_string()));
            } else {
                return Some(Self::root());
            }
        }

        // Handle array index case: $.key[0] -> $.key
        if let Some(pos) = self.0.rfind('[')
            && pos > 1
        {
            return Some(Self(self.0[..pos].to_string()));
        }

        // If no separator found and not root, parent is root
        Some(Self::root())
    }

    /// Get the last segment of the path
    pub fn last_segment(&self) -> Option<PathSegment> {
        if self.0 == "$" {
            return Some(PathSegment::Root);
        }

        // Check for array index: [123]
        if let Some(start) = self.0.rfind('[')
            && let Some(end) = self.0.rfind(']')
            && end > start
        {
            let index_str = &self.0[start + 1..end];
            if let Ok(index) = index_str.parse::<usize>() {
                return Some(PathSegment::Index(index));
            }
        }

        // Check for key segment
        if let Some(pos) = self.0.rfind('.') {
            let key = &self.0[pos + 1..];
            // Remove array index if present
            let key = if let Some(bracket) = key.find('[') {
                &key[..bracket]
            } else {
                key
            };

            if !key.is_empty() {
                return Some(PathSegment::Key(key.to_string()));
            }
        }

        None
    }

    /// Get the depth of the path (number of segments)
    pub fn depth(&self) -> usize {
        if self.0 == "$" {
            return 0;
        }

        let mut depth = 0;
        let mut chars = self.0.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '.' => depth += 1,
                '[' => {
                    // Skip to end of array index
                    for ch in chars.by_ref() {
                        if ch == ']' {
                            break;
                        }
                    }
                    depth += 1;
                }
                _ => {}
            }
        }

        depth
    }

    /// Check if this path is a prefix of another path
    pub fn is_prefix_of(&self, other: &JsonPath) -> bool {
        if self.0.len() >= other.0.len() {
            return false;
        }

        other.0.starts_with(&self.0)
            && (other.0.chars().nth(self.0.len()) == Some('.')
                || other.0.chars().nth(self.0.len()) == Some('['))
    }

    /// Validate JSON path format
    fn validate(path: &str) -> DomainResult<()> {
        if path.is_empty() {
            return Err(DomainError::InvalidPath("Path cannot be empty".to_string()));
        }

        if !path.starts_with('$') {
            return Err(DomainError::InvalidPath(
                "Path must start with '$'".to_string(),
            ));
        }

        if path.len() == 1 {
            return Ok(()); // Root path is valid
        }

        // Validate format: $[.key|[index]]*
        let mut chars = path.chars().skip(1).peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '.' => {
                    // Must be followed by valid key
                    let mut key = String::new();

                    while let Some(&next_ch) = chars.peek() {
                        if next_ch == '.' || next_ch == '[' {
                            break;
                        }
                        key.push(chars.next().ok_or_else(|| {
                            DomainError::InvalidPath("Incomplete key segment".to_string())
                        })?);
                    }

                    if key.is_empty() {
                        return Err(DomainError::InvalidPath("Empty key segment".to_string()));
                    }

                    // Validate key characters
                    if !key
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                    {
                        return Err(DomainError::InvalidPath(format!(
                            "Invalid characters in key '{key}'"
                        )));
                    }
                }
                '[' => {
                    // Must contain valid array index
                    let mut index_str = String::new();

                    for ch in chars.by_ref() {
                        if ch == ']' {
                            break;
                        }
                        index_str.push(ch);
                    }

                    if index_str.is_empty() {
                        return Err(DomainError::InvalidPath("Empty array index".to_string()));
                    }

                    if index_str.parse::<usize>().is_err() {
                        return Err(DomainError::InvalidPath(format!(
                            "Invalid array index '{index_str}'"
                        )));
                    }
                }
                _ => {
                    return Err(DomainError::InvalidPath(format!(
                        "Unexpected character '{ch}' in path"
                    )));
                }
            }
        }

        Ok(())
    }
}

/// Path segment types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    Root,
    Key(String),
    Index(usize),
}

impl fmt::Display for JsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathSegment::Root => write!(f, "$"),
            PathSegment::Key(key) => write!(f, ".{key}"),
            PathSegment::Index(index) => write!(f, "[{index}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_paths() {
        assert!(JsonPath::new("$").is_ok());
        assert!(JsonPath::new("$.key").is_ok());
        assert!(JsonPath::new("$.key.nested").is_ok());
        assert!(JsonPath::new("$.key[0]").is_ok());
        assert!(JsonPath::new("$.array[123].field").is_ok());
    }

    #[test]
    fn test_invalid_paths() {
        assert!(JsonPath::new("").is_err());
        assert!(JsonPath::new("key").is_err());
        assert!(JsonPath::new("$.").is_err());
        assert!(JsonPath::new("$.key.").is_err());
        assert!(JsonPath::new("$.key[]").is_err());
        assert!(JsonPath::new("$.key[abc]").is_err());
        assert!(JsonPath::new("$.key with spaces").is_err());
    }

    #[test]
    fn test_path_operations() {
        let root = JsonPath::root();
        let path = root
            .append_key("users")
            .unwrap()
            .append_index(0)
            .append_key("name")
            .unwrap();

        assert_eq!(path.as_str(), "$.users[0].name");
        assert_eq!(path.depth(), 3);
    }

    #[test]
    fn test_parent_path() {
        let path = JsonPath::new("$.users[0].name").unwrap();
        let parent = path.parent().unwrap();
        assert_eq!(parent.as_str(), "$.users[0]");

        let root = JsonPath::root();
        assert!(root.parent().is_none());
    }

    #[test]
    fn test_last_segment() {
        let path1 = JsonPath::new("$.users").unwrap();
        assert_eq!(
            path1.last_segment(),
            Some(PathSegment::Key("users".to_string()))
        );

        let path2 = JsonPath::new("$.array[42]").unwrap();
        assert_eq!(path2.last_segment(), Some(PathSegment::Index(42)));

        let root = JsonPath::root();
        assert_eq!(root.last_segment(), Some(PathSegment::Root));
    }

    #[test]
    fn test_prefix() {
        let parent = JsonPath::new("$.users").unwrap();
        let child = JsonPath::new("$.users.name").unwrap();

        assert!(parent.is_prefix_of(&child));
        assert!(!child.is_prefix_of(&parent));
    }
}
