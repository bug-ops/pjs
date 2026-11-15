//! Collection of [`Stream`]s keyed only by each stream's own id.

use crate::{
    DomainResult,
    entities::{Frame, Stream, frame::FramePatch, stream::StreamConfig},
    value_objects::{Priority, StreamId},
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::SerializeMap,
};
use std::collections::HashMap;
use std::fmt;

/// Streams indexed by [`StreamId`], where the key is always `stream.id()`.
///
/// [`StreamMap::insert`] derives the key from the stream itself, and decoding
/// rejects any entry whose key differs from its stream's id, so both ways of
/// building a map uphold the invariant. [`StreamMap::get_mut`] hands out a
/// [`StreamMut`], which cannot replace the whole stream.
///
/// On the wire this is a map keyed by the UUID string of each stream.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::entities::{Stream, StreamMap};
/// use pjson_rs_domain::entities::stream::StreamConfig;
/// use pjson_rs_domain::value_objects::{JsonData, SessionId};
///
/// let stream = Stream::new(SessionId::new(), JsonData::Null, StreamConfig::default());
/// let id = stream.id();
///
/// let mut streams = StreamMap::new();
/// assert!(streams.insert(stream).is_none());
/// assert_eq!(streams.len(), 1);
/// assert_eq!(streams.get(id).map(Stream::id), Some(id));
/// ```
#[derive(Debug, Clone, Default)]
pub struct StreamMap(HashMap<StreamId, Stream>);

impl StreamMap {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a stream under its own id, returning the stream it replaced.
    pub fn insert(&mut self, stream: Stream) -> Option<Stream> {
        self.0.insert(stream.id(), stream)
    }

    /// Returns the stream with the given id.
    pub fn get(&self, id: StreamId) -> Option<&Stream> {
        self.0.get(&id)
    }

    /// Returns the stream with the given id for mutation through the
    /// lifecycle operations of [`StreamMut`].
    pub fn get_mut(&mut self, id: StreamId) -> Option<StreamMut<'_>> {
        self.0.get_mut(&id).map(StreamMut)
    }

    /// Number of streams.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the map holds no streams.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over `(id, stream)` pairs in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = (StreamId, &Stream)> {
        self.0.iter().map(|(id, stream)| (*id, stream))
    }

    /// Iterates over the streams in arbitrary order.
    pub fn values(&self) -> impl Iterator<Item = &Stream> {
        self.0.values()
    }

    /// Removes and yields every stream, leaving the map empty.
    pub fn drain(&mut self) -> impl Iterator<Item = Stream> {
        self.0.drain().map(|(_, stream)| stream)
    }
}

/// Mutable access to a [`Stream`] held in a [`StreamMap`].
///
/// Reads go through `Deref<Target = Stream>`; mutation is limited to the
/// forwarded lifecycle methods, so the stream (and with it the map's
/// key-equals-id invariant) can never be swapped for another value.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::entities::{Stream, StreamMap};
/// use pjson_rs_domain::entities::stream::StreamConfig;
/// use pjson_rs_domain::value_objects::{JsonData, SessionId};
///
/// let stream = Stream::new(SessionId::new(), JsonData::Null, StreamConfig::default());
/// let id = stream.id();
/// let mut streams = StreamMap::new();
/// streams.insert(stream);
///
/// let mut handle = streams.get_mut(id).unwrap();
/// handle.start_streaming().unwrap();
/// assert!(handle.is_active());
/// ```
#[derive(Debug)]
pub struct StreamMut<'a>(&'a mut Stream);

impl std::ops::Deref for StreamMut<'_> {
    type Target = Stream;

    fn deref(&self) -> &Stream {
        self.0
    }
}

impl StreamMut<'_> {
    /// Forwards to [`Stream::start_streaming`].
    pub fn start_streaming(&mut self) -> DomainResult<()> {
        self.0.start_streaming()
    }

    /// Forwards to [`Stream::complete`].
    pub fn complete(&mut self) -> DomainResult<()> {
        self.0.complete()
    }

    /// Forwards to [`Stream::fail`].
    pub fn fail(&mut self, error: String) -> DomainResult<()> {
        self.0.fail(error)
    }

    /// Forwards to [`Stream::cancel`].
    pub fn cancel(&mut self) -> DomainResult<()> {
        self.0.cancel()
    }

    /// Forwards to [`Stream::update_config`].
    pub fn update_config(&mut self, config: StreamConfig) -> DomainResult<()> {
        self.0.update_config(config)
    }

    /// Forwards to [`Stream::commit_patch_frames`].
    pub fn commit_patch_frames(
        &mut self,
        patches: Vec<(FramePatch, Priority)>,
        max_frames: usize,
    ) -> DomainResult<Vec<Frame>> {
        self.0.commit_patch_frames(patches, max_frames)
    }

    /// Forwards to [`Stream::finalize_patch_frame`].
    pub fn finalize_patch_frame(
        &mut self,
        priority: Priority,
        frame_patches: Vec<FramePatch>,
    ) -> DomainResult<Frame> {
        self.0.finalize_patch_frame(priority, frame_patches)
    }
}

impl Serialize for StreamMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, stream) in &self.0 {
            map.serialize_entry(&id.as_uuid().to_string(), stream)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for StreamMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StreamMapVisitor;

        impl<'de> Visitor<'de> for StreamMapVisitor {
            type Value = StreamMap;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of stream UUID strings to streams")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<StreamMap, A::Error> {
                use serde::de::Error;

                // Never pre-size from `size_hint`: it is attacker-controlled.
                let mut streams = StreamMap::new();
                while let Some((key, stream)) = access.next_entry::<String, Stream>()? {
                    let key = uuid::Uuid::parse_str(&key).map_err(Error::custom)?;
                    if key != stream.id().as_uuid() {
                        return Err(Error::custom("stream map key does not match stream id"));
                    }
                    if streams.insert(stream).is_some() {
                        return Err(Error::custom("duplicate stream id in stream map"));
                    }
                }
                Ok(streams)
            }
        }

        deserializer.deserialize_map(StreamMapVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::stream::StreamConfig;
    use crate::value_objects::{JsonData, SessionId};

    fn stream() -> Stream {
        Stream::new(SessionId::new(), JsonData::Null, StreamConfig::default())
    }

    fn map_of(streams: impl IntoIterator<Item = Stream>) -> StreamMap {
        let mut map = StreamMap::new();
        for s in streams {
            map.insert(s);
        }
        map
    }

    #[test]
    fn wire_format_is_a_map_keyed_by_uuid_string() {
        let s = stream();
        let key = s.id().as_uuid().to_string();
        let value = serde_json::to_value(map_of([s])).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 1);
        assert_eq!(value[&key]["id"], key);
    }

    #[test]
    fn json_round_trip_preserves_streams() {
        let map = map_of([stream(), stream()]);
        let encoded = serde_json::to_value(&map).unwrap();
        let back: StreamMap = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(serde_json::to_value(&back).unwrap(), encoded);
    }

    #[test]
    fn msgpack_round_trip_preserves_streams() {
        let map = map_of([stream(), stream()]);
        let bytes = rmp_serde::to_vec_named(&map).unwrap();
        let back: StreamMap = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert!(map.values().all(|s| back.get(s.id()).is_some()));
    }

    #[test]
    fn decode_rejects_key_that_is_a_valid_uuid_but_not_the_stream_id() {
        let (a, b) = (stream(), stream());
        let mut value = serde_json::to_value(map_of([a])).unwrap();
        let entry = value
            .as_object_mut()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        value = serde_json::json!({ b.id().as_uuid().to_string(): entry });
        assert!(serde_json::from_value::<StreamMap>(value).is_err());
    }

    #[test]
    fn decode_rejects_malformed_key() {
        let mut value = serde_json::to_value(map_of([stream()])).unwrap();
        let entry = value.as_object().unwrap().values().next().unwrap().clone();
        value = serde_json::json!({ "not-a-uuid": entry });
        assert!(serde_json::from_value::<StreamMap>(value).is_err());
    }

    #[test]
    fn decode_rejects_duplicate_key() {
        let s = stream();
        let key = s.id().as_uuid().to_string();
        let entry = serde_json::to_string(&s).unwrap();
        let raw = format!("{{\"{key}\":{entry},\"{key}\":{entry}}}");
        assert!(serde_json::from_str::<StreamMap>(&raw).is_err());
    }

    #[test]
    fn drain_yields_every_stream_and_empties_the_map() {
        let mut map = map_of([stream(), stream(), stream()]);
        assert_eq!(map.drain().count(), 3);
        assert!(map.is_empty());
    }
}
