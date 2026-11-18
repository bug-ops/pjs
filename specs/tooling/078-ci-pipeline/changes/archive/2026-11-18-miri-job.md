---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[078-ci-pipeline/spec|spec]]"
---

# Delta: miri-job

Merged into [[078-ci-pipeline/spec|spec]] on 2026-11-18 (#716, #697, #692).

## ADDED
- FR-016 Miri job: job `miri` (ubuntu, `timeout-minutes: 20`, on rust changes, push, schedule, manual) shall install the pinned nightly with `miri` and `rust-src` through the `setup-rust` composite action and run `cargo miri test -p pjson-rs --lib -- parser::buffer_pool parser::aligned_alloc` with `MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks"`. Tree Borrows is required because crossbeam-epoch (via rayon in `BufferPool`) is rejected by Stacked Borrows; `-Zmiri-ignore-leaks` because rayon's global pool threads outlive `main`. Measured locally: 36 tests, about 14 s run time, about 100 s cold.
- Secret hygiene (#697): `.gitignore` excludes `.env`, `.env.*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `secrets/`; `.gitleaks.toml` extends the default rules and allowlists the RFC 6455 sample nonce `dGhlIHNhbXBsZSBub25jZQ==`. No gitleaks CI job.

## MODIFIED
- FR-011 the `ci-success` aggregator also requires `miri` (in `needs` and in the check script).
- Job graph: `changes` -> `miri`.
- `deny.toml`, `osv-scanner.toml` and `fuzz/osv-scanner.toml` no longer ignore RUSTSEC-2026-0543 (chrono is gone, #692).
