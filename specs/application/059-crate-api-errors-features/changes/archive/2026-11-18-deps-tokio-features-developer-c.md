---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[059-crate-api-errors-features/spec|spec]]"
---

# Delta: deps-tokio-features

Merged into [[059-crate-api-errors-features/spec|spec]] on 2026-11-18 (#709, #692, #710).

## MODIFIED
- FR-059-008 always-on dependencies: `chrono` is replaced by `jiff` (`std`, `serde`); `tokio` is enabled with `rt-multi-thread`, `sync`, `time`, `macros`, `net` (the `parking_lot` feature is removed). `cargo tree -p pjson-rs -e normal --duplicates --all-features` no longer lists `parking_lot` 0.12; `lock_api`/`parking_lot_core` duplicates remain through `dashmap`. The direct `parking_lot` 0.13 dependency (`BufferPool` statistics) is unchanged. `pjs-demo` keeps tokio `full` and is out of scope.
- Minute-valued durations in non-test code are written with `Duration::from_mins` (`STREAM_GRACE`, `SESSION_CLEANUP_INTERVAL`, `RESPONSE_BODY_IDLE_TIMEOUT`, `RateLimitConfig::default().window_duration`); no MSRV impact because `pjson-rs` declares none.
