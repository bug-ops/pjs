---
aliases:
  - Event publisher, metrics collector and dictionary store adapters
tags:
  - sdd
  - spec
  - infrastructure
  - adapters
  - events
  - metrics
created: 2026-11-15
status: partial
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[052-domain-gat-ports/spec|Domain GAT ports]]"
  - "[[055-application-commands/spec|Application commands]]"
  - "[[057-in-memory-repositories/spec|In-memory repositories]]"
  - "[[009-realtime-metrics-observability/spec|Metrics and observability (009)]]"
  - "[[011-zstd-dictionary-compression/spec|zstd dictionary compression (011)]]"
code:
  - crates/pjs-core/src/infrastructure/adapters/event_publisher.rs
  - crates/pjs-core/src/infrastructure/adapters/metrics_collector.rs
  - crates/pjs-core/src/infrastructure/bounded_channel.rs
  - crates/pjs-core/src/infrastructure/repositories/dictionary_store.rs
  - crates/pjs-core/tests/event_publisher_comprehensive.rs
verified: 2026-11-15
---

# Spec 058: Event Publisher, Metrics Collector and Dictionary Store Adapters

> [!info] Verification (2026-11-15)
> **State: partial.** `InMemoryEventPublisher` and `InMemoryDictionaryStore` are implemented, bounded and used by the HTTP server paths. `InMemoryMetricsCollector` is implemented and exported but has no caller outside its own tests; the Prometheus path of [[009-realtime-metrics-observability/spec|009]] uses the `metrics` feature and crate instead.

## 1. Overview

### Problem
Domain events need an in-process sink with bounded memory and optional streaming to a consumer; metrics and trained dictionaries need in-process stores with hard memory limits.

### Goal
Document the three adapters and their bounds.

### Out of scope
- Zstd training and codec behaviour ([[011-zstd-dictionary-compression/spec|011]]).
- HTTP `metrics` feature and `/metrics` endpoint ([[009-realtime-metrics-observability/spec|009]]).
- Port definitions ([[052-domain-gat-ports/spec|052]]).

## 2. User Stories
- As a test or embedding application, I inspect published events by type or session and subscribe by callback or channel.
- As an operator, I know event, channel and dictionary memory are bounded.
- As a client, I get a trained dictionary for my session after enough frames were generated.

## 3. Functional Requirements

### FR-001 Event publishing
`InMemoryEventPublisher` shall implement `EventPublisherGat`. `publish(event)` shall create a `StoredEvent { id: EventId::new(), event_type: event.event_type(), session_id: Some(event.session_id()), timestamp: event.occurred_at(), metadata: event.metadata(), sequence }`, insert it in the log, evict if over capacity, offer it to the streaming channel, then invoke every notification callback synchronously. `sequence` is a per-instance monotonic counter (`AtomicU64`). Each stored event gets a fresh `EventId` (no content hashing). PR #352.
Code: `event_publisher.rs`.

### FR-002 Batch publishing
`publish_batch(events)` shall reserve a contiguous sequence block, convert and store the events in parallel with `rayon::into_par_iter` (callbacks run on rayon threads in unspecified order), then try-send all to the channel in order and evict once at the end. An empty batch is a no-op.

### FR-003 Bounded event log
The log shall hold at most `EVENT_LOG_CAPACITY` = 10 000 entries; when exceeded, entries with the lowest `sequence` shall be removed until `EVENT_LOG_EVICT_TARGET` = 9 000 remain. Eviction sorts a snapshot by sequence and removes `len - 9000` oldest, also correct for batches larger than the gap. Race regression test `test_concurrent_eviction_forced_interleaving_stays_at_target` (#353).

### FR-004 Streaming channel
`InMemoryEventPublisher::with_channel()` shall return the publisher and an `mpsc::Receiver<Envelope<StoredEvent>>` created by `byte_bounded_channel(EVENT_CHANNEL_CAPACITY = 1000, MAX_QUEUED_EVENT_BYTES = 16 MiB)`. Events are sent with `try_send` and an approximate size (`event_type` + metadata strings + 96 bytes); when the channel is full the event is dropped and a `warn!` is logged, publishing never blocks or fails.

### FR-005 Subscriptions and inspection
`add_notification_callback(Fn(&DomainEvent) + Send + Sync) -> u64` and `remove_notification_callback(id)`; `event_count`, `events_by_type(&str)`, `events_for_session(SessionId)`, `recent_events(limit)` (newest first by sequence), `clear`. `Clone` shares all state.

### FR-006 Metrics collector
`InMemoryMetricsCollector` shall implement `MetricsCollectorGat` and `SessionMetricsGat` over `PerformanceMetrics` plus per-session, per-stream maps and a time series (`parking_lot::RwLock`).
- `increment_counter`: known names `sessions_created`, `streams_created`, `frames_processed`, `bytes_streamed`, `errors`; unknown names ignored.
- `set_gauge`: `active_sessions`, `active_streams`, `average_frame_processing_time_ms`; others ignored.
- `record_timing("frame_processing", d, tags)`: running average over `frames_processed`; `session_id`/`stream_id` tags (parsed strings) refresh `last_activity` or push the duration to the stream's `processing_times`.
- `record_session_created/ended`, `record_stream_created/completed`: maintain active/total counters (decrements saturate at 0), session and stream records, and a time-series snapshot after session events; the series is trimmed by dropping the oldest 100 once it exceeds 1000.
- `performance_snapshot`, `session_metrics`, `stream_metrics`, `time_series(minutes)`, `clear`, `export_prometheus` (text with `pjs_active_sessions`, `pjs_total_sessions_created`, `pjs_active_streams`, `pjs_frames_processed_total`, `pjs_bytes_streamed_total`, `pjs_errors_total`, `pjs_frame_processing_time_ms`).
Status: implemented, not wired.

### FR-007 Dictionary store training
`InMemoryDictionaryStore::new(Arc<CompressionBombDetector>, target_dict_size)` (adapter: feature `compression`, not wasm) shall implement `DictionaryStore`. `train_if_ready(session, sample)` shall: return early if the dictionary exists; under the session's corpus mutex append the sample only if `corpus.len() < N_TRAIN` (32), `sample.len() <= 1 MiB` and the global reservation stays within `TOTAL_CORPUS_BYTE_BUDGET` (128 MiB), otherwise skip it; once 32 samples exist, snapshot the corpus and train once through `OnceCell::get_or_try_init` inside `spawn_blocking` using `ZstdDictCompressor::train(snapshot, target)`; validate the result with `CompressionBombDetector::validate_pre_decompression`; release the byte reservation through a drop guard (also on cancellation). `target_dict_size` is clamped to `MAX_DICT_SIZE` (112 KiB). PR #222, #360.
`get_dictionary(session)` returns `Ok(Some(Arc<TrainedDictionary>))` after training, else `Ok(None)`, and refreshes `last_access`.
`register(session, dict)` shall validate the dictionary size with the bomb detector (`Error::InvalidDictionary(DictionaryError::Rejected)` on rejection) and set it if none exists.

### FR-008 Dictionary eviction
Session state shall be evicted after `SESSION_TTL` = 30 min of inactivity by a background task spawned in `new` when a Tokio runtime is present (interval `SESSION_CLEANUP_INTERVAL` = 5 min, holds only `Weak` references so it stops when the store drops); without a runtime a `warn!` is logged and no task starts. `cleanup_expired(ttl)` runs one pass manually and resynchronizes the global byte counter. `release_corpus_bytes` saturates instead of wrapping; millisecond clocks and TTLs convert with `u64::try_from(..).unwrap_or(u64::MAX)`, so a TTL beyond `u64` milliseconds evicts nothing (#762).

## 4. Non-Functional Requirements
- NFR-MEM-001: event log at most about 10 000 entries; channel at most 1000 messages and 16 MiB; dictionary corpus at most 128 MiB process-wide and 32 MiB per session (`N_TRAIN * 1 MiB`).
- NFR-PERF-001: `publish` is lock-free on the log (DashMap); `publish_batch` parallelizes conversion.
- NFR-SEC-001: training never holds the corpus mutex across `spawn_blocking`; oversized or over-budget samples are skipped, not errors.
- NFR-TEST-001: 21 tests in `dictionary_store.rs` (race-free single training, budget release on drop, saturation), 7 in `event_publisher.rs`, 7 in `metrics_collector.rs`, plus `tests/event_publisher_comprehensive.rs`.

## 5. Data Model
`StoredEvent { id: EventId, event_type: String, session_id: Option<SessionId>, timestamp: jiff::Timestamp, metadata: HashMap<String,String>, sequence: u64 }`; `PerformanceMetrics`, `SessionMetrics`, `StreamMetrics`, `TimestampedMetrics`; `SessionDictState { corpus: Mutex<Vec<Vec<u8>>>, dict: OnceCell<Arc<ZstdDictionary>>, last_access, pending_corpus_bytes }`.

## 6. Edge Cases and Errors

| Case | Result |
|---|---|
| channel full or over byte budget | event dropped, `warn!`, publish returns `Ok` |
| log above 10 000 | trimmed to 9 000 oldest-first |
| sample above 1 MiB | skipped |
| corpus budget exhausted | sample skipped, `debug!` |
| training failure | `Err(Error::Codec { codec: Zstd, operation: Train, .. })`; reservation released |
| `register` of oversized dictionary | `Err(Error::InvalidDictionary(DictionaryError::Rejected))` |
| `train_if_ready` without Tokio runtime | works; eviction task not started |

## 7. Success Criteria
- Event log never exceeds 10 000 after any publish completes (single-threaded) and returns to 9 000 after eviction.
- At most one training run per session under concurrent submitters.

## 8. Agent Boundaries
- Always: mint a fresh `EventId` per stored event; keep every in-memory buffer bounded.
- Ask first: changing capacities (10 000, 1000, 16 MiB, 128 MiB); wiring `InMemoryMetricsCollector` into handlers.
- Never: block `publish` on a full channel; hold a lock across `spawn_blocking`.

## 9. Open Questions
- `InMemoryMetricsCollector` is unused in production; its `session_metrics`/`stream_metrics` maps and per-stream `processing_times` are unbounded (streams are never removed), counters are matched by `&str` names, and `export_prometheus` is not served by any route. [NEEDS CLARIFICATION: remove, or bound and wire.]
- `StoredEvent.event_type` is a `String` (should reuse the domain discriminant), `session_id` is always `Some`.
- Callbacks run inline in `publish`; a panicking or slow callback breaks or stalls publishing; in `publish_batch` they run on rayon worker threads from inside an async task (blocks the executor thread for the batch duration).
- The `DictionaryStore` port is not feature-gated and returns `DomainResult` (#605); it lacks a remove method (`TODO(#144 follow-up)` in `dictionary_store.rs`).
- `InMemoryDictionaryStore::register` is public but only tests call it (`handlers/dictionary.rs` test); the production path is training only.
- `max_time_series_entries` is fixed at 1000 with no builder.

## 10. See Also
[[058-event-metrics-dictionary-adapters/plan|plan]], [[009-realtime-metrics-observability/spec|009]], [[011-zstd-dictionary-compression/spec|011]], `crates/pjs-core/src/infrastructure/bounded_channel.rs`.
