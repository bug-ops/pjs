//! Canonical JSON Path value object for addressing nodes in JSON structures.
//!
//! This is the single `JsonPath` type shared across the domain, application,
//! and infrastructure layers (issue #379 consolidated two divergent copies).
//! A path is a segmented sequence (`Vec<PathSegment>`); the root path is the
//! empty sequence. `Display`/`FromStr` render/parse the JSONPath-like textual
//! form (`$.key[0]`) and are also used for serde, so the textual form is the
//! wire format.

use crate::{DomainError, DomainResult};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Type-safe JSON path for addressing nodes in JSON structures.
///
/// Represented as a sequence of [`PathSegment`]s; the root path (`$`) is the
/// empty sequence. Every constructor funnels object keys through the same
/// validation rule (see `validate_key`), so a `JsonPath` can never contain
/// a key that would make `Display`/`FromStr` ambiguous — see the invariant
/// documented on [`JsonPath`]'s `Display` impl (JP-1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsonPath {
    segments: Vec<PathSegment>,
}

/// Single segment of a [`JsonPath`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PathSegment {
    /// Object property key.
    Key(String),
    /// Array index.
    Index(usize),
}

/// A key is valid iff it is non-empty and contains none of `.`, `[`, `]`.
///
/// This single predicate backs `append_key`, `new`/`FromStr`, and
/// `from_segments`, so the three constructors can never disagree about what
/// a valid key looks like. It is intentionally Unicode-aware (any non-empty,
/// delimiter-free `String` is accepted) rather than restricted to ASCII
/// alphanumerics.
fn validate_key(key: &str) -> DomainResult<()> {
    if key.is_empty() {
        return Err(DomainError::InvalidPath("Key cannot be empty".to_string()));
    }
    if key.contains('.') || key.contains('[') || key.contains(']') {
        return Err(DomainError::InvalidPath(format!(
            "Key '{key}' contains invalid characters"
        )));
    }
    Ok(())
}

impl JsonPath {
    /// Create the root path (`$`), i.e. the empty segment sequence.
    pub fn root() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Parse a JSON path from its textual form (e.g. `"$.users[0].name"`).
    ///
    /// # Examples
    /// ```
    /// use pjson_rs_domain::value_objects::JsonPath;
    ///
    /// let path = JsonPath::new("$.users[0].name").unwrap();
    /// assert_eq!(path.depth(), 3);
    /// assert_eq!(path.to_string(), "$.users[0].name");
    ///
    /// assert!(JsonPath::new("$.key[not_a_number]").is_err());
    /// ```
    pub fn new(path: impl Into<String>) -> DomainResult<Self> {
        path.into().parse()
    }

    /// Build a path directly from segments, validating every [`PathSegment::Key`]
    /// with the same rule as [`JsonPath::append_key`].
    ///
    /// # Examples
    /// ```
    /// use pjson_rs_domain::value_objects::{JsonPath, PathSegment};
    ///
    /// let path = JsonPath::from_segments(vec![
    ///     PathSegment::Key("users".to_string()),
    ///     PathSegment::Index(0),
    /// ])
    /// .unwrap();
    /// assert_eq!(path.to_string(), "$.users[0]");
    ///
    /// // A key containing a delimiter is rejected, just like `append_key`.
    /// let invalid = JsonPath::from_segments(vec![PathSegment::Key("a.b".to_string())]);
    /// assert!(invalid.is_err());
    /// ```
    pub fn from_segments(segments: impl IntoIterator<Item = PathSegment>) -> DomainResult<Self> {
        let segments: Vec<PathSegment> = segments.into_iter().collect();
        for segment in &segments {
            if let PathSegment::Key(key) = segment {
                validate_key(key)?;
            }
        }
        Ok(Self { segments })
    }

    /// Append a key segment, producing a new path.
    ///
    /// # Examples
    /// ```
    /// use pjson_rs_domain::value_objects::JsonPath;
    ///
    /// let path = JsonPath::root().append_key("users").unwrap();
    /// assert_eq!(path.to_string(), "$.users");
    ///
    /// // Keys containing '.', '[', ']', or the empty key are rejected.
    /// assert!(JsonPath::root().append_key("").is_err());
    /// assert!(JsonPath::root().append_key("a.b").is_err());
    /// ```
    pub fn append_key(&self, key: &str) -> DomainResult<Self> {
        validate_key(key)?;
        let mut segments = self.segments.clone();
        segments.push(PathSegment::Key(key.to_string()));
        Ok(Self { segments })
    }

    /// Append an array index segment, producing a new path.
    pub fn append_index(&self, index: usize) -> Self {
        let mut segments = self.segments.clone();
        segments.push(PathSegment::Index(index));
        Self { segments }
    }

    /// Borrow the path's segments.
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Number of segments in the path (`0` for root).
    pub fn depth(&self) -> usize {
        self.segments.len()
    }

    /// Get the parent path, or `None` if this is the root.
    ///
    /// O(1) on the segmented representation. This corrects a bug in the
    /// previous string-based implementation, which returned root for any
    /// path ending in an index segment following a key (e.g. `$.users[0]`
    /// incorrectly produced `$` instead of `$.users`).
    pub fn parent(&self) -> Option<Self> {
        if self.segments.is_empty() {
            return None;
        }
        Some(Self {
            segments: self.segments[..self.segments.len() - 1].to_vec(),
        })
    }

    /// Get the last segment of the path, or `None` at root.
    ///
    /// Distinct from [`JsonPath::last_key`]: this returns the literal final
    /// segment, whether it is a key or an index.
    pub fn last_segment(&self) -> Option<&PathSegment> {
        self.segments.last()
    }

    /// Get the last `Key` segment, skipping any trailing `Index` segments.
    ///
    /// Distinct from [`JsonPath::last_segment`]: for `$.arr[5]` this returns
    /// `Some("arr")`, not `None`. Preserves the WASM/HTTP priority-heuristic
    /// parity fixed in #242 — do not conflate the two methods.
    pub fn last_key(&self) -> Option<&str> {
        self.segments
            .iter()
            .rev()
            .find_map(|segment| match segment {
                PathSegment::Key(key) => Some(key.as_str()),
                PathSegment::Index(_) => None,
            })
    }

    /// Check whether `self` is a strict prefix of `other` (self-prefix is `false`).
    pub fn is_prefix_of(&self, other: &JsonPath) -> bool {
        self.segments.len() < other.segments.len() && other.segments.starts_with(&self.segments)
    }

    /// Convert to a JSON Pointer (RFC 6901) string.
    ///
    /// Does not escape `~` or `/` within keys; see follow-up issue for #379.
    ///
    /// # Examples
    /// ```
    /// use pjson_rs_domain::value_objects::JsonPath;
    ///
    /// let path = JsonPath::new("$.users[0].name").unwrap();
    /// assert_eq!(path.to_json_pointer(), "/users/0/name");
    /// assert_eq!(JsonPath::root().to_json_pointer(), "/");
    /// ```
    pub fn to_json_pointer(&self) -> String {
        if self.segments.is_empty() {
            return "/".to_string();
        }
        let mut pointer = String::new();
        for segment in &self.segments {
            pointer.push('/');
            match segment {
                PathSegment::Key(key) => pointer.push_str(key),
                PathSegment::Index(idx) => pointer.push_str(&idx.to_string()),
            }
        }
        pointer
    }
}

/// **INVARIANT (JP-1):** `Display` is injective and total over representable
/// `JsonPath` values. It holds *only because* key validation (`validate_key`)
/// excludes `.`, `[`, `]`, and the empty key: those delimiters cannot occur
/// inside a valid key, so the boundary between a key and the next segment
/// marker is always unambiguous, and every rendered path re-parses via
/// [`FromStr`] to the same segments. Any future change that widens the key
/// alphabet to admit `.`, `[`, `]`, or the empty string **must** add an
/// escaping grammar (e.g. bracket-quote form with backslash-escaping) and a
/// round-trip proptest in the same change, or `Display`/`FromStr` become a
/// path-forgery primitive (see issue #333).
impl fmt::Display for JsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "$")?;
        for segment in &self.segments {
            match segment {
                PathSegment::Key(key) => write!(f, ".{key}")?,
                PathSegment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

/// Parses the textual form produced by [`JsonPath`]'s `Display` impl.
/// See the injectivity/totality invariant documented there (JP-1).
impl FromStr for JsonPath {
    type Err = DomainError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        if path.is_empty() {
            return Err(DomainError::InvalidPath("Path cannot be empty".to_string()));
        }

        if !path.starts_with('$') {
            return Err(DomainError::InvalidPath(
                "Path must start with '$'".to_string(),
            ));
        }

        if path.len() == 1 {
            return Ok(Self::root());
        }

        let mut segments = Vec::new();
        let mut chars = path.chars().skip(1).peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '.' => {
                    let mut key = String::new();
                    while let Some(&next_ch) = chars.peek() {
                        if next_ch == '.' || next_ch == '[' {
                            break;
                        }
                        key.push(next_ch);
                        chars.next();
                    }
                    validate_key(&key)?;
                    segments.push(PathSegment::Key(key));
                }
                '[' => {
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

                    let index = index_str.parse::<usize>().map_err(|_| {
                        DomainError::InvalidPath(format!("Invalid array index '{index_str}'"))
                    })?;
                    segments.push(PathSegment::Index(index));
                }
                _ => {
                    return Err(DomainError::InvalidPath(format!(
                        "Unexpected character '{ch}' in path"
                    )));
                }
            }
        }

        Ok(Self { segments })
    }
}

impl Serialize for JsonPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
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
        // S7: validation is widened to match `append_key`'s rule (non-empty,
        // no '.', '[', ']'), so spaces are now permitted in keys.
        assert!(JsonPath::new("$.key with spaces").is_ok());
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

        assert_eq!(path.to_string(), "$.users[0].name");
        assert_eq!(path.depth(), 3);
    }

    #[test]
    fn test_parent_path() {
        let path = JsonPath::new("$.users[0].name").unwrap();
        let parent = path.parent().unwrap();
        assert_eq!(parent.to_string(), "$.users[0]");

        let root = JsonPath::root();
        assert!(root.parent().is_none());
    }

    /// M2: pins the parent() bug fix — the previous string-based
    /// implementation incorrectly returned root `$` for `$.users[0]`,
    /// discarding the `users` segment.
    #[test]
    fn test_parent_path_after_index_preserves_key() {
        let path = JsonPath::new("$.users[0]").unwrap();
        let parent = path.parent().unwrap();
        assert_eq!(parent.to_string(), "$.users");

        let short = JsonPath::new("$.a").unwrap();
        assert_eq!(short.parent().unwrap(), JsonPath::root());
    }

    #[test]
    fn test_last_segment() {
        let path1 = JsonPath::new("$.users").unwrap();
        assert_eq!(
            path1.last_segment(),
            Some(&PathSegment::Key("users".to_string()))
        );

        let path2 = JsonPath::new("$.array[42]").unwrap();
        assert_eq!(path2.last_segment(), Some(&PathSegment::Index(42)));

        let root = JsonPath::root();
        assert_eq!(root.last_segment(), None);
    }

    /// M5: `last_segment` and `last_key` are not interchangeable —
    /// `last_key` skips trailing index segments.
    #[test]
    fn test_last_key_skips_trailing_index() {
        let path = JsonPath::new("$.arr[5]").unwrap();
        assert_eq!(path.last_segment(), Some(&PathSegment::Index(5)));
        assert_eq!(path.last_key(), Some("arr"));
    }

    #[test]
    fn test_prefix() {
        let parent = JsonPath::new("$.users").unwrap();
        let child = JsonPath::new("$.users.name").unwrap();

        assert!(parent.is_prefix_of(&child));
        assert!(!child.is_prefix_of(&parent));
    }

    /// M3: self-prefix must be `false` — a bare `starts_with` would wrongly
    /// make every path a prefix of itself.
    #[test]
    fn test_is_prefix_of_self_is_false() {
        let path = JsonPath::new("$.users.name").unwrap();
        assert!(!path.is_prefix_of(&path));
    }

    #[test]
    fn test_display_from_str_round_trip() {
        let path = JsonPath::root()
            .append_key("users")
            .unwrap()
            .append_index(0)
            .append_key("name")
            .unwrap();
        let rendered = path.to_string();
        let parsed: JsonPath = rendered.parse().unwrap();
        assert_eq!(path, parsed);
    }

    #[test]
    fn test_serde_round_trip() {
        let path = JsonPath::new("$.users[0].name").unwrap();
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(json, "\"$.users[0].name\"");
        let restored: JsonPath = serde_json::from_str(&json).unwrap();
        assert_eq!(path, restored);
    }

    proptest::proptest! {
        /// JP-1: `append_key` either rejects an arbitrary key outright, or
        /// the resulting path round-trips exactly through `Display` ->
        /// `FromStr`. This is the guard that fails loudly if validation is
        /// ever widened without adding an escaping grammar.
        #[test]
        fn json_path_round_trips_over_arbitrary_keys(key in ".*") {
            if let Ok(path) = JsonPath::root().append_key(&key) {
                let rendered = path.to_string();
                let parsed: JsonPath = rendered.parse().expect("rendered path must re-parse");
                proptest::prop_assert_eq!(path, parsed);
            }
        }
    }
}
