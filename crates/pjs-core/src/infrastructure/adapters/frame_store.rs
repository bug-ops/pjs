//! In-memory implementation of [`FrameStoreGat`].
//!
//! Backs the `GET /pjs/sessions/{session_id}/streams/{stream_id}/frames` HTTP
//! endpoint by retaining frames produced through `GenerateFramesCommand` /
//! `BatchGenerateFramesCommand` so callers can fetch them after the fact.
//!
//! Memory is bounded per stream by the cap passed to
//! [`InMemoryFrameStore::with_capacity`] (or the default
//! [`crate::domain::config::DEFAULT_FRAME_HISTORY_PER_STREAM`]); once reached,
//! the oldest frames are evicted FIFO. This prevents a long-lived stream from
//! growing the store without bound.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;

use dashmap::DashMap;

use crate::domain::{
    DomainResult,
    config::DEFAULT_FRAME_HISTORY_PER_STREAM,
    entities::Frame,
    ports::{FrameStoreGat, FrameStorePage},
    value_objects::{Priority, StreamId},
};

/// Lock-free in-memory [`FrameStoreGat`] implementation.
///
/// Frames are stored per stream in a [`VecDeque`] in append order. Lookups
/// scan the per-stream deque under a single shard lock, so cost grows linearly
/// with frame history depth — fine for the bounded sizes we cap at.
#[derive(Debug)]
pub struct InMemoryFrameStore {
    frames: Arc<DashMap<StreamId, VecDeque<Frame>>>,
    max_frames_per_stream: usize,
}

impl InMemoryFrameStore {
    /// Create a store with the default per-stream cap
    /// ([`DEFAULT_FRAME_HISTORY_PER_STREAM`]).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_FRAME_HISTORY_PER_STREAM)
    }

    /// Create a store with an explicit per-stream cap.
    ///
    /// `max_frames_per_stream` must be at least 1 — a zero cap would drop every
    /// frame on insertion and is rejected via debug assertion (the constructor
    /// silently substitutes 1 in release builds).
    pub fn with_capacity(max_frames_per_stream: usize) -> Self {
        debug_assert!(
            max_frames_per_stream > 0,
            "max_frames_per_stream must be at least 1"
        );
        Self {
            frames: Arc::new(DashMap::new()),
            max_frames_per_stream: max_frames_per_stream.max(1),
        }
    }

    /// Number of streams that currently have frame history.
    pub fn stream_count(&self) -> usize {
        self.frames.len()
    }
}

impl Default for InMemoryFrameStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameStoreGat for InMemoryFrameStore {
    type AppendFramesFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetFramesFuture<'a>
        = impl Future<Output = DomainResult<FrameStorePage>> + Send + 'a
    where
        Self: 'a;

    type DeleteFramesForStreamFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn append_frames(
        &self,
        stream_id: StreamId,
        frames: Vec<Frame>,
    ) -> Self::AppendFramesFuture<'_> {
        async move {
            if frames.is_empty() {
                return Ok(());
            }
            let cap = self.max_frames_per_stream;
            let mut entry = self.frames.entry(stream_id).or_default();
            let history = entry.value_mut();
            for frame in frames {
                if history.len() >= cap {
                    history.pop_front();
                }
                history.push_back(frame);
            }
            Ok(())
        }
    }

    fn get_frames(
        &self,
        stream_id: StreamId,
        since_sequence: Option<u64>,
        priority_filter: Option<Priority>,
        limit: Option<usize>,
    ) -> Self::GetFramesFuture<'_> {
        async move {
            let Some(entry) = self.frames.get(&stream_id) else {
                return Ok(FrameStorePage {
                    frames: Vec::new(),
                    total_matching: 0,
                });
            };
            let min_priority = priority_filter.map(|p| p.value());
            let history = entry.value();
            let mut total_matching = 0usize;
            let cap = limit.unwrap_or(usize::MAX);
            let mut out = Vec::new();
            for frame in history.iter() {
                if let Some(since) = since_sequence
                    && frame.sequence() <= since
                {
                    continue;
                }
                if let Some(min) = min_priority
                    && frame.priority().value() < min
                {
                    continue;
                }
                total_matching += 1;
                if out.len() < cap {
                    out.push(frame.clone());
                }
            }
            Ok(FrameStorePage {
                frames: out,
                total_matching,
            })
        }
    }

    fn delete_frames_for_stream(
        &self,
        stream_id: StreamId,
    ) -> Self::DeleteFramesForStreamFuture<'_> {
        async move {
            self.frames.remove(&stream_id);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        entities::frame::FramePatch,
        value_objects::{JsonData, JsonPath},
    };

    fn make_frame(stream_id: StreamId, sequence: u64, priority: Priority) -> Frame {
        let patch = FramePatch::set(
            JsonPath::new(format!("$.field_{sequence}")).unwrap(),
            JsonData::Integer(sequence as i64),
        );
        Frame::patch(stream_id, sequence, priority, vec![patch]).unwrap()
    }

    #[tokio::test]
    async fn appended_frames_are_returned_in_order() {
        let store = InMemoryFrameStore::new();
        let stream_id = StreamId::new();

        let frames = vec![
            make_frame(stream_id, 1, Priority::HIGH),
            make_frame(stream_id, 2, Priority::HIGH),
            make_frame(stream_id, 3, Priority::HIGH),
        ];
        store.append_frames(stream_id, frames).await.unwrap();

        let page = store.get_frames(stream_id, None, None, None).await.unwrap();
        assert_eq!(page.total_matching, 3);
        assert_eq!(
            page.frames.iter().map(Frame::sequence).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[tokio::test]
    async fn since_sequence_filter_excludes_older_frames() {
        let store = InMemoryFrameStore::new();
        let stream_id = StreamId::new();
        store
            .append_frames(
                stream_id,
                vec![
                    make_frame(stream_id, 1, Priority::HIGH),
                    make_frame(stream_id, 2, Priority::HIGH),
                    make_frame(stream_id, 3, Priority::HIGH),
                ],
            )
            .await
            .unwrap();

        let page = store
            .get_frames(stream_id, Some(1), None, None)
            .await
            .unwrap();
        assert_eq!(page.total_matching, 2);
        assert_eq!(
            page.frames.iter().map(Frame::sequence).collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[tokio::test]
    async fn priority_filter_keeps_only_higher_or_equal_priority() {
        let store = InMemoryFrameStore::new();
        let stream_id = StreamId::new();
        store
            .append_frames(
                stream_id,
                vec![
                    make_frame(stream_id, 1, Priority::LOW),
                    make_frame(stream_id, 2, Priority::HIGH),
                    make_frame(stream_id, 3, Priority::CRITICAL),
                ],
            )
            .await
            .unwrap();

        let page = store
            .get_frames(stream_id, None, Some(Priority::HIGH), None)
            .await
            .unwrap();
        assert_eq!(page.total_matching, 2);
        assert_eq!(
            page.frames.iter().map(Frame::sequence).collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[tokio::test]
    async fn limit_caps_returned_frames_but_not_total() {
        let store = InMemoryFrameStore::new();
        let stream_id = StreamId::new();
        store
            .append_frames(
                stream_id,
                (1..=5)
                    .map(|s| make_frame(stream_id, s, Priority::HIGH))
                    .collect(),
            )
            .await
            .unwrap();

        let page = store
            .get_frames(stream_id, None, None, Some(2))
            .await
            .unwrap();
        assert_eq!(page.frames.len(), 2);
        assert_eq!(page.total_matching, 5);
    }

    #[tokio::test]
    async fn capacity_evicts_oldest_first() {
        let store = InMemoryFrameStore::with_capacity(3);
        let stream_id = StreamId::new();
        store
            .append_frames(
                stream_id,
                (1..=5)
                    .map(|s| make_frame(stream_id, s, Priority::HIGH))
                    .collect(),
            )
            .await
            .unwrap();

        let page = store.get_frames(stream_id, None, None, None).await.unwrap();
        assert_eq!(
            page.frames.iter().map(Frame::sequence).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(page.total_matching, 3);
    }

    #[tokio::test]
    async fn delete_frames_drops_history() {
        let store = InMemoryFrameStore::new();
        let stream_id = StreamId::new();
        store
            .append_frames(stream_id, vec![make_frame(stream_id, 1, Priority::HIGH)])
            .await
            .unwrap();

        store.delete_frames_for_stream(stream_id).await.unwrap();
        let page = store.get_frames(stream_id, None, None, None).await.unwrap();
        assert!(page.frames.is_empty());
        assert_eq!(page.total_matching, 0);
    }

    #[tokio::test]
    async fn unknown_stream_returns_empty_page() {
        let store = InMemoryFrameStore::new();
        let page = store
            .get_frames(StreamId::new(), None, None, None)
            .await
            .unwrap();
        assert!(page.frames.is_empty());
        assert_eq!(page.total_matching, 0);
    }
}
