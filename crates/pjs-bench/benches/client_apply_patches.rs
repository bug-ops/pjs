//! Benchmark for `ClientStreamSession::apply_patches` (#670).
//!
//! The fuzzing entry point it drives exists only under `--cfg pjs_bench`:
//!
//! ```text
//! RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench --bench client_apply_patches
//! ```
//!
//! Each case applies one frame worth of patches to a fresh session:
//! `set_keys` (one `Set` per top-level key), `merge_entries` (one root `Merge`),
//! `append_values` (one `Append` of scalars) and `append_str_chunks`.

#[cfg(pjs_bench)]
mod bench {
    use std::hint::black_box;

    use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group};
    use pjson_rs::infrastructure::websocket::{
        WirePatch, WirePath, WireSegment, fuzzing::ClientPatchHarness,
    };
    use serde_json::{Map, Value, json};

    const COUNTS: [usize; 3] = [100, 1_000, 10_000];
    const MAX_BYTES: usize = usize::MAX;

    fn key_path(key: String) -> WirePath {
        WirePath(vec![WireSegment::Key(key)])
    }

    fn entry(index: usize) -> (String, Value) {
        (
            format!("key-{index}"),
            json!({"id": index, "label": "item", "tags": ["a", "b"]}),
        )
    }

    fn set_keys(count: usize) -> Vec<WirePatch> {
        (0..count)
            .map(|index| {
                let (key, value) = entry(index);
                WirePatch::Set {
                    path: key_path(key),
                    value,
                }
            })
            .collect()
    }

    fn merge_entries(count: usize) -> Vec<WirePatch> {
        let entries: Map<String, Value> = (0..count).map(entry).collect();
        vec![WirePatch::Merge {
            path: WirePath(Vec::new()),
            entries,
        }]
    }

    fn append_values(count: usize) -> Vec<WirePatch> {
        let init = WirePatch::Set {
            path: key_path("list".into()),
            value: json!([]),
        };
        let values = (0..count).map(|index| json!(index)).collect();
        let append = WirePatch::Append {
            path: key_path("list".into()),
            values,
        };
        vec![init, append]
    }

    fn append_str_chunks(count: usize) -> Vec<WirePatch> {
        let init = WirePatch::Set {
            path: key_path("text".into()),
            value: json!(""),
        };
        let chunks = (0..count).map(|_| WirePatch::AppendStr {
            path: key_path("text".into()),
            text: "chunk ".into(),
        });
        std::iter::once(init).chain(chunks).collect()
    }

    fn apply_patches(c: &mut Criterion) {
        type Build = fn(usize) -> Vec<WirePatch>;
        let cases: [(&str, Build); 4] = [
            ("set_keys", set_keys),
            ("merge_entries", merge_entries),
            ("append_values", append_values),
            ("append_str_chunks", append_str_chunks),
        ];
        let mut group = c.benchmark_group("client_apply_patches");
        for (name, build) in cases {
            for count in COUNTS {
                let patches = build(count);
                group.throughput(Throughput::Elements(count as u64));
                group.bench_with_input(BenchmarkId::new(name, count), &patches, |b, patches| {
                    b.iter_batched(
                        || (ClientPatchHarness::new(), patches.clone()),
                        |(mut harness, patches)| {
                            harness
                                .apply_patches(patches, MAX_BYTES)
                                .expect("patches apply");
                            black_box(harness.tracked_bytes())
                        },
                        BatchSize::LargeInput,
                    );
                });
            }
        }
        group.finish();
    }

    criterion_group!(benches, apply_patches);
}

#[cfg(pjs_bench)]
criterion::criterion_main!(bench::benches);

#[cfg(not(pjs_bench))]
fn main() {
    println!(
        "client_apply_patches skipped: run RUSTFLAGS=\"-C target-cpu=native --cfg pjs_bench\" cargo bench -p pjs-bench --bench client_apply_patches"
    );
}
