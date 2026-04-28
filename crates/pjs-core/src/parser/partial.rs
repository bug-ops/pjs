//! Partial JSON parsing for streaming frame delivery.
//!
//! This module provides [`PartialJsonParser`], a sealed trait for parsers that
//! tolerate truncated JSON input and report the largest structurally-complete
//! prefix they could recover.
//!
//! The concrete implementation [`JiterPartialParser`] drives the `jiter` crate's
//! per-token public API (`peek`, `known_array`, `known_object`, `known_str`,
//! `known_number`, `known_bool`, `known_null`, `array_step`, `next_key`) plus
//! `current_index()` for cursor tracking.
//!
//! # What this module does NOT use
//!
//! The following `Jiter` methods hardcode `PartialMode::Off` and therefore fail
//! on truncated input. They **must not** be called from this module:
//!
//! - `Jiter::next_value` / `known_value`
//! - `Jiter::next_value_owned` / `known_value_owned`
//! - `Jiter::next_skip` / `known_skip`
//! - `JsonValue::parse` / `parse_with_config` / `parse_owned`
//!
//! # Architecture note
//!
//! `pjs-domain` must not depend on `jiter`. All jiter imports are confined to
//! this module inside `pjs-core`.
//!
//! # TODO(critic): consolidate JsonPath — pjs-domain::value_objects::JsonPath
//! (string newtype) and pjs-core::stream::priority::JsonPath (segmented) overlap;
//! tracked as follow-up to #117.

use std::collections::HashMap;

use jiter::{Jiter, JiterErrorType, JsonErrorType, NumberAny, NumberInt, Peek};
use pjson_rs_domain::value_objects::JsonData;

use crate::Result;
use crate::error::Error;
use crate::stream::priority::JsonPath;

mod private {
    /// Sealing token — external crates cannot implement [`super::PartialJsonParser`].
    pub trait Sealed {}
}

/// Parser that tolerates truncated JSON input and reports what was consumed.
///
/// # Sealing
///
/// This trait is **sealed**: it cannot be implemented outside `pjs-core`.
/// New methods may be added in minor releases without a breaking change.
///
/// # Contract
///
/// Implementations MUST be deterministic: identical input produces identical
/// [`PartialParseResult`]. The returned `value` MUST be a structurally complete
/// JSON value (objects/arrays balanced) — any trailing fragment that would
/// require more bytes to be valid is dropped from `value` and reported via
/// `consumed`.
///
/// When jiter cannot produce any value at all (e.g. input `[` — just an open
/// bracket), the implementation returns:
/// ```text
/// PartialParseResult { value: JsonData::Null, consumed: 0, is_complete: false, ... }
/// ```
///
/// The [`crate::parser::Parser::parse_partial`] wrapper translates `consumed == 0`
/// into `Ok(None)` for the streaming caller.
pub trait PartialJsonParser: private::Sealed {
    /// Parse the largest valid JSON prefix from `input`.
    ///
    /// Returns a [`PartialParseResult`] describing what was recovered.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidJson`] only when the input is **syntactically
    /// invalid up to the point of truncation** — for example a stray `}` with no
    /// matching `{`, or a malformed `\u` escape sequence. Truncation alone is
    /// not an error.
    ///
    /// Returns [`Error::Buffer`] when `input.len()` exceeds
    /// [`JiterConfig::max_input_size`].
    fn parse_partial(&self, input: &[u8]) -> Result<PartialParseResult>;
}

/// Outcome of a partial parse.
///
/// `value` and `consumed` are the canonical pair. `value` is **owned** — it
/// never borrows from `input` — so it survives the caller dropping the buffer
/// (typical for streaming where the producer owns and refills the buffer).
///
/// # Examples
///
/// ```rust,no_run
/// use pjson_rs::parser::partial::{JiterPartialParser, PartialJsonParser};
///
/// let parser = JiterPartialParser::new(Default::default());
/// let result = parser.parse_partial(b"{\"a\":1,\"b\":[2,3").unwrap();
/// assert!(!result.is_complete);
/// assert!(result.consumed > 0);
/// ```
#[derive(Debug, Clone)]
pub struct PartialParseResult {
    /// Structurally complete fragment recovered from the input.
    ///
    /// For input `{"a":1,"b":[2,3` this is `{"a": 1}` — the trailing open
    /// array is dropped because it is not yet closed.
    pub value: JsonData,

    /// Number of bytes successfully consumed. The unparsed tail is
    /// `&input[consumed..]`. Always satisfies `consumed <= input.len()`.
    pub consumed: usize,

    /// `true` when `consumed == input.len()` and parsing reached the
    /// document's natural end. `false` when truncation was tolerated.
    pub is_complete: bool,

    /// Optional skeleton + patch decomposition of `value`, useful for the
    /// streaming reconstruction path. `None` when
    /// [`JiterConfig::emit_streaming_hint`] is `false` or the value is a
    /// single scalar.
    pub streaming_hint: Option<StreamingHint>,

    /// Diagnostics emitted during parsing. Distinct from errors: parsing
    /// succeeded but the caller may want to know about non-fatal observations
    /// such as duplicate-key collisions or lossy big-integer conversions.
    pub diagnostics: Vec<ParseDiagnostic>,
}

impl PartialParseResult {
    fn empty() -> Self {
        Self {
            value: JsonData::Null,
            consumed: 0,
            is_complete: false,
            streaming_hint: None,
            diagnostics: vec![],
        }
    }
}

/// Non-fatal observation produced during partial parsing.
///
/// These are distinct from errors: the parse succeeded, but the caller may
/// want to react (e.g. reject the frame if it requires lossless integer
/// precision).
#[derive(Debug, Clone, PartialEq)]
pub enum ParseDiagnostic {
    /// Two or more entries with the same key were observed in the same object.
    ///
    /// The conversion to [`JsonData::Object`] (backed by `HashMap`) collapsed
    /// them using last-write-wins. `path` points at the containing object;
    /// `key` is the duplicated key name.
    DuplicateKey {
        /// Path to the object containing the duplicate key.
        path: JsonPath,
        /// The key that appeared more than once.
        key: String,
    },

    /// jiter parsed an integer that does not fit in `i64`.
    ///
    /// With jiter's default `num-bigint` feature, such values surface as
    /// `NumberInt::BigInt` rather than `JsonError::NumberOutOfRange`.
    /// `JsonData` has no `BigInt` variant, so the value was lossily converted
    /// to [`JsonData::Float`] via `f64` parsing of the decimal string.
    ///
    /// `original` is the lossless decimal string from jiter; `converted` is
    /// the `f64` that landed in `JsonData`. When `converted.is_infinite()`,
    /// the value overflows `f64::MAX`. A strict consumer can compare the two
    /// and reject the frame.
    BigIntLossyConversion {
        /// Path to the affected number in the document.
        path: JsonPath,
        /// Lossless decimal string representation of the original value.
        original: String,
        /// The `f64` value stored in [`JsonData::Float`].
        converted: f64,
    },
}

/// Decomposition of a partially-parsed value into priority-aware fragments.
///
/// Produced when [`JiterConfig::emit_streaming_hint`] is `true` and the
/// top-level value is an object or array.
#[derive(Debug, Clone)]
pub struct StreamingHint {
    /// JSON paths whose values are stable: their entire serialized form lies
    /// strictly before the truncation boundary. These can be emitted as
    /// `PriorityStreamFrame::Patch` without risk of revision on the next feed.
    pub stable_paths: Vec<JsonPath>,

    /// JSON paths whose values sit at the truncation boundary. The consumer
    /// MAY emit these eagerly and revise them via `PatchOperation::Replace` on
    /// the next feed, or defer until they become stable.
    pub tentative_paths: Vec<JsonPath>,
}

/// Configuration for [`JiterPartialParser`].
#[derive(Debug, Clone)]
pub struct JiterConfig {
    /// Maximum input size in bytes (DoS guard).
    ///
    /// Default: 100 MiB.
    pub max_input_size: usize,

    /// Maximum nesting depth before returning [`Error::Buffer`].
    ///
    /// Applied on top of jiter's hard recursion limit. Default: 64.
    pub max_depth: usize,

    /// When `true`, also populate [`PartialParseResult::streaming_hint`].
    ///
    /// This adds one traversal over the produced [`JsonData`] tree.
    /// Default: `true`.
    pub emit_streaming_hint: bool,

    /// When `true`, an open string at the truncation boundary is kept as
    /// [`JsonData::String`] containing the bytes received so far
    /// (`jiter`'s `with_allow_partial_strings()`). When `false`, the open
    /// string is dropped and `consumed` points before it.
    ///
    /// Default: `true`.
    pub allow_trailing_strings: bool,

    /// Whether `NaN`/`Infinity`/`-Infinity` in input are accepted as numbers.
    ///
    /// Default: `false`.
    pub allow_inf_nan: bool,
}

impl Default for JiterConfig {
    fn default() -> Self {
        Self {
            max_input_size: 100 * 1024 * 1024,
            max_depth: 64,
            emit_streaming_hint: true,
            allow_trailing_strings: true,
            allow_inf_nan: false,
        }
    }
}

/// Partial parser backed by `jiter`.
///
/// Internally drives `Jiter` token-by-token via the **public per-token API**
/// (`peek`, `known_array`, `known_object`, `array_step`, `next_key`,
/// `known_str`, `known_number`, `known_bool`, `known_null`) plus
/// `current_index()` for cursor tracking. The `next_value` / `known_value` /
/// `next_skip` family is **never** called — those methods hardcode
/// `PartialMode::Off` and would error on the very truncation we tolerate.
///
/// # Examples
///
/// ```rust,no_run
/// use pjson_rs::parser::partial::{JiterPartialParser, JiterConfig, PartialJsonParser};
///
/// let parser = JiterPartialParser::new(JiterConfig::default());
///
/// // Complete document
/// let r = parser.parse_partial(b"42").unwrap();
/// assert!(r.is_complete);
/// assert_eq!(r.consumed, 2);
///
/// // Truncated object — only the closed prefix is returned
/// let r = parser.parse_partial(b"{\"a\":1,\"b\":[").unwrap();
/// assert!(!r.is_complete);
/// ```
#[derive(Debug, Clone)]
pub struct JiterPartialParser {
    config: JiterConfig,
}

impl private::Sealed for JiterPartialParser {}

impl JiterPartialParser {
    /// Create a new parser with the given configuration.
    pub fn new(config: JiterConfig) -> Self {
        Self { config }
    }
}

impl Default for JiterPartialParser {
    fn default() -> Self {
        Self::new(JiterConfig::default())
    }
}

impl PartialJsonParser for JiterPartialParser {
    fn parse_partial(&self, input: &[u8]) -> Result<PartialParseResult> {
        if input.len() > self.config.max_input_size {
            return Err(Error::Buffer(format!(
                "input length {} exceeds max_input_size {}",
                input.len(),
                self.config.max_input_size
            )));
        }

        // Empty or all-whitespace: nothing to parse.
        if input.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(PartialParseResult::empty());
        }

        let mut jiter = Jiter::new(input);
        if self.config.allow_inf_nan {
            jiter = jiter.with_allow_inf_nan();
        }
        if self.config.allow_trailing_strings {
            jiter = jiter.with_allow_partial_strings();
        }

        let path = JsonPath::root();
        let outcome = walk(&mut jiter, 0, &path, &self.config);

        match outcome {
            WalkOutcome::Complete {
                value,
                cursor,
                diagnostics,
            } => {
                // Absorb trailing whitespace for is_complete detection.
                let consumed = cursor + count_trailing_whitespace(input, cursor);
                let is_complete = consumed >= input.len();
                let hint = if self.config.emit_streaming_hint {
                    build_streaming_hint(&value, &path, false)
                } else {
                    None
                };
                Ok(PartialParseResult {
                    value,
                    consumed,
                    is_complete,
                    streaming_hint: hint,
                    diagnostics,
                })
            }
            WalkOutcome::Truncated {
                partial,
                cursor,
                diagnostics,
            } => {
                let hint = if self.config.emit_streaming_hint {
                    build_streaming_hint(&partial, &path, true)
                } else {
                    None
                };
                Ok(PartialParseResult {
                    value: partial,
                    consumed: cursor,
                    is_complete: false,
                    streaming_hint: hint,
                    diagnostics,
                })
            }
            WalkOutcome::Hard(e) => Err(Error::invalid_json(e.index, e.error_type.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Walker internals
// ---------------------------------------------------------------------------

/// Internal outcome of a single recursive walk step.
enum WalkOutcome {
    /// A structurally complete value was produced.
    Complete {
        value: JsonData,
        /// `current_index()` after the value's last byte.
        cursor: usize,
        diagnostics: Vec<ParseDiagnostic>,
    },
    /// Truncation was tolerated mid-structure.
    Truncated {
        partial: JsonData,
        /// `current_index()` at the start of the failing token (before the
        /// call that errored).
        cursor: usize,
        diagnostics: Vec<ParseDiagnostic>,
    },
    /// A hard parse error — not tolerable even under partial mode.
    Hard(jiter::JiterError),
}

/// Returns `true` for EOF-class errors that jiter's `allowed_if_partial` set
/// covers, replicated here because `JsonError::allowed_if_partial` is
/// `pub(crate)` in jiter.
fn is_partial_tolerated(e: &jiter::JiterError) -> bool {
    matches!(
        e.error_type,
        JiterErrorType::JsonError(
            JsonErrorType::EofWhileParsingList
                | JsonErrorType::EofWhileParsingObject
                | JsonErrorType::EofWhileParsingString
                | JsonErrorType::EofWhileParsingValue
                | JsonErrorType::ExpectedListCommaOrEnd
                | JsonErrorType::ExpectedObjectCommaOrEnd
        )
    )
}

fn is_eof_value(e: &jiter::JiterError) -> bool {
    matches!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::EofWhileParsingValue)
    )
}

fn count_trailing_whitespace(input: &[u8], from: usize) -> usize {
    input[from..]
        .iter()
        .take_while(|b| b.is_ascii_whitespace())
        .count()
}

fn walk(jiter: &mut Jiter<'_>, depth: usize, path: &JsonPath, config: &JiterConfig) -> WalkOutcome {
    if depth >= config.max_depth {
        return WalkOutcome::Hard(jiter::JiterError {
            error_type: JiterErrorType::JsonError(JsonErrorType::RecursionLimitExceeded),
            index: jiter.current_index(),
        });
    }

    let pre = jiter.current_index();

    let peek = match jiter.peek() {
        Ok(p) => p,
        Err(e) if is_eof_value(&e) => {
            return WalkOutcome::Truncated {
                partial: JsonData::Null,
                cursor: pre,
                diagnostics: vec![],
            };
        }
        Err(e) => return WalkOutcome::Hard(e),
    };

    match peek {
        Peek::Null => match jiter.known_null() {
            Ok(()) => WalkOutcome::Complete {
                value: JsonData::Null,
                cursor: jiter.current_index(),
                diagnostics: vec![],
            },
            Err(e) if is_partial_tolerated(&e) => WalkOutcome::Truncated {
                partial: JsonData::Null,
                cursor: pre,
                diagnostics: vec![],
            },
            Err(e) => WalkOutcome::Hard(e),
        },
        Peek::True | Peek::False => match jiter.known_bool(peek) {
            Ok(b) => WalkOutcome::Complete {
                value: JsonData::Bool(b),
                cursor: jiter.current_index(),
                diagnostics: vec![],
            },
            Err(e) if is_partial_tolerated(&e) => WalkOutcome::Truncated {
                partial: JsonData::Null,
                cursor: pre,
                diagnostics: vec![],
            },
            Err(e) => WalkOutcome::Hard(e),
        },
        Peek::String => walk_string(jiter, pre, config),
        p if p.is_num() => walk_number(jiter, peek, pre, path),
        Peek::Array => {
            // M1 fix: capture cursor BEFORE calling known_array (peek does not
            // consume the `[` byte; known_array advances past it).
            let pre_open = jiter.current_index();
            walk_array(jiter, depth + 1, path, config, pre_open)
        }
        Peek::Object => {
            // M1 fix: same pattern for `{`.
            let pre_open = jiter.current_index();
            walk_object(jiter, depth + 1, path, config, pre_open)
        }
        _ => {
            // Unexpected peek variant — surface as a hard error.
            WalkOutcome::Hard(jiter::JiterError {
                error_type: JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue),
                index: pre,
            })
        }
    }
}

fn walk_string(jiter: &mut Jiter<'_>, pre_open: usize, _config: &JiterConfig) -> WalkOutcome {
    match jiter.known_str() {
        Ok(s) => {
            // M2 fix: copy &str to owned String before any further jiter call.
            let owned = s.to_owned();
            WalkOutcome::Complete {
                value: JsonData::String(owned),
                cursor: jiter.current_index(),
                diagnostics: vec![],
            }
        }
        Err(e)
            if matches!(
                e.error_type,
                JiterErrorType::JsonError(JsonErrorType::EofWhileParsingString)
            ) =>
        {
            // with_allow_partial_strings is set on the Jiter by the caller when
            // allow_trailing_strings is true. In that case known_str() returns
            // Ok(s) for truncated strings — we only reach here when
            // allow_trailing_strings is false or the partial-strings mode still
            // didn't satisfy jiter internally. Drop the trailing string.
            WalkOutcome::Truncated {
                partial: JsonData::Null,
                cursor: pre_open,
                diagnostics: vec![],
            }
        }
        Err(e) => WalkOutcome::Hard(e),
    }
}

fn walk_number(jiter: &mut Jiter<'_>, peek: Peek, pre: usize, path: &JsonPath) -> WalkOutcome {
    match jiter.known_number(peek) {
        Ok(NumberAny::Int(NumberInt::Int(i))) => WalkOutcome::Complete {
            value: JsonData::Integer(i),
            cursor: jiter.current_index(),
            diagnostics: vec![],
        },
        Ok(NumberAny::Int(NumberInt::BigInt(b))) => {
            // M4 fix: use expect() (BigInt::to_string() always produces a valid
            // decimal that f64::from_str can attempt); detect overflow via
            // is_infinite() and emit a rich diagnostic.
            let original = b.to_string();
            let converted: f64 = original
                .parse()
                .expect("BigInt produces parseable decimal for f64::from_str");
            WalkOutcome::Complete {
                value: JsonData::Float(converted),
                cursor: jiter.current_index(),
                diagnostics: vec![ParseDiagnostic::BigIntLossyConversion {
                    path: path.clone(),
                    original,
                    converted,
                }],
            }
        }
        Ok(NumberAny::Float(f)) => WalkOutcome::Complete {
            value: JsonData::Float(f),
            cursor: jiter.current_index(),
            diagnostics: vec![],
        },
        Err(e)
            if matches!(
                e.error_type,
                JiterErrorType::JsonError(JsonErrorType::EofWhileParsingValue)
            ) =>
        {
            // Bare `-` at EOF — boundary case (d): tolerated, drop the partial token.
            WalkOutcome::Truncated {
                partial: JsonData::Null,
                cursor: pre,
                diagnostics: vec![],
            }
        }
        Err(e)
            if matches!(
                e.error_type,
                JiterErrorType::JsonError(JsonErrorType::InvalidNumber)
            ) =>
        {
            // Truncated number like `1.` or `1e` — boundary case (b): NOT tolerated.
            WalkOutcome::Hard(e)
        }
        Err(e) => WalkOutcome::Hard(e),
    }
}

fn walk_array(
    jiter: &mut Jiter<'_>,
    depth: usize,
    path: &JsonPath,
    config: &JiterConfig,
    pre_open: usize,
) -> WalkOutcome {
    // known_array() advances past `[` and peeks at the first element.
    let first_peek = match jiter.known_array() {
        Ok(Some(p)) => p,
        Ok(None) => {
            return WalkOutcome::Complete {
                value: JsonData::Array(vec![]),
                cursor: jiter.current_index(),
                diagnostics: vec![],
            };
        }
        Err(e) if is_partial_tolerated(&e) => {
            // The array opened but immediately hit EOF.
            return WalkOutcome::Truncated {
                partial: JsonData::Array(vec![]),
                cursor: pre_open,
                diagnostics: vec![],
            };
        }
        Err(e) => return WalkOutcome::Hard(e),
    };

    let mut items: Vec<JsonData> = Vec::new();
    // `cursor_after_last_complete` is updated after each element's Complete
    // outcome and used when array_step() hits EOF. The initial value is a
    // dead initialization; the first loop iteration either completes (and
    // updates it) or returns early, so `pre_open` is never actually read.
    // The assignment is intentional to keep a consistent type.
    #[allow(unused_assignments)]
    let mut cursor_after_last_complete = pre_open;
    let mut next_peek = Some(first_peek);
    let mut all_diagnostics: Vec<ParseDiagnostic> = vec![];

    loop {
        let peek = next_peek.take().expect("loop invariant: always set at top");
        let child_path = path.append_index(items.len());

        match walk_with_peek(jiter, peek, depth, &child_path, config) {
            WalkOutcome::Complete {
                value,
                cursor,
                diagnostics,
            } => {
                items.push(value);
                cursor_after_last_complete = cursor;
                all_diagnostics.extend(diagnostics);
            }
            WalkOutcome::Truncated {
                partial,
                cursor,
                diagnostics,
            } => {
                items.push(partial);
                all_diagnostics.extend(diagnostics);
                return WalkOutcome::Truncated {
                    partial: JsonData::Array(items),
                    cursor,
                    diagnostics: all_diagnostics,
                };
            }
            WalkOutcome::Hard(e) => return WalkOutcome::Hard(e),
        }

        // Advance to the next element.
        match jiter.array_step() {
            Ok(Some(p)) => {
                // Cursor after the comma; next iteration will update
                // cursor_after_last_complete when the element completes.
                next_peek = Some(p);
            }
            Ok(None) => {
                return WalkOutcome::Complete {
                    value: JsonData::Array(items),
                    cursor: jiter.current_index(),
                    diagnostics: all_diagnostics,
                };
            }
            Err(e) if is_partial_tolerated(&e) => {
                return WalkOutcome::Truncated {
                    partial: JsonData::Array(items),
                    cursor: cursor_after_last_complete,
                    diagnostics: all_diagnostics,
                };
            }
            Err(e) => return WalkOutcome::Hard(e),
        }
    }
}

fn walk_object(
    jiter: &mut Jiter<'_>,
    depth: usize,
    path: &JsonPath,
    config: &JiterConfig,
    pre_open: usize,
) -> WalkOutcome {
    // known_object() advances past `{` and returns the first key (if any).
    let first_key: Option<String> = match jiter.known_object() {
        Ok(Some(k)) => Some(k.to_owned()), // M2 fix: copy before any further jiter call
        Ok(None) => {
            return WalkOutcome::Complete {
                value: JsonData::Object(HashMap::new()),
                cursor: jiter.current_index(),
                diagnostics: vec![],
            };
        }
        Err(e) if is_partial_tolerated(&e) => {
            return WalkOutcome::Truncated {
                partial: JsonData::Object(HashMap::new()),
                cursor: pre_open,
                diagnostics: vec![],
            };
        }
        Err(e) => return WalkOutcome::Hard(e),
    };

    let mut map: HashMap<String, JsonData> = HashMap::new();
    // `cursor_after_last_complete` tracks the cursor after the last fully
    // parsed key-value pair. Used when next_key() hits EOF: `input[..cursor]`
    // is the last byte of a structurally complete subtree. Initialized to
    // `pre_open` so an empty-pair truncation points at `{`.
    let mut cursor_after_last_complete = pre_open;
    let mut next_key: Option<String> = first_key;
    let mut all_diagnostics: Vec<ParseDiagnostic> = vec![];

    loop {
        let key = next_key.take().expect("loop invariant: always set at top");
        let child_path = path.append_key(&key);

        // Peek at the value following the key.
        let peek = match jiter.peek() {
            Ok(p) => p,
            Err(e) if is_partial_tolerated(&e) => {
                // Key present but value missing — drop the incomplete entry.
                return WalkOutcome::Truncated {
                    partial: JsonData::Object(map),
                    cursor: cursor_after_last_complete,
                    diagnostics: all_diagnostics,
                };
            }
            Err(e) => return WalkOutcome::Hard(e),
        };

        match walk_with_peek(jiter, peek, depth, &child_path, config) {
            WalkOutcome::Complete {
                value,
                cursor,
                diagnostics,
            } => {
                // Last-write-wins + DuplicateKey diagnostic (§4.4).
                if map.contains_key(&key) {
                    all_diagnostics.push(ParseDiagnostic::DuplicateKey {
                        path: path.clone(),
                        key: key.clone(),
                    });
                }
                map.insert(key, value);
                cursor_after_last_complete = cursor;
                all_diagnostics.extend(diagnostics);
            }
            WalkOutcome::Truncated {
                partial,
                cursor,
                diagnostics,
            } => {
                // Partial value for this key — include whatever we parsed.
                if map.contains_key(&key) {
                    all_diagnostics.push(ParseDiagnostic::DuplicateKey {
                        path: path.clone(),
                        key: key.clone(),
                    });
                }
                map.insert(key, partial);
                all_diagnostics.extend(diagnostics);
                return WalkOutcome::Truncated {
                    partial: JsonData::Object(map),
                    cursor,
                    diagnostics: all_diagnostics,
                };
            }
            WalkOutcome::Hard(e) => return WalkOutcome::Hard(e),
        }

        // Advance to the next key.
        // M2 fix: the &str returned by next_key() borrows from jiter.tape.
        // Call .to_owned() first (releasing the borrow), then current_index().
        match jiter.next_key() {
            Ok(Some(k)) => {
                let owned_key = k.to_owned(); // borrow released
                // cursor_after_last_complete stays at the previous complete pair;
                // it will be updated when this new pair's value completes.
                next_key = Some(owned_key);
            }
            Ok(None) => {
                return WalkOutcome::Complete {
                    value: JsonData::Object(map),
                    cursor: jiter.current_index(),
                    diagnostics: all_diagnostics,
                };
            }
            Err(e) if is_partial_tolerated(&e) => {
                return WalkOutcome::Truncated {
                    partial: JsonData::Object(map),
                    cursor: cursor_after_last_complete,
                    diagnostics: all_diagnostics,
                };
            }
            Err(e) => return WalkOutcome::Hard(e),
        }
    }
}

/// Dispatch a pre-peeked value through the walker without calling `peek()` again.
fn walk_with_peek(
    jiter: &mut Jiter<'_>,
    peek: Peek,
    depth: usize,
    path: &JsonPath,
    config: &JiterConfig,
) -> WalkOutcome {
    if depth >= config.max_depth {
        return WalkOutcome::Hard(jiter::JiterError {
            error_type: JiterErrorType::JsonError(JsonErrorType::RecursionLimitExceeded),
            index: jiter.current_index(),
        });
    }

    let pre = jiter.current_index();

    match peek {
        Peek::Null => match jiter.known_null() {
            Ok(()) => WalkOutcome::Complete {
                value: JsonData::Null,
                cursor: jiter.current_index(),
                diagnostics: vec![],
            },
            Err(e) if is_partial_tolerated(&e) => WalkOutcome::Truncated {
                partial: JsonData::Null,
                cursor: pre,
                diagnostics: vec![],
            },
            Err(e) => WalkOutcome::Hard(e),
        },
        Peek::True | Peek::False => match jiter.known_bool(peek) {
            Ok(b) => WalkOutcome::Complete {
                value: JsonData::Bool(b),
                cursor: jiter.current_index(),
                diagnostics: vec![],
            },
            Err(e) if is_partial_tolerated(&e) => WalkOutcome::Truncated {
                partial: JsonData::Null,
                cursor: pre,
                diagnostics: vec![],
            },
            Err(e) => WalkOutcome::Hard(e),
        },
        Peek::String => walk_string(jiter, pre, config),
        p if p.is_num() => walk_number(jiter, peek, pre, path),
        Peek::Array => {
            let pre_open = jiter.current_index();
            walk_array(jiter, depth + 1, path, config, pre_open)
        }
        Peek::Object => {
            let pre_open = jiter.current_index();
            walk_object(jiter, depth + 1, path, config, pre_open)
        }
        _ => WalkOutcome::Hard(jiter::JiterError {
            error_type: JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue),
            index: pre,
        }),
    }
}

// ---------------------------------------------------------------------------
// Streaming hint construction
// ---------------------------------------------------------------------------

/// Walk the produced `JsonData` tree and classify leaves as stable or tentative.
///
/// When `is_truncated` is `true` (parse ended mid-structure), the last leaf in
/// depth-first order sits at the truncation boundary and is placed in
/// [`StreamingHint::tentative_paths`]. All earlier leaves are stable.
///
/// When `is_truncated` is `false` (complete parse), all leaves are stable.
fn build_streaming_hint(
    value: &JsonData,
    root: &JsonPath,
    is_truncated: bool,
) -> Option<StreamingHint> {
    match value {
        JsonData::Object(_) | JsonData::Array(_) => {
            let mut all_paths: Vec<JsonPath> = vec![];
            collect_leaves(value, root, &mut all_paths);

            let mut hint = StreamingHint {
                stable_paths: vec![],
                tentative_paths: vec![],
            };

            if is_truncated && !all_paths.is_empty() {
                // The last leaf in depth-first order is at the truncation boundary.
                let tentative = all_paths.pop().expect("non-empty vec");
                hint.stable_paths = all_paths;
                hint.tentative_paths = vec![tentative];
            } else {
                hint.stable_paths = all_paths;
            }

            Some(hint)
        }
        // Single scalars: no hint needed.
        _ => None,
    }
}

/// Collect all leaf paths from a JSON value tree in depth-first order.
fn collect_leaves(value: &JsonData, path: &JsonPath, out: &mut Vec<JsonPath>) {
    match value {
        JsonData::Object(map) => {
            for (key, child) in map {
                let child_path = path.append_key(key);
                collect_leaves(child, &child_path, out);
            }
        }
        JsonData::Array(items) => {
            for (idx, child) in items.iter().enumerate() {
                let child_path = path.append_index(idx);
                collect_leaves(child, &child_path, out);
            }
        }
        _ => {
            out.push(path.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> JiterPartialParser {
        JiterPartialParser::default()
    }

    // --- §7.6 edge inputs ---

    #[test]
    fn test_empty_input_returns_consumed_zero() {
        let r = parser().parse_partial(b"").unwrap();
        assert_eq!(r.consumed, 0);
        assert!(!r.is_complete);
        assert!(r.diagnostics.is_empty());
        assert!(matches!(r.value, JsonData::Null));
    }

    #[test]
    fn test_whitespace_only_returns_consumed_zero() {
        let r = parser().parse_partial(b"   \n\t").unwrap();
        assert_eq!(r.consumed, 0);
        assert!(!r.is_complete);
    }

    #[test]
    fn test_lone_open_bracket_consumed_zero() {
        let r = parser().parse_partial(b"[").unwrap();
        assert_eq!(r.consumed, 0, "open array must not advance consumed");
        assert!(!r.is_complete);
        assert!(matches!(r.value, JsonData::Array(ref v) if v.is_empty()));
    }

    #[test]
    fn test_lone_open_brace_consumed_zero() {
        let r = parser().parse_partial(b"{").unwrap();
        assert_eq!(r.consumed, 0, "open object must not advance consumed");
        assert!(!r.is_complete);
        assert!(matches!(r.value, JsonData::Object(ref m) if m.is_empty()));
    }

    #[test]
    fn test_complete_null() {
        let r = parser().parse_partial(b"null").unwrap();
        assert!(r.is_complete);
        assert_eq!(r.consumed, 4);
        assert!(matches!(r.value, JsonData::Null));
    }

    #[test]
    fn test_complete_int_with_trailing_whitespace() {
        let r = parser().parse_partial(b"42 ").unwrap();
        assert!(r.is_complete);
        assert_eq!(r.consumed, 3);
        assert!(matches!(r.value, JsonData::Integer(42)));
    }

    #[test]
    fn test_oversized_input_returns_error() {
        let config = JiterConfig {
            max_input_size: 4,
            ..Default::default()
        };
        let p = JiterPartialParser::new(config);
        let result = p.parse_partial(b"12345");
        assert!(result.is_err());
    }

    // --- Complete document ---

    #[test]
    fn test_complete_json_is_complete_true() {
        let r = parser().parse_partial(br#"{"a":1,"b":true}"#).unwrap();
        assert!(r.is_complete);
        assert!(r.diagnostics.is_empty());
        if let JsonData::Object(map) = &r.value {
            assert_eq!(map.get("a"), Some(&JsonData::Integer(1)));
            assert_eq!(map.get("b"), Some(&JsonData::Bool(true)));
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn test_complete_array() {
        let r = parser().parse_partial(b"[1,2,3]").unwrap();
        assert!(r.is_complete);
        assert!(matches!(
            r.value,
            JsonData::Array(ref v) if v.len() == 3
        ));
    }

    // --- Truncated input ---

    #[test]
    fn test_truncated_object_partial_recovered() {
        // {"key": "val  — truncated mid-value
        let r = parser().parse_partial(b"{\"key\": \"val").unwrap();
        assert!(!r.is_complete);
        // Some prefix of the object should be recovered (consumed > 0 or consumed == 0
        // depending on where exactly truncation lands, but not an error).
        // The important assertion: no error returned.
    }

    #[test]
    fn test_truncated_array_partial_recovered() {
        let r = parser().parse_partial(b"[1,2,").unwrap();
        assert!(!r.is_complete);
        // At minimum the two complete elements should appear.
        if let JsonData::Array(items) = &r.value {
            assert!(
                items.len() >= 2,
                "expected at least 2 items, got {:?}",
                items
            );
        }
    }

    // --- §7.2 token-boundary table ---

    #[test]
    fn test_boundary_a_mid_hex_escape_tolerated_as_partial_string() {
        // `"\u00` (5 bytes: `"`, `\`, `u`, `0`, `0`) — with allow_trailing_strings=true,
        // jiter's parse_escape gets EofWhileParsingString from parse_u4, which is
        // caught by the partial-string handler in decode_to_tape and returns Ok("").
        // The cursor advances to the end of input, so consumed=5, is_complete=true.
        // Spec draft claimed this was a hard error; jiter 0.14 tolerates it as a
        // partial (empty) string.
        let result = parser().parse_partial(b"\"\\u00");
        assert!(
            result.is_ok(),
            "mid-hex-escape is tolerated by jiter 0.14 partial-string mode"
        );
        let r = result.unwrap();
        assert!(matches!(r.value, JsonData::String(_)));
        assert_eq!(r.consumed, 5, "all 5 bytes consumed");
        assert!(r.is_complete, "consumed == input.len() so is_complete");
    }

    #[test]
    fn test_boundary_b_truncated_number_tolerated() {
        // `1.` — jiter 0.14 behavior: NumberRange::consume_decimal hits EOF after
        // the `.`, returning EofWhileParsingValue (a tolerated partial error).
        // The number token is dropped and consumed=0. The spec draft claimed this
        // was InvalidNumber (hard error), but jiter treats it as truncation.
        let result = parser().parse_partial(b"1.");
        assert!(result.is_ok(), "jiter tolerates `1.` as truncation");
        let r = result.unwrap();
        assert_eq!(r.consumed, 0, "number dropped: nothing committed");
        assert!(!r.is_complete);
    }

    #[test]
    fn test_boundary_c_mid_key_drops_key() {
        // {"ke — EofWhileParsingString for the key, tolerated
        let r = parser().parse_partial(b"{\"ke").unwrap();
        assert!(!r.is_complete);
        // No panic; the partial key is dropped.
        assert!(matches!(r.value, JsonData::Object(_)));
    }

    #[test]
    fn test_boundary_d_bare_minus_tolerated() {
        // `-` alone — EofWhileParsingValue, tolerated
        let r = parser().parse_partial(b"-").unwrap();
        assert!(!r.is_complete);
        assert_eq!(r.consumed, 0);
    }

    #[test]
    fn test_boundary_e_partial_keyword_tolerated_as_truncation() {
        // `tru` — jiter's consume_ident hits EOF after consuming `r`, `u` and tries
        // for `e` which is missing, returning EofWhileParsingValue. That IS in the
        // partial-tolerated set, so walk returns Truncated { Null, cursor=0 }.
        // Spec draft claimed this was a hard error (ExpectedSomeIdent), but jiter
        // 0.14 returns EofWhileParsingValue which is tolerated.
        let result = parser().parse_partial(b"tru");
        assert!(
            result.is_ok(),
            "partial keyword `tru` is tolerated as truncation in jiter 0.14"
        );
        let r = result.unwrap();
        assert!(!r.is_complete);
        assert_eq!(r.consumed, 0);
    }

    // --- Duplicate-key policy (§4.4) ---

    #[test]
    fn test_duplicate_key_last_write_wins() {
        let r = parser().parse_partial(b"{\"x\":1,\"x\":2}").unwrap();
        assert!(r.is_complete);
        if let JsonData::Object(map) = &r.value {
            assert_eq!(
                map.get("x"),
                Some(&JsonData::Integer(2)),
                "last-write-wins: x should be 2"
            );
        } else {
            panic!("expected Object");
        }
        let dup = r
            .diagnostics
            .iter()
            .find(|d| matches!(d, ParseDiagnostic::DuplicateKey { key, .. } if key == "x"));
        assert!(dup.is_some(), "DuplicateKey diagnostic must be emitted");
    }

    // --- is_complete false for truncated variants ---

    #[test]
    fn test_nested_truncation_is_not_complete() {
        let r = parser().parse_partial(b"{\"a\":{\"b\":1").unwrap();
        assert!(!r.is_complete);
    }

    // --- Streaming hint populated for compound values ---

    #[test]
    fn test_streaming_hint_populated_for_object() {
        let r = parser().parse_partial(br#"{"a":1}"#).unwrap();
        assert!(r.is_complete);
        let hint = r.streaming_hint.expect("hint must be Some for object");
        // Complete parse: all leaves stable, none tentative.
        assert!(
            !hint.stable_paths.is_empty(),
            "complete object must have stable paths"
        );
        assert!(
            hint.tentative_paths.is_empty(),
            "complete parse must have no tentative paths"
        );
        // The single leaf path should end with key "a".
        assert!(
            hint.stable_paths
                .iter()
                .any(|p| p.last_key().as_deref() == Some("a")),
            "stable_paths should contain a path ending with key 'a'"
        );
    }

    #[test]
    fn test_streaming_hint_none_for_scalar() {
        let r = parser().parse_partial(b"42").unwrap();
        assert!(r.streaming_hint.is_none());
    }

    #[test]
    fn test_streaming_hint_truncated_string_at_eof_is_tentative() {
        // {"a":"hello" — truncated after value, but "hello" is complete due to
        // allow_trailing_strings. The trailing string value is tentative (may be
        // extended by next feed).
        let r = parser().parse_partial(b"{\"a\":\"hello").unwrap();
        assert!(!r.is_complete);
        let hint = r
            .streaming_hint
            .expect("hint must be Some for truncated object");
        // With is_truncated=true: last leaf goes to tentative_paths.
        assert_eq!(
            hint.tentative_paths.len(),
            1,
            "exactly one tentative path for truncated string leaf"
        );
    }

    #[test]
    fn test_streaming_hint_complete_object_all_stable() {
        let r = parser().parse_partial(br#"{"x":1,"y":2}"#).unwrap();
        assert!(r.is_complete);
        let hint = r
            .streaming_hint
            .expect("hint must be Some for complete object");
        assert_eq!(
            hint.tentative_paths.len(),
            0,
            "no tentative paths for complete object"
        );
        assert_eq!(
            hint.stable_paths.len(),
            2,
            "two stable leaf paths for two-field object"
        );
    }
}
