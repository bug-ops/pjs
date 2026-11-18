---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[061-http-middleware-stack/spec|spec]]"
---

# Delta: single-rate-limit-config

Merged into [[061-http-middleware-stack/spec|spec]] on 2026-11-18 (#704).

## MODIFIED
- FR-004 `RateLimitMiddleware::new` takes `security::RateLimitConfig` (only `max_requests_per_window` and `window_duration` apply to HTTP); the duplicate `infrastructure::http::middleware::RateLimitConfig` and its re-exports are removed, and the 60 s default window lives only in `security`.
- FR-005 trusted proxies are configured only through `RateLimitMiddleware::with_trusted_proxies`.
