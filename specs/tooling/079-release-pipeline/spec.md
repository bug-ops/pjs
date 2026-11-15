---
aliases:
  - release pipeline
  - publishing
tags:
  - sdd
  - spec
  - release
  - ci
  - npm
created: 2026-11-15
status: implemented
priority: P1
related:
  - "[[078-ci-pipeline/spec]]"
  - "[[072-wasm-packaging-targets/spec]]"
  - "[[073-js-client-core/spec]]"
  - "[[075-bench-suite/spec]]"
code:
  - .github/workflows/release.yml
  - Cargo.toml
  - crates/pjs-core/Cargo.toml
  - crates/pjs-js-client/package.json
  - scripts/profile.sh
  - CHANGELOG.md
verified: 2026-11-15
---

# Spec 079: release and publish pipeline

Part of [[MOC-specs]]; see [[constitution]]. PR-time checks are in [[078-ci-pipeline/spec]].

> [!info] Verification (2026-11-15)
> Implemented. `.github/workflows/release.yml` (327 lines) read in full; releases `v0.6.2`, `v0.6.3`, `v0.7.0` exist as tags and in `CHANGELOG.md`. Since then fixed: #617 (publish job installs nightly), #588 (least-privilege permissions) and #589 (`@pjson/node` installs from `package-lock.json`).

## Overview

**Problem.** One tag must publish the Rust crates to crates.io and two npm packages with provenance, only after the full verification suite passes.

**Goal.** Describe the tag-triggered workflow, the version gate, the three publishers and the GitHub release.

**Out of scope.** Version-bump preparation (a `release:` commit/PR per `.claude/rules/commits-and-issues.md`), changelog content rules.

## Functional Requirements

### FR-001 Trigger and version gate
The workflow shall run on pushed tags matching `v[0-9]+.[0-9]+.[0-9]+`. Job `version-check` shall derive the version from the tag and fail unless it equals the first `version = "..."` line of the root `Cargo.toml` (`[workspace.package]` version, currently 0.7.0).
Status: done.

### FR-002 Verification gates
Before any publish, jobs `format-check` (`cargo fmt --all -- --check`), `clippy-check` (`cargo clippy --workspace --all-features --all-targets -- -D warnings`) and `test-check` (`cargo nextest run --workspace --all-features --all-targets --no-fail-fast`, then `cargo test --workspace --doc --all-features`) shall pass. `test-check` includes integration tests and `pjs-demo` tests that PR CI does not run ([[078-ci-pipeline/spec]]).
Status: done.

### FR-003 crates.io publish
Job `publish` (environment `crates-io`, needs all gates) shall authenticate through crates.io Trusted Publishing (`rust-lang/crates-io-auth-action`, OIDC) and run `katyo/publish-crates` with `ignore-unpublished-changes: true` and `publish-delay: 10000`. Publishable crates are those with `publish = true`: `pjson-rs` (`crates/pjs-core`) and `pjson-rs-domain`; `pjs-wasm`, `pjs-bench`, `pjs-demo` inherit workspace `publish = false`. `pjs-core` carries `[package.metadata.release.publishers.crates-io]` with `trusted-publishing = true`, workflow `release.yml`.
Status: done. The job installs the nightly toolchain, like the gates (#617).

### FR-004 WASM npm publish
Job `publish-wasm` (environment `npm`, Node 24) shall build `wasm-pack build crates/pjs-wasm --target web --release --scope pjson`, rewrite `pkg/package.json` with `jq` (name `@pjson/wasm`, `collaborators` -> `contributors`, repository URL normalised to `git+...git`), skip when `npm view @pjson/wasm@<version>` already exists, else `npm publish --access public --provenance`.
Status: done.

### FR-005 JS client npm publish
Job `publish-node` (environment `npm`, Node 24) shall rewrite `crates/pjs-js-client/package.json` to name `@pjson/node`, version = tag version, and rewrite the optional dependency `pjs-wasm` to `npm:@pjson/wasm@^<version>`; skip if already published; else `npm install --no-package-lock --ignore-scripts`, `npm run build`, `npm publish --access public --provenance --ignore-scripts`.
Status: done. The committed lockfile is not used for the publish build (#589).

### FR-006 GitHub release and notification
Job `github-release` (needs `publish`, `publish-wasm`, `publish-node`) shall create the release `Release v<version>` for the tag with generated notes (`softprops/action-gh-release`, `contents: write`). Job `notify` prints the release, crates.io and npm URLs.
Status: done.

### FR-007 Idempotent re-runs
npm publishes shall be skipped when the exact version exists; crates publishing tolerates already-published versions (`ignore-unpublished-changes`).
Status: done.

### FR-008 Release environments and permissions
Workflow-level `permissions` are `contents: read`; the three publish jobs add `id-token: write` (OIDC) and only `github-release` adds `contents: write` (#588).
Status: done.

### FR-009 Local profiling helper
`scripts/profile.sh` shall provide `cpu`, `heap`, `bench`, `help` subcommands for performance work before releases ([[075-bench-suite/spec]]).
Status: partial (see [[075-bench-suite/spec]] Open Questions).

## Non-Functional Requirements

- NFR-SEC-001: no long-lived registry tokens; crates.io and npm use OIDC (Trusted Publishing; both npm jobs use Node 24).
- NFR-SEC-002: provenance attestation on both npm packages (`--provenance`).
- NFR-SEC-003: all actions SHA-pinned.
- NFR-REL-001: publish only after format, clippy, tests and doctests pass on the tagged commit.
- NFR-TIME-001: job timeouts in the workflow: version-check 5, format 10, clippy 15, test 30, publish 30, publish-wasm 20, publish-node 20, github-release 10 minutes.

## Data Model

Version sources: root `Cargo.toml` `[workspace.package].version` (single source for all crates), tag `vX.Y.Z`; npm versions derived from the tag at publish time (in-repo `crates/pjs-js-client/package.json` stays `0.3.0-alpha.2`; `pjs-wasm` pkg version comes from Cargo).

## Edge Cases and Errors

| Case | Behaviour |
|------|-----------|
| tag differs from Cargo version | `version-check` fails, nothing publishes |
| npm version already published | publish step skipped |
| one publisher fails | `github-release` (needs all three) does not run |
| crates published before npm failure | partial release; recovery is a manual re-run |

## Success Criteria

1. Tag `vX.Y.Z` yields crates `pjson-rs` and `pjson-rs-domain` at X.Y.Z, npm `@pjson/wasm` and `@pjson/node` at X.Y.Z, and a GitHub release.
2. Re-running the workflow on the same tag publishes nothing new.

## Agent Boundaries

- Always: run the full local suite and update `CHANGELOG.md` before tagging; keep `release.yml` checks a superset of PR CI.
- Ask first: changing publish order, environments, or package names.
- Never: push a tag without a matching version bump; publish with a static token.

## Open Questions

- Name inconsistency across in-repo files (`@pjs/client` vs `@pjson/node`; `pjs-wasm` vs `@pjson/wasm`).
- The in-repo `@pjs/client` `VERSION` constant (`0.3.0-alpha.1`) is not updated by the pipeline, so the published `@pjson/node` reports a different version than its `package.json`.
- Release preparation (version bump, changelog finalisation) is manual; `release:` commit convention only.
- `crates.io` publish order for `pjson-rs` depending on `pjson-rs-domain` relies on `publish-crates` dependency ordering with a 10 s delay.

## See Also

[[plan]] | [[078-ci-pipeline/spec]] | [[072-wasm-packaging-targets/spec]] | `CHANGELOG.md` | `SECURITY.md`
