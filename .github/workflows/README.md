# GitHub Actions Workflows

This document describes all GitHub Actions workflows used in the PJS project.

## Overview

The PJS project uses a multi-stage CI/CD pipeline to ensure code quality, performance, and compatibility across platforms and build targets.

### Workflow Summary

| Workflow | Trigger | Purpose | Status |
|----------|---------|---------|--------|
| [Rust Build](#rust-build) | push, PR | Build for multiple allocators | [![Rust Build](../../.github/workflows/rust.yml) |
| [Rust Test & Coverage](#rust-test--coverage) | push, PR | Cross-platform testing and coverage | [![Rust Test](../../.github/workflows/rust-test.yml) |
| [WASM Build & Test](#wasm-build--test) | push, PR, dispatch | WebAssembly builds and validation | [![WASM Build](../../.github/workflows/wasm.yml) |
| [Rust Clippy](#rust-clippy) | push, PR | Static analysis | [![Clippy](../../.github/workflows/rust-clippy.yml) |
| [Security Scanner](#osv-scanner) | PR | Dependency vulnerability scanning | [![OSV Scanner](../../.github/workflows/osv-scanner-pr.yml) |

---

## Rust Build

**File:** `.github/workflows/rust.yml`

Compiles the Rust workspace with multiple memory allocator configurations.

### Triggers
- Push to `main` or `develop`
- Pull requests to `main` or `develop`

### Jobs

#### Build (Matrix)
Runs on: `ubuntu-latest`
Timeout: 30 minutes

Tests three allocator configurations:
- **system** - Default system allocator
- **jemalloc** - tikv-jemallocator (performance-optimized)
- **mimalloc** - Microsoft's mimalloc (memory-efficient)

**Features tested:**
```
schema-validation,compression,http-server,[allocator]
```

**Steps:**
1. Checkout code
2. Install Rust nightly
3. Setup sccache for incremental compilation
4. Cache Cargo dependencies
5. Build with features
6. Build release optimized version
7. Report sccache stats

**Performance Optimizations:**
- Rust nightly for zero-cost GAT abstractions
- sccache for cross-job compilation caching
- Cargo incremental builds disabled (CI friendly)
- Fail-fast disabled to test all allocators

### Artifacts
- Build artifacts (target/ directory)
- sccache statistics

### Caching Strategy
- **Cargo cache:** Shared across build jobs, saved on main branch
- **sccache:** Persists compilation artifacts between runs
- **Key:** `build-${{ matrix.allocator }}`

---

## Rust Test & Coverage

**File:** `.github/workflows/rust-test.yml`

Runs comprehensive tests across platforms with code coverage tracking.

### Triggers
- Push to `main` or `develop`
- Pull requests to `main` or `develop`

### Jobs

#### Test (Cross-Platform Matrix)
Runs on: `ubuntu-latest`, `macos-latest`, `windows-latest`
Timeout: 45 minutes

Tests across three platforms and three allocators:
- **Platforms:** Linux (Ubuntu), macOS, Windows
- **Allocators:** system, jemalloc, mimalloc (jemalloc excluded on Windows)
- **Total combinations:** 8 (macOS & Windows use system allocator only)

**Features tested:**
```
schema-validation,compression,http-server,[allocator]
```

**Steps:**
1. Checkout code
2. Install Rust nightly with llvm-tools
3. Setup sccache (platform-specific)
4. Cache Cargo dependencies
5. Install cargo-nextest
6. Build all targets
7. Run tests with nextest
8. Run doctests
9. Report sccache stats

**Test Execution:**
```bash
cargo nextest run --workspace --features [features] --no-fail-fast
cargo test --workspace --doc --features [features]
```

**Performance Optimizations:**
- cargo-nextest for parallel test execution (3x faster)
- No fail-fast on first test to catch all failures
- Separate sccache per platform
- Platform-specific dependency setup

#### Coverage
Runs on: `ubuntu-latest`
Timeout: 30 minutes

Generates code coverage reports with codecov integration.

**Steps:**
1. Checkout code
2. Install Rust nightly
3. Install cargo-llvm-cov
4. Install cargo-nextest
5. Cache Cargo dependencies
6. Generate LCOV format coverage
7. Upload to codecov.io
8. Generate HTML coverage report
9. Upload artifacts

**Coverage Command:**
```bash
cargo llvm-cov --features schema-validation,compression,http-server \
  --workspace --lcov --output-path lcov.info nextest
```

**Coverage Requirements:**
- **Business logic (domain/):** Minimum 80%
- **Security-critical code:** 100%
- **Application layer:** 70%
- **Infrastructure layer:** 60%

### Artifacts
- `lcov.info` - LCOV format coverage report
- HTML coverage report in `target/llvm-cov/html/`
- Codecov integration (if token configured)

### Caching Strategy
- **Cargo cache:** Shared `coverage` key, saved on main
- **sccache:** Platform-specific caching
- **Key:** `test-${{ matrix.os }}`

---

## WASM Build & Test

**File:** `.github/workflows/wasm.yml`

Builds and validates WebAssembly binaries for the PJS library.

### Triggers
- Push to `main` or `develop` with changes to:
  - `crates/pjs-wasm/**`
  - `crates/pjs-domain/**`
  - `crates/pjs-js-client/examples/browser-wasm/**`
  - `.github/workflows/wasm.yml`
- Pull requests to `main` or `develop`
- Manual workflow dispatch

### Jobs

#### WASM Library Tests
Runs on: `ubuntu-latest`
Timeout: 20 minutes

Tests Rust library code for WASM crates.

**Steps:**
1. Checkout code
2. Install Rust nightly with llvm-tools
3. Cache Cargo dependencies
4. Install cargo-nextest
5. Run pjs-wasm library tests
6. Run pjs-domain library tests
7. Run doctests

**Test Command:**
```bash
cargo nextest run -p pjs-wasm --lib --no-fail-fast
cargo nextest run -p pjs-domain --lib --no-fail-fast
cargo test -p pjs-wasm --doc
cargo test -p pjs-domain --doc
```

#### Build WASM (Multi-Target)
Runs on: `ubuntu-latest`
Timeout: 30 minutes

Builds WASM binaries for three targets:
- **web** - Browser ES modules (bundler target)
- **nodejs** - Node.js CommonJS
- **bundler** - Generic bundler target (Webpack, Rollup, Vite, etc.)

**Features:**
- Release optimization (`-Os` size optimization)
- wasm32-unknown-unknown target
- Full TypeScript definitions generation

**Steps per target:**
1. Checkout code
2. Install Rust nightly with wasm32-unknown-unknown
3. Setup sccache
4. Cache Cargo & wasm-pack
5. Install wasm-pack
6. Build WASM: `wasm-pack build --target [web|nodejs|bundler] --release`
7. Verify build (check for required files)
8. Validate TypeScript definitions
9. Report sccache stats
10. Upload artifacts

**Build Configuration:**
```toml
[profile.release]
opt-level = "s"      # Size optimization
lto = true           # Link-time optimization

[package.metadata.wasm-pack.profile.release]
wasm-opt = ["-Os"]   # Post-processing size optimization
```

**Expected Files:**
```
pkg/
├── pjs_wasm.js              # JS bindings
├── pjs_wasm.d.ts            # TypeScript definitions
├── pjs_wasm_bg.wasm         # WASM binary
├── pjs_wasm_bg.wasm.d.ts    # WASM TS definitions
└── package.json             # NPM metadata
```

#### WASM Bundle Size
Runs on: `ubuntu-latest`
Timeout: 15 minutes

Analyzes and validates WASM bundle sizes.

**Requirements:**
- Raw binary: < 200 KB
- Gzipped: < 80 KB

**Analysis:**
- Calculates raw and gzipped sizes for all targets
- Generates markdown report
- Posts results as PR comment
- Fails if thresholds exceeded

**Report Format:**
```
| Target | Raw (KB) | Gzipped (KB) | Status |
|--------|----------|-------------|--------|
| web    | 145      | 52          | ✓ PASS |
| nodejs | 148      | 54          | ✓ PASS |
| bundler| 145      | 52          | ✓ PASS |
```

#### Node.js Example Test
Runs on: `ubuntu-latest`
Timeout: 15 minutes

Tests the Node.js example with real WASM binary.

**Steps:**
1. Checkout code
2. Download web build artifact
3. Setup Node.js 18
4. Run example: `node example.js`
5. Verify output contains expected strings

**Example Validation:**
- Must output "Example 1: Basic Usage"
- Must show frame generation
- Must complete without errors

#### NPM Package Validation
Runs on: `ubuntu-latest`
Timeout: 15 minutes

Validates generated npm package structure and content.

**Validations:**
1. **package.json:**
   - Required fields: name, version, files, main, types
   - Correct metadata (author, license, etc.)

2. **Required files:**
   - `pjs_wasm.js` - JS bindings
   - `pjs_wasm.d.ts` - TypeScript definitions
   - `pjs_wasm_bg.wasm` - Binary
   - `pjs_wasm_bg.wasm.d.ts` - Binary TS definitions
   - `package.json` - Metadata
   - `README.md` - Documentation

3. **TypeScript definitions:**
   - `class PjsParser` - Main parser class
   - `class PriorityConstants` - Constants
   - `class PriorityConfigBuilder` - Config builder
   - `export function version()` - Version function

#### WASM Code Quality
Runs on: `ubuntu-latest`
Timeout: 20 minutes

Checks code quality and standards for WASM crates.

**Steps:**
1. Checkout code
2. Install Rust nightly with rustfmt & clippy
3. Cache Cargo dependencies
4. Check formatting: `cargo fmt --check`
5. Clippy analysis: `cargo clippy -- -D warnings`
6. Documentation check: `cargo doc --no-deps`
7. Verify no unnecessary unsafe code

**Quality Checks:**
- Format compliance (nightly rustfmt)
- No clippy warnings (strict mode)
- Documentation completeness
- Unsafe code analysis

#### WASM CI Success
Runs on: `ubuntu-latest`

Final gate job that validates all WASM jobs passed.

**Validates:**
- wasm-test
- wasm-build (all targets)
- wasm-bundle-size
- wasm-node-example
- wasm-package-validation
- wasm-quality

### Artifacts

| Artifact | Retention | Purpose |
|----------|-----------|---------|
| `wasm-build-[web\|nodejs\|bundler]` | 7 days | Built WASM package |
| `bundle-size-report` | 30 days | Size analysis report |
| `validated-wasm-package` | 7 days | Validated NPM package |

### Caching Strategy

**Cargo Cache:**
- Key: `wasm-test`, `wasm-build-[target]`, `wasm-quality`
- Saves only on main branch
- Shared across jobs with different keys

**wasm-pack Cache:**
- Path: `~/.wasm-pack`
- Key: `wasm-pack-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}`
- Prevents re-downloading wasm-pack

### Performance Notes

**Build Time Estimates:**
- wasm-test: 2-3 minutes
- wasm-build (per target): 3-5 minutes
- wasm-bundle-size: 1-2 minutes
- wasm-node-example: 1 minute
- wasm-package-validation: 1 minute
- wasm-quality: 2-3 minutes

**Total WASM workflow:** ~8-10 minutes

**Cost Optimization:**
- Only runs on WASM-related changes
- No redundant builds (uses artifacts)
- Minimal dependencies (Node.js only for examples)
- wasm-pack cache reduces download time

---

## Rust Clippy

**File:** `.github/workflows/rust-clippy.yml`

CLIPPY is run via GitHub Actions integration (separate repository action).

### Triggers
- Push to any branch
- Pull requests

### What It Does
- Runs clippy with strict warning settings
- Posts suggestions as PR comments
- Integrated with GitHub's code scanning

### Notes
- Part of GitHub's official Rust actions
- Can be configured in `clippy.toml` or inline

---

## OSV Scanner

**File:** `.github/workflows/osv-scanner-pr.yml`

Scans dependencies for known security vulnerabilities.

### Triggers
- Pull requests
- Manual workflow dispatch

### What It Does
- Scans `Cargo.lock` and `package.json` for vulnerabilities
- Reports findings in PR
- Uses OSV (Open Source Vulnerabilities) database
- Requires no configuration

### Integration
- Comments on PRs with findings
- Links to vulnerability details
- Blocks merge if critical vulnerabilities found

---

## Local Development

### Building WASM Locally

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for web
cd crates/pjs-wasm
wasm-pack build --target web --release

# Build for Node.js
wasm-pack build --target nodejs --release

# Build for bundlers
wasm-pack build --target bundler --release
```

### Running Tests Locally

```bash
# All tests
cargo nextest run --workspace --all-features

# WASM-specific tests
cargo nextest run -p pjs-wasm
cargo nextest run -p pjs-domain

# With coverage
cargo llvm-cov --workspace --lcov --output-path lcov.info nextest
```

### Bundle Size Check

```bash
# Check bundle size locally
./.github/scripts/check-wasm-bundle-size.sh \
  crates/pjs-wasm/pkg/pjs_wasm_bg.wasm 200 80
```

### Running Examples

```bash
# Node.js example
cd crates/pjs-js-client/examples/browser-wasm
npm install
node example.js
```

---

## Troubleshooting

### WASM Build Fails

**Problem:** `wasm-pack build` fails with target error

**Solution:**
```bash
# Ensure wasm32 target is installed
rustup target add wasm32-unknown-unknown --toolchain nightly
```

**Problem:** Out of disk space during build

**Solution:**
```bash
# Clean old artifacts
cargo clean
rm -rf ~/.wasm-pack/
```

### Tests Timeout

**Problem:** Tests take > 45 minutes

**Solution:**
1. Check for infinite loops in new tests
2. Run individual test suites locally:
   ```bash
   cargo nextest run -p [crate_name]
   ```

### Coverage Missing

**Problem:** Codecov shows 0% coverage

**Solution:**
1. Verify `CODECOV_TOKEN` is set in GitHub Secrets
2. Check PR has write access to repo
3. Run locally: `cargo llvm-cov --lcov`

### PR Comment Not Appearing

**Problem:** Bundle size report not posted as PR comment

**Solution:**
1. Verify `permissions.contents: read` is set
2. Check Actions has write access to issues
3. Run locally to verify script output

---

## Performance Summary

### Total CI Time by Workflow

| Workflow | Duration | Cost |
|----------|----------|------|
| Rust Build | ~10 min | Medium |
| Rust Test | ~30 min | High |
| WASM Build | ~8 min | Low |
| Total | ~48 min | Medium-High |

### Optimization Opportunities

1. **Parallel Jobs:** All jobs run in parallel
2. **Caching:** Cargo cache + sccache reduce rebuild time
3. **Fail-fast:** Disabled to catch all errors
4. **Platform Skip:** jemalloc excluded on Windows
5. **Selective Triggers:** WASM workflow only on WASM changes

### CI Cost Estimate

Using GitHub Actions free tier (2000 minutes/month):
- **Typical PJS repo:** ~10 pushes/day = 480 pushes/month
- **Average run time:** 48 minutes
- **Monthly usage:** ~23,040 minutes (exceeds free tier)

**Recommendation:** Configure spending limits or upgrade to GitHub Actions Pro.

---

## Contributing

When adding new workflows:

1. **Follow naming:** `[language]-[purpose].yml`
2. **Set concurrency:** Cancel previous runs on new push
3. **Use caching:** Leverage Swatinem/rust-cache and sccache
4. **Timeout jobs:** Add `timeout-minutes` to all jobs
5. **Document:** Update this README with new workflow details
6. **Test locally:** Verify all steps work before committing
7. **Monitor time:** Keep individual jobs under 30 minutes

---

## References

- [GitHub Actions Documentation](https://docs.github.io/en/actions)
- [Rust GitHub Actions](https://github.com/dtolnay/rust-toolchain)
- [wasm-pack Documentation](https://rustwasm.github.io/docs/wasm-pack/)
- [sccache Documentation](https://github.com/mozilla/sccache)
- [Codecov Integration](https://about.codecov.io/)
- [PJS Project Documentation](../../SPECIFICATION.md)
