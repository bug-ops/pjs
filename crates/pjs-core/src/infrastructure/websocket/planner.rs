//! Stream planner: turns one JSON document into the ordered, pre-serialized
//! WebSocket messages a session streams.
//!
//! The plan is a list of complete message texts, so a session holds an exact,
//! capped byte count rather than a `Value` tree several times the payload
//! size, and `enqueue` forwards each text as is.
//!
//! * [`Framing::Merge`] splits an object root by top-level key into
//!   `StreamFrame`s that receivers shallow-merge; a single entry always travels
//!   whole.
//! * [`Framing::Patch`] splits any document into [`WirePatch`] operations:
//!   an object too big for a frame becomes `Set(path, {})` followed by
//!   `Merge` batches, an array `Set(path, [])` followed by `Append` batches,
//!   a string `Set(path, "")` followed by `AppendStr` chunks, recursing into
//!   children that do not fit a frame on their own. An object root is
//!   delivered by `Merge([], ..)` batches onto the receiver's initial `{}`.
//!
//! Planning is O(payload): one measuring pass records every container's
//! serialized size, then the walk consumes the sizes in the same pre-order.
//! The pass costs 24 bytes per container of transient memory, bounded by the
//! inbound message cap.

use super::protocol::{
    Framing, MAX_PATCH_PATH_DEPTH, StreamOptions, WirePatch, WirePath, WireSegment, WsMessage,
};
use crate::{
    domain::{Priority, SessionId},
    error::{PlanError, StreamOptionsError},
};
use serde_json::{Map, Value};
use std::{collections::HashMap, io, time::Duration};

/// Cumulative byte budget of one connection's outgoing channel, and therefore
/// the largest message the planner may produce.
pub(super) const MAX_QUEUED_OUTGOING_BYTES: usize = 16 * 1024 * 1024;

/// Smallest accepted `StreamOptions::max_frame_size`.
pub(super) const MIN_FRAME_SIZE: usize = 1024;

/// Bytes a frame envelope may add around its payload or patch list.
pub(super) const ENVELOPE_RESERVE: usize = 256;

/// Most frames one plan may hold.
pub(super) const MAX_PLAN_FRAMES: usize = 4096;

/// Deepest value nesting a `StreamPatch` can carry and still decode.
///
/// serde_json refuses documents nested more than 127 levels. The envelope
/// takes four of them (message, `data`, `patches`, the patch array), so a
/// `Set` argument may nest 123 levels; a `Merge` or `Append` argument takes
/// one more level for its own container.
const MAX_PATCH_VALUE_DEPTH: usize = 123;

/// Deepest container nesting the measuring pass will recurse into; documents
/// nested deeper are refused before they can exhaust the stack.
const MEASURE_DEPTH_CAP: usize = MAX_PATCH_PATH_DEPTH + 2;

/// Validated planning parameters derived from [`StreamOptions`].
#[derive(Debug, Clone)]
pub(super) struct PlanOptions {
    framing: Framing,
    frame_limit: usize,
    priority_mapping: Option<HashMap<String, Priority>>,
    /// Minimum pause between frames requested through `client_fps`.
    pub(super) frame_interval: Option<Duration>,
}

impl TryFrom<&StreamOptions> for PlanOptions {
    type Error = StreamOptionsError;

    fn try_from(options: &StreamOptions) -> Result<Self, Self::Error> {
        if options.max_frame_size < MIN_FRAME_SIZE {
            return Err(StreamOptionsError::MaxFrameSizeTooSmall {
                requested: options.max_frame_size,
                min: MIN_FRAME_SIZE,
            });
        }
        let frame_interval = match options.client_fps {
            Some(0) => return Err(StreamOptionsError::ZeroClientFps),
            Some(fps) => Some(Duration::from_secs(1) / fps),
            None => None,
        };
        Ok(Self {
            framing: options.framing,
            frame_limit: options.max_frame_size.min(MAX_QUEUED_OUTGOING_BYTES) - ENVELOPE_RESERVE,
            priority_mapping: options.priority_mapping.clone(),
            frame_interval,
        })
    }
}

impl PlanOptions {
    fn priority_of(&self, key: &str) -> Priority {
        self.priority_mapping
            .as_ref()
            .and_then(|mapping| mapping.get(key))
            .copied()
            .unwrap_or(Priority::HIGH)
    }
}

/// A session's delivery plan.
#[derive(Debug)]
pub(super) struct Plan {
    /// Complete message texts in delivery order.
    pub(super) messages: Vec<String>,
    /// Sum of the message lengths.
    pub(super) total_bytes: usize,
}

/// Plans the messages that deliver `data` to a client of `session_id`.
///
/// Every message is at most `max_frame_size` bytes unless a single unit
/// cannot be split (then it travels alone). Frames hold units of one priority
/// each; frames are ordered by priority, highest first, and stably within a
/// priority. Documents nested deeper than the wire allows are refused.
///
/// # Errors
///
/// Returns [`PlanError::UnitExceedsBudget`] when one message would exceed
/// [`MAX_QUEUED_OUTGOING_BYTES`], [`PlanError::TooManyFrames`] above
/// [`MAX_PLAN_FRAMES`] frames, [`PlanError::PlanBytesExceeded`] when the plan
/// is larger than twice the document plus the per-frame envelope reserve, and
/// [`PlanError::PathTooDeep`] for nesting beyond [`MAX_PATCH_PATH_DEPTH`].
pub(super) fn plan_stream(
    session_id: SessionId,
    data: Value,
    options: &PlanOptions,
) -> Result<Plan, PlanError> {
    let mut measured = Vec::new();
    let data_size = measure(&data, 1, &mut measured)?.size;
    let mut planner = Planner {
        options,
        measured: &measured,
        frames: Frames::new(
            session_id,
            options.frame_limit,
            data_size
                .saturating_mul(2)
                .saturating_add(MAX_PLAN_FRAMES * ENVELOPE_RESERVE),
        ),
        path: Vec::new(),
        path_bytes: 0,
    };
    match options.framing {
        Framing::Merge => planner.plan_merge(data)?,
        Framing::Patch => planner.plan_patch(data)?,
    }
    planner.frames.finish()
}

/// Whether `value` certainly serializes to more than `limit` bytes.
///
/// Sums a cheap lower bound (strings and keys `len() + 2`, containers 2,
/// `null` and booleans 4, other scalars 1) and stops at the first sum above
/// `limit`, so the work is bounded by `limit` and independent of the
/// document's size. Iterative, so it is stack-safe at any nesting depth.
/// `false` means "not provably over", not "small".
pub(super) fn exceeds_size(value: &Value, limit: usize) -> bool {
    let mut bound = 0_usize;
    let mut pending = vec![value];
    while let Some(node) = pending.pop() {
        // Every child adds at least 1, so later ones can never be reached.
        let room = limit.saturating_sub(bound).saturating_add(1);
        bound = bound.saturating_add(match node {
            Value::String(s) => s.len().saturating_add(2),
            Value::Null | Value::Bool(_) => 4,
            Value::Number(_) => 1,
            Value::Array(items) => {
                pending.extend(items.iter().take(room));
                2
            }
            Value::Object(map) => {
                let mut keys = 2_usize;
                for (key, child) in map.iter().take(room) {
                    keys = keys.saturating_add(key.len().saturating_add(2));
                    pending.push(child);
                }
                keys
            }
        });
        if bound > limit {
            return true;
        }
    }
    false
}

/// Serialized size and shape of one value.
#[derive(Debug, Clone, Copy)]
struct Measure {
    /// Serialized byte length.
    size: usize,
    /// Nesting depth: 0 for a scalar, 1 for a container of scalars.
    depth: usize,
    /// Containers inside this value (self excluded); lets a walk skip it.
    span: usize,
}

/// Records one entry per container in pre-order and returns the value's
/// measure. Scalars are measured on demand and take no entry.
fn measure(value: &Value, level: usize, out: &mut Vec<Measure>) -> Result<Measure, PlanError> {
    match value {
        Value::Array(items) => {
            let slot = reserve(out, level)?;
            let (mut size, mut depth) = (2, 0);
            let before = out.len();
            for (i, item) in items.iter().enumerate() {
                let child = measure(item, level + 1, out)?;
                size += child.size + usize::from(i > 0);
                depth = depth.max(child.depth);
            }
            Ok(settle(out, slot, size, depth + 1, before))
        }
        Value::Object(map) => {
            let slot = reserve(out, level)?;
            let (mut size, mut depth) = (2, 0);
            let before = out.len();
            for (i, (key, item)) in map.iter().enumerate() {
                let child = measure(item, level + 1, out)?;
                size += string_size(key) + 1 + child.size + usize::from(i > 0);
                depth = depth.max(child.depth);
            }
            Ok(settle(out, slot, size, depth + 1, before))
        }
        scalar => Ok(Measure {
            size: scalar_size(scalar),
            depth: 0,
            span: 0,
        }),
    }
}

fn reserve(out: &mut Vec<Measure>, level: usize) -> Result<usize, PlanError> {
    if level > MEASURE_DEPTH_CAP {
        return Err(PlanError::PathTooDeep {
            depth: level,
            max: MAX_PATCH_PATH_DEPTH,
        });
    }
    out.push(Measure {
        size: 0,
        depth: 0,
        span: 0,
    });
    Ok(out.len() - 1)
}

fn settle(out: &mut [Measure], slot: usize, size: usize, depth: usize, before: usize) -> Measure {
    let measure = Measure {
        size,
        depth,
        span: out.len() - before,
    };
    out[slot] = measure;
    measure
}

/// Serialized length of a string value, quotes included.
fn string_size(s: &str) -> usize {
    2 + s
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        })
        .sum::<usize>()
}

fn scalar_size(value: &Value) -> usize {
    match value {
        Value::String(s) => string_size(s),
        Value::Null => 4,
        Value::Bool(true) => 4,
        Value::Bool(false) => 5,
        other => {
            let mut counter = Counter(0);
            // Writing into a counter cannot fail; a failure counts as empty.
            let _ = serde_json::to_writer(&mut counter, other);
            counter.0
        }
    }
}

struct Counter(usize);

impl io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Content of one frame while it is being filled.
enum Content {
    Patches(Vec<WirePatch>),
    Payload(Map<String, Value>),
    Whole(Value),
}

/// A finished frame waiting to learn whether it is the last one.
struct Held {
    priority: Priority,
    content: Content,
}

/// Accumulates frames, enforcing the plan limits as they are produced.
struct Frames {
    session_id: SessionId,
    limit: usize,
    plan_bytes_cap: usize,
    messages: Vec<String>,
    total_bytes: usize,
    held: Option<Held>,
    priority: Option<Priority>,
    patches: Vec<WirePatch>,
    patches_bytes: usize,
    payload: Map<String, Value>,
    payload_bytes: usize,
}

impl Frames {
    fn new(session_id: SessionId, limit: usize, plan_bytes_cap: usize) -> Self {
        Self {
            session_id,
            limit,
            plan_bytes_cap,
            messages: Vec::new(),
            total_bytes: 0,
            held: None,
            priority: None,
            patches: Vec::new(),
            patches_bytes: 0,
            payload: Map::new(),
            payload_bytes: 0,
        }
    }

    /// Starts a unit of `priority`, closing the open frame if it holds another.
    fn begin(&mut self, priority: Priority) -> Result<(), PlanError> {
        if self.priority != Some(priority) {
            self.close()?;
            self.priority = Some(priority);
        }
        Ok(())
    }

    fn push_patch(&mut self, patch: WirePatch, size: usize) -> Result<(), PlanError> {
        let mut add = size + usize::from(!self.patches.is_empty());
        if !self.patches.is_empty() && self.patches_bytes + add > self.limit {
            self.close()?;
            add = size;
        }
        self.patches.push(patch);
        self.patches_bytes += add;
        Ok(())
    }

    /// `entry_size` is the serialized `"key":value` size.
    fn push_entry(
        &mut self,
        key: String,
        value: Value,
        entry_size: usize,
    ) -> Result<(), PlanError> {
        let mut add = entry_size + usize::from(!self.payload.is_empty());
        if !self.payload.is_empty() && 2 + self.payload_bytes + add > self.limit {
            self.close()?;
            add = entry_size;
        }
        self.payload.insert(key, value);
        self.payload_bytes += add;
        Ok(())
    }

    /// Closes the open frame, if any.
    fn close(&mut self) -> Result<(), PlanError> {
        let priority = self.priority.unwrap_or(Priority::HIGH);
        let content = if !self.patches.is_empty() {
            self.patches_bytes = 0;
            Content::Patches(std::mem::take(&mut self.patches))
        } else if !self.payload.is_empty() {
            self.payload_bytes = 0;
            Content::Payload(std::mem::take(&mut self.payload))
        } else {
            return Ok(());
        };
        self.hold(Held { priority, content })
    }

    fn hold(&mut self, frame: Held) -> Result<(), PlanError> {
        if let Some(previous) = self.held.replace(frame) {
            self.render(previous, false)?;
        }
        Ok(())
    }

    /// Adds a frame carrying `value` as its whole payload.
    fn push_whole(&mut self, priority: Priority, value: Value) -> Result<(), PlanError> {
        self.close()?;
        self.hold(Held {
            priority,
            content: Content::Whole(value),
        })
    }

    fn render(&mut self, frame: Held, is_complete: bool) -> Result<(), PlanError> {
        let frame_id = u32::try_from(self.messages.len()).unwrap_or(u32::MAX);
        let message = match frame.content {
            Content::Patches(patches) => WsMessage::StreamPatch {
                session_id: self.session_id,
                frame_id,
                priority: frame.priority,
                patches,
                is_complete,
            },
            Content::Payload(map) => stream_frame(
                self.session_id,
                frame_id,
                frame.priority,
                Value::Object(map),
                is_complete,
            ),
            Content::Whole(value) => stream_frame(
                self.session_id,
                frame_id,
                frame.priority,
                value,
                is_complete,
            ),
        };
        let mut text = serialized(serde_json::to_string(&message))?;
        // serde_json grows its buffer by doubling; budgets count `len`, so
        // retain only what is counted.
        text.shrink_to_fit();
        if text.len() > MAX_QUEUED_OUTGOING_BYTES {
            return Err(PlanError::UnitExceedsBudget {
                size: text.len(),
                budget: MAX_QUEUED_OUTGOING_BYTES,
            });
        }
        if self.messages.len() + 1 > MAX_PLAN_FRAMES {
            return Err(PlanError::TooManyFrames {
                frames: self.messages.len() + 2,
                max: MAX_PLAN_FRAMES,
            });
        }
        self.total_bytes += text.len();
        if self.total_bytes > self.plan_bytes_cap {
            return Err(PlanError::PlanBytesExceeded {
                bytes: self.total_bytes,
                max: self.plan_bytes_cap,
            });
        }
        self.messages.push(text);
        Ok(())
    }

    fn finish(mut self) -> Result<Plan, PlanError> {
        self.close()?;
        if let Some(last) = self.held.take() {
            self.render(last, true)?;
        }
        Ok(Plan {
            messages: self.messages,
            total_bytes: self.total_bytes,
        })
    }
}

/// Maps a serialization failure to [`PlanError::Unserializable`] instead of
/// planning an empty message. Messages are built from `Value`s, so this is not
/// expected to happen.
fn serialized(result: serde_json::Result<String>) -> Result<String, PlanError> {
    result.map_err(|_| PlanError::Unserializable)
}

fn stream_frame(
    session_id: SessionId,
    frame_id: u32,
    priority: Priority,
    payload: Value,
    is_complete: bool,
) -> WsMessage {
    WsMessage::StreamFrame {
        session_id,
        frame_id,
        priority,
        payload,
        is_complete,
    }
}

struct Planner<'a> {
    options: &'a PlanOptions,
    measured: &'a [Measure],
    frames: Frames,
    path: Vec<WireSegment>,
    /// Sum of the serialized sizes of the path's segments.
    path_bytes: usize,
}

/// One entry of an object being split, with the pre-order cursor of its value.
struct Entry {
    priority: Priority,
    key: String,
    value: Value,
    cursor: usize,
}

impl Planner<'_> {
    fn limit(&self) -> usize {
        self.frames.limit
    }

    /// Size and shape of `value`, whose container entry (if it is one) sits at
    /// `cursor`, and the cursor just past it.
    fn info(&self, value: &Value, cursor: usize) -> (Measure, usize) {
        match value {
            Value::Array(_) | Value::Object(_) => {
                let m = self.measured[cursor];
                (m, cursor + 1 + m.span)
            }
            scalar => (
                Measure {
                    size: scalar_size(scalar),
                    depth: 0,
                    span: 0,
                },
                cursor,
            ),
        }
    }

    fn path_size(&self) -> usize {
        2 + self.path_bytes + self.path.len().saturating_sub(1)
    }

    /// Serialized size of `[op, path, ` plus the closing `]`, around an argument.
    fn patch_overhead(&self, op_size: usize) -> usize {
        4 + op_size + self.path_size()
    }

    fn descend(&mut self, segment: WireSegment) {
        self.path_bytes += segment_size(&segment);
        self.path.push(segment);
    }

    fn ascend(&mut self) {
        if let Some(segment) = self.path.pop() {
            self.path_bytes -= segment_size(&segment);
        }
    }

    fn wire_path(&self) -> WirePath {
        WirePath(self.path.clone())
    }

    // ---- merge framing -------------------------------------------------

    fn plan_merge(&mut self, data: Value) -> Result<(), PlanError> {
        let Value::Object(map) = data else {
            return self.frames.push_whole(Priority::HIGH, data);
        };
        if map.is_empty() {
            return self.frames.push_whole(Priority::HIGH, Value::Object(map));
        }
        for entry in self.sorted_entries(map, 1) {
            self.frames.begin(entry.priority)?;
            let (m, _) = self.info(&entry.value, entry.cursor);
            let entry_size = string_size(&entry.key) + 1 + m.size;
            self.frames.push_entry(entry.key, entry.value, entry_size)?;
        }
        Ok(())
    }

    // ---- patch framing -------------------------------------------------

    fn plan_patch(&mut self, data: Value) -> Result<(), PlanError> {
        match data {
            Value::Object(map) if !map.is_empty() => {
                let entries = self.sorted_entries(map, 1);
                self.emit_entries(entries)
            }
            other => {
                self.frames.begin(Priority::HIGH)?;
                self.emit(other, 0).map(|_| ())
            }
        }
    }

    /// Entries of `map` (whose children's cursors start at `first`) ordered by
    /// priority, highest first, stably within a priority.
    fn sorted_entries(&self, map: Map<String, Value>, first: usize) -> Vec<Entry> {
        let mut cursor = first;
        let mut entries: Vec<Entry> = map
            .into_iter()
            .map(|(key, value)| {
                let here = cursor;
                cursor = self.info(&value, here).1;
                Entry {
                    priority: self.options.priority_of(&key),
                    key,
                    value,
                    cursor: here,
                }
            })
            .collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.priority));
        entries
    }

    /// Delivers the entries of the object at the current path: small entries
    /// in `Merge` batches, entries too big for one frame recursively.
    fn emit_entries(&mut self, entries: Vec<Entry>) -> Result<(), PlanError> {
        let overhead = self.patch_overhead(7) + 2;
        let mut batch = Map::new();
        let mut batch_bytes = 0;
        let mut batch_priority = None;
        for entry in entries {
            if batch_priority != Some(entry.priority) {
                self.flush_merge(&mut batch, &mut batch_bytes)?;
                self.frames.begin(entry.priority)?;
                batch_priority = Some(entry.priority);
            }
            let (m, _) = self.info(&entry.value, entry.cursor);
            let entry_size = string_size(&entry.key) + 1 + m.size;
            if overhead + entry_size <= self.limit() && m.depth < MAX_PATCH_VALUE_DEPTH {
                let add = entry_size + usize::from(!batch.is_empty());
                if overhead + batch_bytes + add > self.limit() {
                    self.flush_merge(&mut batch, &mut batch_bytes)?;
                }
                batch_bytes += entry_size + usize::from(!batch.is_empty());
                batch.insert(entry.key, entry.value);
            } else {
                self.flush_merge(&mut batch, &mut batch_bytes)?;
                self.descend(WireSegment::Key(entry.key));
                self.emit(entry.value, entry.cursor)?;
                self.ascend();
            }
        }
        self.flush_merge(&mut batch, &mut batch_bytes)
    }

    fn flush_merge(
        &mut self,
        batch: &mut Map<String, Value>,
        bytes: &mut usize,
    ) -> Result<(), PlanError> {
        if batch.is_empty() {
            return Ok(());
        }
        let size = self.patch_overhead(7) + 2 + *bytes;
        *bytes = 0;
        let patch = WirePatch::Merge {
            path: self.wire_path(),
            entries: std::mem::take(batch),
        };
        self.frames.push_patch(patch, size)
    }

    /// Delivers `value` at the current path as a `Set`, splitting it when it
    /// does not fit a frame (or the envelope's nesting allowance) whole.
    /// Returns the cursor just past the value.
    fn emit(&mut self, value: Value, cursor: usize) -> Result<usize, PlanError> {
        if self.path.len() > MAX_PATCH_PATH_DEPTH {
            return Err(PlanError::PathTooDeep {
                depth: self.path.len(),
                max: MAX_PATCH_PATH_DEPTH,
            });
        }
        let (m, next) = self.info(&value, cursor);
        let set_size = self.patch_overhead(5) + m.size;
        if set_size <= self.limit() && m.depth <= MAX_PATCH_VALUE_DEPTH {
            let patch = WirePatch::Set {
                path: self.wire_path(),
                value,
            };
            self.frames.push_patch(patch, set_size)?;
            return Ok(next);
        }
        match value {
            Value::Object(map) => {
                self.set_empty(Value::Object(Map::new()))?;
                let priority = self.frames.priority.unwrap_or(Priority::HIGH);
                let entries = map
                    .into_iter()
                    .scan(cursor + 1, |c, (key, value)| {
                        let here = *c;
                        *c = self.info(&value, here).1;
                        Some(Entry {
                            priority,
                            key,
                            value,
                            cursor: here,
                        })
                    })
                    .collect();
                self.emit_entries(entries)?;
            }
            Value::Array(items) => {
                self.set_empty(Value::Array(Vec::new()))?;
                self.emit_items(items, cursor + 1)?;
            }
            Value::String(text) => {
                self.set_empty(Value::String(String::new()))?;
                self.emit_text(&text)?;
            }
            scalar => {
                let patch = WirePatch::Set {
                    path: self.wire_path(),
                    value: scalar,
                };
                self.frames.push_patch(patch, set_size)?;
            }
        }
        Ok(next)
    }

    fn set_empty(&mut self, empty: Value) -> Result<(), PlanError> {
        let size = self.patch_overhead(5) + scalar_or_empty_size(&empty);
        let patch = WirePatch::Set {
            path: self.wire_path(),
            value: empty,
        };
        self.frames.push_patch(patch, size)
    }

    /// Appends the elements of the array at the current path in `Append`
    /// batches; an element too big for a frame is appended as `null` and then
    /// replaced by recursing into it.
    fn emit_items(&mut self, items: Vec<Value>, first: usize) -> Result<(), PlanError> {
        let overhead = self.patch_overhead(8) + 2;
        let mut batch = Vec::new();
        let mut batch_bytes = 0;
        let mut present = 0usize;
        let mut cursor = first;
        for item in items {
            let (m, next) = self.info(&item, cursor);
            if overhead + m.size <= self.limit() && m.depth < MAX_PATCH_VALUE_DEPTH {
                let add = m.size + usize::from(!batch.is_empty());
                if overhead + batch_bytes + add > self.limit() {
                    present += self.flush_append(&mut batch, &mut batch_bytes)?;
                }
                batch_bytes += m.size + usize::from(!batch.is_empty());
                batch.push(item);
            } else {
                present += self.flush_append(&mut batch, &mut batch_bytes)?;
                let patch = WirePatch::Append {
                    path: self.wire_path(),
                    values: vec![Value::Null],
                };
                self.frames.push_patch(patch, overhead + 4)?;
                self.descend(WireSegment::Index(present));
                present += 1;
                self.emit(item, cursor)?;
                self.ascend();
            }
            cursor = next;
        }
        self.flush_append(&mut batch, &mut batch_bytes)?;
        Ok(())
    }

    /// Returns how many elements the flushed batch added.
    fn flush_append(
        &mut self,
        batch: &mut Vec<Value>,
        bytes: &mut usize,
    ) -> Result<usize, PlanError> {
        if batch.is_empty() {
            return Ok(0);
        }
        let size = self.patch_overhead(8) + 2 + *bytes;
        *bytes = 0;
        let values = std::mem::take(batch);
        let added = values.len();
        let patch = WirePatch::Append {
            path: self.wire_path(),
            values,
        };
        self.frames.push_patch(patch, size)?;
        Ok(added)
    }

    /// Appends `text` to the (empty) string at the current path in chunks cut
    /// at character boundaries.
    fn emit_text(&mut self, text: &str) -> Result<(), PlanError> {
        let overhead = self.patch_overhead(12);
        let body_budget = self.limit().saturating_sub(overhead + 2);
        let mut start = 0;
        let mut chunk_bytes = 0;
        for (index, c) in text.char_indices() {
            let cost = string_size(c.encode_utf8(&mut [0; 4])) - 2;
            if chunk_bytes > 0 && chunk_bytes + cost > body_budget {
                self.push_chunk(&text[start..index], overhead + 2 + chunk_bytes)?;
                start = index;
                chunk_bytes = 0;
            }
            chunk_bytes += cost;
        }
        if start < text.len() {
            self.push_chunk(&text[start..], overhead + 2 + chunk_bytes)?;
        }
        Ok(())
    }

    fn push_chunk(&mut self, chunk: &str, size: usize) -> Result<(), PlanError> {
        let patch = WirePatch::AppendStr {
            path: self.wire_path(),
            text: chunk.to_owned(),
        };
        self.frames.push_patch(patch, size)
    }
}

fn segment_size(segment: &WireSegment) -> usize {
    match segment {
        WireSegment::Key(key) => string_size(key),
        WireSegment::Index(index) => index
            .checked_ilog10()
            .map_or(1, |digits| digits as usize + 1),
    }
}

fn scalar_or_empty_size(value: &Value) -> usize {
    match value {
        Value::Object(_) | Value::Array(_) => 2,
        scalar => scalar_size(scalar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::websocket::protocol::apply_patch;
    use core::assert_matches;
    use serde_json::json;

    fn options(framing: Framing, max_frame_size: usize) -> PlanOptions {
        PlanOptions::try_from(&StreamOptions {
            framing,
            max_frame_size,
            ..StreamOptions::default()
        })
        .unwrap()
    }

    fn decode(plan: &Plan) -> Vec<WsMessage> {
        plan.messages
            .iter()
            .map(|text| serde_json::from_str(text).expect("planner output decodes"))
            .collect()
    }

    /// Applies a patch plan to `{}` and checks frame bookkeeping.
    fn replay(plan: &Plan) -> Value {
        let mut state = json!({});
        let messages = decode(plan);
        for (i, message) in messages.iter().enumerate() {
            let WsMessage::StreamPatch {
                frame_id,
                patches,
                is_complete,
                ..
            } = message
            else {
                panic!("expected StreamPatch");
            };
            assert_eq!(*frame_id as usize, i);
            assert_eq!(*is_complete, i + 1 == messages.len());
            for patch in patches.iter().cloned() {
                apply_patch(&mut state, patch).expect("planned patches apply");
            }
        }
        state
    }

    fn patch_round_trip(data: &Value, max_frame_size: usize) -> Plan {
        let plan = plan_stream(
            SessionId::new(),
            data.clone(),
            &options(Framing::Patch, max_frame_size),
        )
        .expect("plans");
        assert_eq!(&replay(&plan), data);
        for text in &plan.messages {
            assert!(
                text.len() <= max_frame_size,
                "{} > {max_frame_size}",
                text.len()
            );
        }
        plan
    }

    /// Deterministic xorshift so the generated documents are reproducible.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn text(&mut self, max_len: u64) -> String {
            const ALPHABET: &[char] = &[
                'a',
                'b',
                '.',
                '[',
                ']',
                '"',
                '\\',
                '\n',
                '\u{e9}',
                '\u{1f600}',
                '0',
                ' ',
                '\u{1}',
            ];
            let len = self.below(max_len);
            (0..len)
                .map(|_| ALPHABET[self.below(ALPHABET.len() as u64) as usize])
                .collect()
        }

        fn value(&mut self, depth: u32) -> Value {
            match self.below(if depth == 0 { 4 } else { 7 }) {
                0 => Value::Null,
                1 => json!(self.below(1_000_000) as i64 - 500_000),
                2 => json!(self.below(2) == 0),
                3 => Value::String(self.text(2500)),
                4 | 5 => {
                    let len = self.below(10);
                    Value::Array((0..len).map(|_| self.value(depth - 1)).collect())
                }
                _ => {
                    let len = self.below(8);
                    Value::Object(
                        (0..len)
                            .map(|_| (self.text(8), self.value(depth - 1)))
                            .collect(),
                    )
                }
            }
        }
    }

    #[test]
    fn test_string_size_matches_serde_json() {
        for s in [
            "",
            "plain",
            "q\"b\\n\n\t\r\u{8}\u{c}\u{1}\u{1f}",
            "\u{e9}\u{1f600}\u{7f}",
        ] {
            assert_eq!(
                string_size(s),
                serde_json::to_string(s).unwrap().len(),
                "{s:?}"
            );
        }
    }

    #[test]
    fn test_generated_documents_round_trip_in_patch_framing() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..200 {
            let depth = 1 + rng.below(5) as u32;
            let data = rng.value(depth);
            let frame = 1024 + rng.below(3000) as usize;
            patch_round_trip(&data, frame);
        }
    }

    #[test]
    fn test_generated_objects_round_trip_in_merge_framing() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for _ in 0..100 {
            let len = 1 + rng.below(60);
            let entries: Map<String, Value> = (0..len)
                .map(|i| (format!("{}{i}", rng.text(4)), rng.value(2)))
                .collect();
            let data = Value::Object(entries);
            let plan = plan_stream(
                SessionId::new(),
                data.clone(),
                &options(Framing::Merge, 2048),
            )
            .unwrap();
            let mut merged = Map::new();
            for message in decode(&plan) {
                let WsMessage::StreamFrame {
                    payload: Value::Object(entries),
                    ..
                } = message
                else {
                    panic!("object payload expected");
                };
                merged.extend(entries);
            }
            assert_eq!(Value::Object(merged), data);
        }
    }

    #[test]
    fn test_priorities_never_increase_and_frames_hold_one_priority() {
        let data = json!({"lo": "a".repeat(900), "hi": "b".repeat(900), "mid": "c".repeat(900)});
        let mut options = options(Framing::Patch, 2048);
        options.priority_mapping = Some(HashMap::from([
            ("hi".to_owned(), Priority::new(90).unwrap()),
            ("lo".to_owned(), Priority::new(10).unwrap()),
        ]));
        let plan = plan_stream(SessionId::new(), data, &options).unwrap();
        let priorities: Vec<_> = decode(&plan)
            .into_iter()
            .map(|m| match m {
                WsMessage::StreamPatch { priority, .. } => priority,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(
            priorities.windows(2).all(|w| w[0] >= w[1]),
            "{priorities:?}"
        );
        assert_eq!(priorities.first(), Some(&Priority::new(90).unwrap()));
    }

    #[test]
    fn test_envelope_fits_reserve_for_both_message_kinds() {
        let session_id = SessionId::new();
        let top = Priority::new(255).unwrap();
        let patch = WsMessage::StreamPatch {
            session_id,
            frame_id: u32::MAX,
            priority: top,
            patches: vec![],
            is_complete: false,
        };
        let frame = stream_frame(session_id, u32::MAX, top, json!(null), false);
        for message in [patch, frame] {
            let empty = serde_json::to_string(&message).unwrap().len();
            assert!(empty <= ENVELOPE_RESERVE, "{empty} > {ENVELOPE_RESERVE}");
        }
    }

    fn nested(depth: usize) -> Value {
        (1..depth).fold(json!([]), |inner, _| json!([inner]))
    }

    #[test]
    fn test_patch_value_depth_constant_is_the_decode_boundary() {
        let decodes = |depth: usize| {
            let patch = WsMessage::StreamPatch {
                session_id: SessionId::new(),
                frame_id: 0,
                priority: Priority::HIGH,
                patches: vec![WirePatch::Set {
                    path: WirePath(vec![]),
                    value: nested(depth),
                }],
                is_complete: true,
            };
            serde_json::from_str::<WsMessage>(&serde_json::to_string(&patch).unwrap()).is_ok()
        };
        assert!(decodes(MAX_PATCH_VALUE_DEPTH));
        assert!(!decodes(MAX_PATCH_VALUE_DEPTH + 1));
    }

    #[test]
    fn test_value_nested_beyond_the_decode_boundary_is_decomposed() {
        let data = json!({"k": nested(MAX_PATCH_VALUE_DEPTH + 1)});

        let plan = patch_round_trip(&data, 64 * 1024);

        assert!(decode(&plan).len() == plan.messages.len());
    }

    #[test]
    fn test_document_nested_beyond_the_measure_cap_is_rejected() {
        let data = nested(MAX_PATCH_PATH_DEPTH + 3);
        let result = plan_stream(SessionId::new(), data, &options(Framing::Patch, 64 * 1024));
        assert_matches!(result, Err(PlanError::PathTooDeep { .. }), "{result:?}");
    }

    #[test]
    fn test_path_deeper_than_the_cap_is_rejected_when_a_split_needs_it() {
        let mut data = json!("x".repeat(5000));
        for _ in 0..MAX_PATCH_PATH_DEPTH + 1 {
            data = json!({"k": data});
        }
        let result = plan_stream(SessionId::new(), data, &options(Framing::Patch, 1024));
        assert_matches!(result, Err(PlanError::PathTooDeep { .. }), "{result:?}");
    }

    #[test]
    fn test_options_validation() {
        let too_small = StreamOptions {
            max_frame_size: 1023,
            ..StreamOptions::default()
        };
        let zero_fps = StreamOptions {
            client_fps: Some(0),
            ..StreamOptions::default()
        };
        assert_eq!(
            PlanOptions::try_from(&too_small).unwrap_err(),
            StreamOptionsError::MaxFrameSizeTooSmall {
                requested: 1023,
                min: 1024
            }
        );
        assert_eq!(
            PlanOptions::try_from(&zero_fps).unwrap_err(),
            StreamOptionsError::ZeroClientFps
        );
        let fps = StreamOptions {
            client_fps: Some(4),
            ..StreamOptions::default()
        };
        assert_eq!(
            PlanOptions::try_from(&fps).unwrap().frame_interval,
            Some(Duration::from_millis(250))
        );
    }

    #[test]
    fn test_frame_limit_is_clamped_to_the_outgoing_budget() {
        let huge = StreamOptions {
            max_frame_size: usize::MAX,
            ..StreamOptions::default()
        };
        assert_eq!(
            PlanOptions::try_from(&huge).unwrap().frame_limit,
            MAX_QUEUED_OUTGOING_BYTES - ENVELOPE_RESERVE
        );
    }

    #[test]
    fn test_merge_entry_above_the_budget_is_a_unit_error() {
        let data = json!({"huge": "x".repeat(MAX_QUEUED_OUTGOING_BYTES + 1), "small": 1});
        let result = plan_stream(SessionId::new(), data, &options(Framing::Merge, 64 * 1024));
        assert_matches!(
            result,
            Err(PlanError::UnitExceedsBudget { .. }),
            "{result:?}"
        );
    }

    #[test]
    fn test_more_than_the_frame_cap_is_rejected() {
        let data = Value::Array(
            (0..MAX_PLAN_FRAMES + 50)
                .map(|_| json!("x".repeat(900)))
                .collect(),
        );
        let result = plan_stream(SessionId::new(), data, &options(Framing::Patch, 1024));
        assert_matches!(
            result,
            Err(PlanError::TooManyFrames { .. } | PlanError::PlanBytesExceeded { .. }),
            "{result:?}"
        );
    }

    #[test]
    fn test_patch_plan_of_a_wide_document_is_linear() {
        let items: Map<String, Value> = (0..50_000)
            .map(|i| (format!("k{i:05}"), json!(0)))
            .collect();
        let data = json!({"items": items});
        let started = std::time::Instant::now();

        let plan = patch_round_trip(&data, 64 * 1024);

        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert!(plan.total_bytes < 2 * serde_json::to_string(&data).unwrap().len());
    }

    #[test]
    fn test_small_scalars_and_empty_roots_are_one_frame() {
        for data in [
            json!(1),
            json!(null),
            json!("s"),
            json!([]),
            json!({}),
            json!([1, 2]),
        ] {
            for framing in [Framing::Merge, Framing::Patch] {
                let plan =
                    plan_stream(SessionId::new(), data.clone(), &options(framing, 1024)).unwrap();
                assert_eq!(plan.messages.len(), 1, "{data} {framing:?}");
                assert_eq!(plan.total_bytes, plan.messages[0].len());
            }
        }
    }

    #[test]
    fn test_plan_bytes_cap_rejects_path_amplification_below_the_frame_cap() {
        let mut data = Value::Object(
            (0..200)
                .map(|i| (format!("e{i:03}"), json!("x".repeat(600))))
                .collect(),
        );
        for _ in 0..100 {
            data = json!({ "k".repeat(60): data });
        }

        let result = plan_stream(SessionId::new(), data, &options(Framing::Patch, 1024));

        assert_matches!(
            result,
            Err(PlanError::PlanBytesExceeded { .. }),
            "{result:?}"
        );
    }

    #[test]
    fn test_serialization_failure_is_a_plan_error_not_an_empty_message() {
        let failure = serde_json::from_str::<Value>("{").unwrap_err();

        assert_eq!(serialized(Err(failure)), Err(PlanError::Unserializable));
        assert_eq!(serialized(Ok("{}".to_owned())), Ok("{}".to_owned()));
    }
}
