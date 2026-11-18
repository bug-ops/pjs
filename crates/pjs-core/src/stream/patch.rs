//! The wire form of a stream patch.
//!
//! [`WirePatch`] is the single patch representation shared by the priority
//! streamer, the reconstructor and the WebSocket transport; it lives here so
//! it is available without any transport feature.

use crate::domain::{JsonPath, PathSegment};
use crate::error::PatchError;
use crate::json_tree::{self, resolve_mut};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wire form of a [`JsonPath`] segment: a JSON string addresses an object
/// member and a JSON integer an array element, so every key (including `.`,
/// `[`, `]` and the empty string) is expressible and `"0"` stays distinct from
/// index `0`.
struct WireSegment<'a>(&'a PathSegment);

impl Serialize for WireSegment<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            PathSegment::Key(key) => serializer.serialize_str(key),
            PathSegment::Index(index) => serializer.serialize_u64(*index as u64),
        }
    }
}

struct OwnedWireSegment(PathSegment);

impl<'de> Deserialize<'de> for OwnedWireSegment {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SegmentVisitor;

        impl serde::de::Visitor<'_> for SegmentVisitor {
            type Value = OwnedWireSegment;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a string key or a non-negative integer index")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<OwnedWireSegment, E> {
                Ok(OwnedWireSegment(PathSegment::Key(v.to_owned())))
            }

            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<OwnedWireSegment, E> {
                Ok(OwnedWireSegment(PathSegment::Key(v)))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<OwnedWireSegment, E> {
                usize::try_from(v)
                    .map(|index| OwnedWireSegment(PathSegment::Index(index)))
                    .map_err(|_| E::custom("array index does not fit in usize"))
            }
        }

        deserializer.deserialize_any(SegmentVisitor)
    }
}

/// Serialize adapter writing a [`JsonPath`] as the wire array of segments.
struct ArrayPathRef<'a>(&'a JsonPath);

impl Serialize for ArrayPathRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.segments().iter().map(WireSegment))
    }
}

/// Deserialize adapter reading the wire array of segments into a [`JsonPath`].
struct ArrayPath(JsonPath);

impl<'de> Deserialize<'de> for ArrayPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let segments = Vec::<OwnedWireSegment>::deserialize(deserializer)?;
        Ok(Self(JsonPath::from_segments(
            segments.into_iter().map(|segment| segment.0),
        )))
    }
}

/// One patch operation of a `WsMessage::StreamPatch` frame.
///
/// The wire form is the 3-array `[op, path, argument]` with `op` one of
/// `"set"`, `"merge"`, `"append"`, `"append_str"`:
///
/// | op | argument | effect |
/// |----|----------|--------|
/// | `set` | any value | replaces the value at `path` (the document for `[]`); a parent object gets the key inserted, a parent array needs an existing index |
/// | `merge` | object | inserts or replaces each entry of the object at `path` (shallow) |
/// | `append` | array | appends the elements to the array at `path` |
/// | `append_str` | string | appends the text to the string at `path` |
///
/// # Path depth
///
/// Decoding and applying a patch enforce no path depth limit. The WebSocket
/// client refuses paths longer than `MAX_PATCH_PATH_DEPTH` (128 segments)
/// before it applies them; an embedder applying patches from an untrusted
/// peer through [`JsonReconstructor`](crate::stream::JsonReconstructor) must
/// bound the depth itself.
///
/// # Examples
///
/// ```
/// use pjson_rs::stream::patch::WirePatch;
///
/// let patch: WirePatch = serde_json::from_str(r#"["append_str", ["s"], "tail"]"#).unwrap();
/// assert_eq!(serde_json::to_string(&patch).unwrap(), r#"["append_str",["s"],"tail"]"#);
/// assert!(serde_json::from_str::<WirePatch>(r#"["merge", [], [1]]"#).is_err());
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WirePatch {
    /// Replace the value at `path`
    Set {
        /// Target location
        path: JsonPath,
        /// New value
        value: Value,
    },
    /// Insert or replace object entries at `path`
    Merge {
        /// Target location
        path: JsonPath,
        /// Entries to insert
        entries: serde_json::Map<String, Value>,
    },
    /// Append array elements at `path`
    Append {
        /// Target location
        path: JsonPath,
        /// Elements to append
        values: Vec<Value>,
    },
    /// Append text to the string at `path`
    AppendStr {
        /// Target location
        path: JsonPath,
        /// Text to append
        text: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PatchOp {
    Set,
    Merge,
    Append,
    AppendStr,
}

impl Serialize for WirePatch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;

        let mut seq = serializer.serialize_seq(Some(3))?;
        match self {
            Self::Set { path, value } => {
                seq.serialize_element("set")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(value)?;
            }
            Self::Merge { path, entries } => {
                seq.serialize_element("merge")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(entries)?;
            }
            Self::Append { path, values } => {
                seq.serialize_element("append")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(values)?;
            }
            Self::AppendStr { path, text } => {
                seq.serialize_element("append_str")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(text)?;
            }
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for WirePatch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A sequence visitor reads the op first and then decodes the argument
        // directly into its typed form; an internally tagged or untagged enum
        // would buffer every `Value` through serde's `Content`.
        struct PatchVisitor;

        impl<'de> serde::de::Visitor<'de> for PatchVisitor {
            type Value = WirePatch;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a 3-array [op, path, argument]")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<WirePatch, A::Error> {
                use serde::de::Error;

                fn missing<E: Error>(index: usize) -> E {
                    E::invalid_length(index, &"a 3-array [op, path, argument]")
                }

                let op: PatchOp = seq.next_element()?.ok_or_else(|| missing(0))?;
                let ArrayPath(path) = seq.next_element()?.ok_or_else(|| missing(1))?;
                let patch = match op {
                    PatchOp::Set => WirePatch::Set {
                        path,
                        value: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                    PatchOp::Merge => WirePatch::Merge {
                        path,
                        entries: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                    PatchOp::Append => WirePatch::Append {
                        path,
                        values: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                    PatchOp::AppendStr => WirePatch::AppendStr {
                        path,
                        text: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                };
                if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(A::Error::invalid_length(
                        4,
                        &"a 3-array [op, path, argument]",
                    ));
                }
                Ok(patch)
            }
        }

        deserializer.deserialize_seq(PatchVisitor)
    }
}

/// Applies `patch` to `state`, moving its payload into the state, and returns
/// the path it addressed.
///
/// The single implementation of patch semantics, shared by the stream
/// reconstructor and the WebSocket client. It enforces no path depth limit;
/// callers facing untrusted input check the depth first. On error `state` is
/// unchanged.
///
/// # Errors
///
/// A resolution error ([`PatchError::KeyNotFound`], [`PatchError::IndexOutOfBounds`],
/// [`PatchError::ExpectedObject`], [`PatchError::ExpectedArray`]) when the path
/// does not resolve, and [`PatchError::ExpectedObject`] /
/// [`PatchError::ExpectedArray`] / [`PatchError::ExpectedString`] when the
/// target has the wrong kind for the op.
pub(crate) fn apply_patch(state: &mut Value, patch: WirePatch) -> Result<JsonPath, PatchError> {
    match patch {
        WirePatch::Set { path, value } => {
            json_tree::set(state, path.segments(), value)?;
            Ok(path)
        }
        WirePatch::Merge { path, entries } => {
            let Value::Object(target) = resolve_mut(state, path.segments())? else {
                return Err(PatchError::ExpectedObject);
            };
            target.extend(entries);
            Ok(path)
        }
        WirePatch::Append { path, values } => {
            let Value::Array(target) = resolve_mut(state, path.segments())? else {
                return Err(PatchError::ExpectedArray);
            };
            target.extend(values);
            Ok(path)
        }
        WirePatch::AppendStr { path, text } => {
            let Value::String(target) = resolve_mut(state, path.segments())? else {
                return Err(PatchError::ExpectedString);
            };
            target.push_str(&text);
            Ok(path)
        }
    }
}
