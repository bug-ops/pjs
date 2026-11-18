//! Canonical JSON Path value object for addressing nodes in JSON structures.
//!
//! This is the single `JsonPath` type shared across the domain, application,
//! and infrastructure layers (issue #379 consolidated two divergent copies).
//! A path is a segmented sequence (`Vec<PathSegment>`); the root path is the
//! empty sequence. `Display`/`FromStr` render/parse the JSONPath-like textual
//! form (`$.key[0]`, `$["a.b"]`) and are also used for serde, so the textual
//! form is the wire format.
//!
//! Grammar: `path = "$" *( "." plain / "[" index "]" / "[" quoted "]" )`,
//! where `plain` is a non-empty key without `.`, `[`, `]`, `index` is `0` or
//! `[1-9][0-9]*`, and `quoted` is the canonical `serde_json` string literal of
//! a key that is not plain. Every key is therefore addressable and every
//! accepted text re-renders identically.

use crate::{DomainError, DomainResult};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Type-safe JSON path for addressing nodes in JSON structures.
///
/// Represented as a sequence of [`PathSegment`]s; the root path (`$`) is the
/// empty sequence. Any key, including the empty key and keys containing `.`,
/// `[` or `]`, is addressable; see the module docs for the text grammar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsonPath {
    segments: Vec<PathSegment>,
}

/// Single segment of a [`JsonPath`]: an object key or an array index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    /// Object property key (any string).
    Key(String),
    /// Array index.
    Index(usize),
}

/// A plain key renders as `.key`; any other key renders as a quoted segment.
fn is_plain_key(key: &str) -> bool {
    !key.is_empty() && !key.contains(['.', '[', ']'])
}

fn invalid(message: impl Into<String>) -> DomainError {
    DomainError::InvalidPath(message.into())
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

    /// Build a path directly from segments; every key is representable.
    ///
    /// # Examples
    /// ```
    /// use pjson_rs_domain::value_objects::{JsonPath, PathSegment};
    ///
    /// let path = JsonPath::from_segments(vec![
    ///     PathSegment::Key("a.b".to_string()),
    ///     PathSegment::Index(0),
    /// ]);
    /// assert_eq!(path.to_string(), r#"$["a.b"][0]"#);
    /// ```
    pub fn from_segments(segments: impl IntoIterator<Item = PathSegment>) -> Self {
        Self {
            segments: segments.into_iter().collect(),
        }
    }

    /// Append a key segment, producing a new path. Any key is addressable.
    ///
    /// # Examples
    /// ```
    /// use pjson_rs_domain::value_objects::JsonPath;
    ///
    /// assert_eq!(JsonPath::root().append_key("users").to_string(), "$.users");
    /// assert_eq!(JsonPath::root().append_key("a.b").to_string(), r#"$["a.b"]"#);
    /// assert_eq!(JsonPath::root().append_key("").to_string(), r#"$[""]"#);
    /// ```
    pub fn append_key(&self, key: &str) -> Self {
        let mut segments = self.segments.clone();
        segments.push(PathSegment::Key(key.to_string()));
        Self { segments }
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
    /// The root is the empty string and the empty key is `"/"`. Does not
    /// escape `~` or `/` within keys; see follow-up issue for #379.
    ///
    /// # Examples
    /// ```
    /// use pjson_rs_domain::value_objects::JsonPath;
    ///
    /// let path = JsonPath::new("$.users[0].name").unwrap();
    /// assert_eq!(path.to_json_pointer(), "/users/0/name");
    /// assert_eq!(JsonPath::root().to_json_pointer(), "");
    /// assert_eq!(JsonPath::root().append_key("").to_json_pointer(), "/");
    /// ```
    pub fn to_json_pointer(&self) -> String {
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

/// `Display` and [`FromStr`] are an exact pair: every path renders to text that
/// parses back to the same segments, and every accepted text re-renders
/// byte-for-byte. Non-plain keys render as a canonical quoted segment, which
/// keeps the format unambiguous without restricting keys.
impl fmt::Display for JsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "$")?;
        for segment in &self.segments {
            match segment {
                PathSegment::Key(key) if is_plain_key(key) => write!(f, ".{key}")?,
                PathSegment::Key(key) => {
                    let literal = serde_json::to_string(key).map_err(|_| fmt::Error)?;
                    write!(f, "[{literal}]")?;
                }
                PathSegment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

/// Byte length of the JSON string literal at the start of `text` (which must
/// begin with `"`), up to and including the unescaped closing quote.
fn quoted_literal_len(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (offset, ch) in text.char_indices().skip(1) {
        match (escaped, ch) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => return Some(offset + 1),
            (false, _) => {}
        }
    }
    None
}

fn parse_quoted_key(text: &str) -> DomainResult<(String, usize)> {
    let len = quoted_literal_len(text).ok_or_else(|| invalid("Unterminated quoted key"))?;
    let literal = &text[..len];
    let key: String =
        serde_json::from_str(literal).map_err(|_| invalid("Invalid quoted key literal"))?;
    let canonical = serde_json::to_string(&key).map_err(|_| invalid("Invalid quoted key"))?;
    if canonical != literal {
        return Err(invalid("Quoted key is not in canonical form"));
    }
    if is_plain_key(&key) {
        return Err(invalid("Plain key must not be quoted"));
    }
    Ok((key, len))
}

fn parse_index(text: &str) -> DomainResult<usize> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if !canonical {
        return Err(invalid(format!("Invalid array index '{text}'")));
    }
    text.parse::<usize>()
        .map_err(|_| invalid(format!("Invalid array index '{text}'")))
}

/// Parses the textual form produced by [`JsonPath`]'s `Display` impl; see the
/// module docs for the grammar.
impl FromStr for JsonPath {
    type Err = DomainError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        if path.is_empty() {
            return Err(invalid("Path cannot be empty"));
        }
        let mut rest = path
            .strip_prefix('$')
            .ok_or_else(|| invalid("Path must start with '$'"))?;

        let mut segments = Vec::new();
        while let Some(marker) = rest.chars().next() {
            let body = &rest[marker.len_utf8()..];
            match marker {
                '.' => {
                    let end = body.find(['.', '[']).unwrap_or(body.len());
                    let key = &body[..end];
                    if !is_plain_key(key) {
                        return Err(invalid(format!("Invalid key '{key}'")));
                    }
                    segments.push(PathSegment::Key(key.to_string()));
                    rest = &body[end..];
                }
                '[' if body.starts_with('"') => {
                    let (key, len) = parse_quoted_key(body)?;
                    rest = body[len..]
                        .strip_prefix(']')
                        .ok_or_else(|| invalid("Quoted key must be followed by ']'"))?;
                    segments.push(PathSegment::Key(key));
                }
                '[' => {
                    let end = body
                        .find(']')
                        .ok_or_else(|| invalid("Unterminated array index"))?;
                    segments.push(PathSegment::Index(parse_index(&body[..end])?));
                    rest = &body[end + 1..];
                }
                other => {
                    return Err(invalid(format!("Unexpected character '{other}' in path")));
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
        let path = root.append_key("users").append_index(0).append_key("name");

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
            .append_index(0)
            .append_key("name");
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
}
