//! Build script for pjs-core.
//!
//! Translates `simd-*` Cargo features into source-level `cfg` gates so that
//! `#[target_feature]`-annotated hot paths and the sonic-rs dependency are
//! activated correctly for the current target.
//!
//! Cargo cannot pass `-C target-feature` from a build script to rustc.
//! To genuinely activate SIMD in sonic-rs and elsewhere, the user must
//! compile with `RUSTFLAGS="-C target-cpu=native"` (set in `.cargo/config.toml`
//! at the workspace root by default). This script verifies that intent and
//! warns when it is not satisfied.

use std::env;

fn main() {
    // Tell cargo about all custom cfgs we may emit so `--check-cfg` does not warn
    // (required on nightly with `unexpected_cfgs` lint).
    println!("cargo::rustc-check-cfg=cfg(pjs_simd_avx2)");
    println!("cargo::rustc-check-cfg=cfg(pjs_simd_avx512)");
    println!("cargo::rustc-check-cfg=cfg(pjs_simd_sse42)");
    println!("cargo::rustc-check-cfg=cfg(pjs_simd_neon)");
    println!("cargo::rustc-check-cfg=cfg(pjs_simd_auto)");

    // Re-run only when these inputs change.
    println!("cargo::rerun-if-env-changed=CARGO_CFG_TARGET_FEATURE");
    println!("cargo::rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo::rerun-if-changed=build.rs");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let has = |feat: &str| target_features.split(',').any(|f| f == feat);

    let want_auto = env::var_os("CARGO_FEATURE_SIMD_AUTO").is_some();
    let want_avx2 = env::var_os("CARGO_FEATURE_SIMD_AVX2").is_some();
    let want_avx512 = env::var_os("CARGO_FEATURE_SIMD_AVX512").is_some();
    let want_sse42 = env::var_os("CARGO_FEATURE_SIMD_SSE42").is_some();
    let want_neon = env::var_os("CARGO_FEATURE_SIMD_NEON").is_some();

    if want_auto {
        println!("cargo::rustc-cfg=pjs_simd_auto");
    }

    // x86_64 features: AVX-512 implies AVX2 implies SSE4.2.
    if target_arch == "x86_64" {
        let avx512_active = (want_avx512 || want_auto) && has("avx512f");
        let avx2_active = avx512_active || ((want_avx2 || want_auto) && has("avx2"));
        let sse42_active = avx2_active || ((want_sse42 || want_auto) && has("sse4.2"));

        if avx512_active {
            println!("cargo::rustc-cfg=pjs_simd_avx512");
        }
        if avx2_active {
            println!("cargo::rustc-cfg=pjs_simd_avx2");
        }
        if sse42_active {
            println!("cargo::rustc-cfg=pjs_simd_sse42");
        }

        if want_avx512 && !has("avx512f") {
            println!(
                "cargo::warning=feature `simd-avx512` is enabled but rustc was not invoked \
                 with AVX-512 target features. Set RUSTFLAGS=\"-C target-cpu=native\" or \
                 `-C target-feature=+avx512f` in .cargo/config.toml. \
                 SIMD codegen in sonic-rs will fall back to scalar."
            );
        }
        if want_avx2 && !has("avx2") {
            println!(
                "cargo::warning=feature `simd-avx2` is enabled but rustc target features \
                 lack `avx2`. Set RUSTFLAGS=\"-C target-cpu=native\" (recommended) or \
                 `-C target-feature=+avx2`. sonic-rs SIMD path inactive."
            );
        }
        if want_sse42 && !has("sse4.2") {
            println!(
                "cargo::warning=feature `simd-sse42` is enabled but rustc target features \
                 lack `sse4.2`. Set RUSTFLAGS=\"-C target-cpu=native\"."
            );
        }
        if want_auto && !has("avx2") && !has("sse4.2") {
            println!(
                "cargo::warning=feature `simd-auto` is enabled but no x86 SIMD target \
                 features are exposed to rustc. Add RUSTFLAGS=\"-C target-cpu=native\" in \
                 .cargo/config.toml so sonic-rs and SIMD hot paths activate."
            );
        }
    }

    // aarch64: NEON is mandatory in the AArch64 base ISA, so it is essentially always present.
    if target_arch == "aarch64" {
        let neon_active = (want_neon || want_auto) && has("neon");
        if neon_active {
            println!("cargo::rustc-cfg=pjs_simd_neon");
        }
        if want_neon && !has("neon") {
            println!(
                "cargo::warning=feature `simd-neon` is enabled but rustc target features \
                 lack `neon`. This is unusual on aarch64 — check your target triple."
            );
        }
    }

    // Unsupported feature combinations on non-matching architectures: silently no-op.
    // sonic-rs already handles the runtime fallback.
}
