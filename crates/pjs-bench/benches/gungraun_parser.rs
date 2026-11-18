//! Instruction-count parser benchmarks (ADR-002, #736)
//!
//! Deterministic Callgrind counterpart of the `simple_throughput` parser group.
//! Valgrind runs on Linux only, so other targets compile an empty `main`.

#[cfg(target_os = "linux")]
mod linux {
    use gungraun::{library_benchmark, library_benchmark_group};
    use std::hint::black_box;

    const MEDIUM_JSON: &str = r#"{"user":{"id":12345,"name":"John Doe","email":"john.doe@example.com","status":"active","profile":{"bio":"Software engineer","location":"San Francisco","company":"TechCorp"},"posts":[{"id":1,"title":"Hello World","likes":25},{"id":2,"title":"Tech Tips","likes":42}]}}"#;

    fn large_json() -> String {
        let items: Vec<String> = (0..1000)
            .map(|i| {
                format!(
                    r#"{{"id":{i},"name":"Item {i}","description":"This is item number {i} with some content","price":{:.2},"category":"Category {}","active":{},"metadata":{{"created":"2024-01-01T10:30:00Z","tags":["tag1","tag2","tag3"]}}}}"#,
                    f64::from(i) * 1.5 + 10.0,
                    i % 10,
                    i % 2 == 0
                )
            })
            .collect();
        format!(r#"{{"data":[{}],"total":1000,"page":1}}"#, items.join(","))
    }

    #[library_benchmark]
    #[bench::medium(MEDIUM_JSON.to_owned())]
    #[bench::large(large_json())]
    fn serde_json_parse(json: String) -> serde_json::Value {
        black_box(serde_json::from_str(black_box(&json)).expect("valid JSON"))
    }

    #[library_benchmark]
    #[bench::medium(MEDIUM_JSON.to_owned())]
    #[bench::large(large_json())]
    fn sonic_rs_parse(json: String) -> sonic_rs::Value {
        black_box(sonic_rs::from_str(black_box(&json)).expect("valid JSON"))
    }

    library_benchmark_group!(
        name = parser,
        benchmarks = [serde_json_parse, sonic_rs_parse]
    );
}

#[cfg(target_os = "linux")]
use linux::parser;

#[cfg(target_os = "linux")]
gungraun::main!(library_benchmark_groups = parser);

#[cfg(not(target_os = "linux"))]
fn main() {}
