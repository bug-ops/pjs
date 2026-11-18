---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[044-domain-entities-events-priority-heuristic/spec|spec]]"
---

# Delta: timestamps-jiff

Merged into [[044-domain-entities-events-priority-heuristic/spec|spec]] on 2026-11-18 (#692).

## MODIFIED
- FR-044-003 `Frame::timestamp()` returns `jiff::Timestamp` (was `DateTime<Utc>`).
- FR-044-004 `Stream::created_at/updated_at/completed_at` return `jiff::Timestamp`; `Stream::duration()` returns `Option<jiff::SignedDuration>`.
- FR-044-009 `DomainEvent` variants are timestamped with `jiff::Timestamp`; `DomainEvent::timestamp()`/`occurred_at()` return it; `EventStore::get_events_since` takes it. `metadata()["timestamp"]` renders with `Z` (was `+00:00`).
- Entities call `Timestamp::now()` directly (was `Utc::now()`); the clock-injection caveat in the Never/Known-issues text is unchanged.
- Serialized timestamp form: see the 042 delta `timestamps-jiff` (`Z`, shortest exact fraction, no grouping by three).
