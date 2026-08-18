//! Build script for pjs-core.
//!
//! Translates the `simd-auto`/`simd-avx512` Cargo features into a single
//! `cfg(pjs_simd)` gate.
//!
//! A Cargo feature being enabled does not by itself mean rustc was invoked
//! with the matching `-C target-feature`; sonic-rs's own SIMD codegen still
//! depends on that. Cargo cannot pass `-C target-feature` from a build
//! script, so the user must compile with `RUSTFLAGS="-C target-cpu=native"`
//! (set in `.cargo/config.toml` at the workspace root by default). This
//! script checks that the requested target features are actually exposed to
//! rustc and warns when they are not.

use std::env;

fn main() {
    // Tell cargo about the custom cfg we may emit so `--check-cfg` does not warn
    // (required on nightly with `unexpected_cfgs` lint).
    println!("cargo::rustc-check-cfg=cfg(pjs_simd)");

    // Re-run only when these inputs change.
    println!("cargo::rerun-if-env-changed=CARGO_CFG_TARGET_FEATURE");
    println!("cargo::rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo::rerun-if-changed=build.rs");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let has = |feat: &str| target_features.split(',').any(|f| f == feat);

    let want_auto = env::var_os("CARGO_FEATURE_SIMD_AUTO").is_some();
    let want_avx512 = env::var_os("CARGO_FEATURE_SIMD_AVX512").is_some();

    if target_arch == "x86_64" {
        if want_avx512 && !has("avx512f") {
            println!(
                "cargo::warning=feature `simd-avx512` is enabled but rustc was not invoked \
                 with AVX-512 target features. Set RUSTFLAGS=\"-C target-cpu=native\" or \
                 `-C target-feature=+avx512f` in .cargo/config.toml. \
                 SIMD codegen in sonic-rs will fall back to scalar."
            );
        }
        if want_auto && !has("avx2") && !has("sse4.2") {
            println!(
                "cargo::warning=feature `simd-auto` is enabled but no x86 SIMD target \
                 features are exposed to rustc. The `pjs_simd` cfg this enables currently \
                 has no readers in pjs-core (its consumer was removed in #486/#488); \
                 `sonic-rs`'s own runtime SIMD dispatch, used unconditionally elsewhere \
                 in this crate, is unaffected either way."
            );
        }
    }

    // aarch64: NEON is mandatory in the AArch64 base ISA, so it is essentially always
    // present — no per-feature gate is needed, `simd-auto` alone covers it via the
    // umbrella `pjs_simd` cfg below.

    // Unsupported feature combinations on non-matching architectures: silently no-op.
    // sonic-rs already handles the runtime fallback.

    if want_auto || want_avx512 {
        println!("cargo::rustc-cfg=pjs_simd");
    }
}
