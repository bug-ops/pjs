---
aliases:
  - ADR-002
  - Instruction-count bench gate
tags:
  - sdd
  - adr
  - decision
  - benchmarks
  - ci
  - pjs
created: 2026-11-18
status: accepted
issue: "#695"
code:
  - crates/pjs-bench/benches/
  - .github/workflows/ci.yml
related:
  - "[[075-bench-suite/spec|075 Bench suite]]"
  - "[[testing-strategy]]"
  - "[[decisions/adr-001-allocator-decision|ADR-001 allocator]]"
  - "[[MOC-specs]]"
---

# ADR-002: Deterministic instruction-count regression gate for benchmarks

## Status

Accepted; limit provisional until the variance is measured. #695 was the written evaluation. #736 adds the `gungraun` dev-dependency (Linux only), the `gungraun_parser` bench and a report-only CI job outside `ci-success` (user-approved exception to the gating rule). Making the job gating is a follow-up.

## Context

The project rule is that a benchmark regression above 10% is a P1. Nothing enforces it: CI does not run `pjs-bench` (it only checks that `client_apply_patches` compiles under `--cfg pjs_bench`), and every bench is a Criterion wall-clock bench. Wall-clock numbers depend on the machine and its load, so spec 075 relies on `noise_threshold(0.15)` for the e2e group and on the quiet-machine A/B procedure from #619. Hosted runners are too noisy to gate a 10% threshold on wall-clock time.

## Decision

1. Use **gungraun 0.20.0** (formerly `iai-callgrind`; current crates.io release verified 2026-11-18) as the deterministic harness. It runs benches under Valgrind Callgrind and reports instruction counts (Ir) with per-benchmark regression limits. It is self-contained: no hosted service.
2. Rejected: `codspeed-criterion-compat`. It would keep the Criterion API but measures through a hosted external service, adding a third-party service dependency and an account/token to the CI trust boundary.
3. Linux only: Valgrind does not run on the macOS development machines, so the gate is a single CI job on `ubuntu-latest`.
4. Job shape: path-filtered to `crates/pjs-core/**` and `crates/pjs-bench/**`; third-party actions pinned to full commit SHAs; the baseline is produced from the base branch and compared against the PR head.
5. Candidate subset (stable, CPU-bound benches only): the parser group, the planner, frame serialization and `client_apply_patches`. I/O-bound and scheduling-sensitive benches (lifecycle, WebSocket controller concurrency) stay Criterion-only.
6. Limit: **+5% Ir is provisional and unmeasured.** It is a starting proposal, not a result. Run-to-run variance of gungraun on a hosted runner and the final per-bench limits are to be measured in the implementation follow-up (#736) before the gate is enabled. No figure in this ADR is a measurement.
7. Supply chain: run `cargo deny check` over gungraun and its dependency tree before adding it (`deny.toml` licenses and advisories), and pin `gungraun-runner` to exactly the same version as the `gungraun` crate, since a mismatch is rejected at runtime.
8. Criterion stays for local wall-clock work. The quiet-machine A/B procedure (#619) remains the only basis for throughput claims; instruction counts do not capture cache, memory-bandwidth or contention effects and do not replace it.

## Consequences

- The 10% rule gets an automated, low-noise gate for the CPU-bound hot paths, at the cost of maintaining a second bench harness and a Valgrind CI job.
- Ir does not reflect SIMD width or memory behaviour one-to-one, so a wall-clock regression can pass the gate and an Ir increase can be harmless; the gate is a tripwire, not a throughput measurement.
- Benches in the subset need gungraun ports (setup excluded from measurement); this duplicates a small amount of bench code unless shared setup is extracted.

## Open follow-ups

- Measure run-to-run variance on a hosted runner, fix the subset and the final limit, then make the job gating and add it to `ci-success`.

## See also

- [[075-bench-suite/spec|075 Bench suite]]
- [[testing-strategy]]
