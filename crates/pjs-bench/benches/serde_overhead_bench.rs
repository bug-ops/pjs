use chrono::Utc;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use pjson_rs_domain::events::DomainEvent;
use pjson_rs_domain::value_objects::{Priority, SessionId, StreamId};
use std::hint::black_box;
use uuid::Uuid;

fn benchmark_custom_serde_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("custom_serde_serialization");

    // Create test data
    let session_id = SessionId::new();
    let stream_id = StreamId::new();
    let priority = Priority::new(50).unwrap();

    // Benchmark SessionActivated event serialization
    let event = DomainEvent::SessionActivated {
        session_id: session_id.clone(),
        timestamp: Utc::now(),
    };

    group.throughput(Throughput::Elements(1));
    group.bench_function("SessionActivated", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&event)).unwrap();
            black_box(json);
        });
    });

    // Benchmark StreamCreated event serialization (has multiple custom serde fields)
    let event = DomainEvent::StreamCreated {
        stream_id: stream_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
    };

    group.bench_function("StreamCreated", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&event)).unwrap();
            black_box(json);
        });
    });

    // Benchmark batch serialization (simulate real-world event publishing)
    let events: Vec<DomainEvent> = (0..100)
        .map(|i| {
            if i % 3 == 0 {
                DomainEvent::SessionActivated {
                    session_id: SessionId::new(),
                    timestamp: Utc::now(),
                }
            } else if i % 3 == 1 {
                DomainEvent::StreamCreated {
                    stream_id: StreamId::new(),
                    session_id: SessionId::new(),
                    timestamp: Utc::now(),
                }
            } else {
                DomainEvent::StreamStarted {
                    stream_id: StreamId::new(),
                    session_id: SessionId::new(),
                    timestamp: Utc::now(),
                }
            }
        })
        .collect();

    group.throughput(Throughput::Elements(100));
    group.bench_function("batch_100_events", |b| {
        b.iter(|| {
            let results: Vec<String> = black_box(&events)
                .iter()
                .map(|e| serde_json::to_string(e).unwrap())
                .collect();
            black_box(results);
        });
    });

    group.finish();
}

fn benchmark_custom_serde_deserialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("custom_serde_deserialization");

    // Pre-serialize test data
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let session_activated_json = serde_json::to_string(&DomainEvent::SessionActivated {
        session_id: session_id.clone(),
        timestamp: Utc::now(),
    })
    .unwrap();

    let stream_created_json = serde_json::to_string(&DomainEvent::StreamCreated {
        stream_id: stream_id.clone(),
        session_id: session_id.clone(),
        timestamp: Utc::now(),
    })
    .unwrap();

    group.throughput(Throughput::Elements(1));
    group.bench_function("SessionActivated", |b| {
        b.iter(|| {
            let event: DomainEvent =
                serde_json::from_str(black_box(&session_activated_json)).unwrap();
            black_box(event);
        });
    });

    group.bench_function("StreamCreated", |b| {
        b.iter(|| {
            let event: DomainEvent = serde_json::from_str(black_box(&stream_created_json)).unwrap();
            black_box(event);
        });
    });

    // Batch deserialization
    let batch_json: Vec<String> = (0..100)
        .map(|i| {
            let event = if i % 2 == 0 {
                DomainEvent::SessionActivated {
                    session_id: SessionId::new(),
                    timestamp: Utc::now(),
                }
            } else {
                DomainEvent::StreamCreated {
                    stream_id: StreamId::new(),
                    session_id: SessionId::new(),
                    timestamp: Utc::now(),
                }
            };
            serde_json::to_string(&event).unwrap()
        })
        .collect();

    group.throughput(Throughput::Elements(100));
    group.bench_function("batch_100_events", |b| {
        b.iter(|| {
            let events: Vec<DomainEvent> = black_box(&batch_json)
                .iter()
                .map(|json| serde_json::from_str(json).unwrap())
                .collect();
            black_box(events);
        });
    });

    group.finish();
}

fn benchmark_value_object_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("value_object_creation");

    group.throughput(Throughput::Elements(1000));
    group.bench_function("Priority::new", |b| {
        b.iter(|| {
            let priorities: Vec<Priority> = (1..=100)
                .map(|i| Priority::new(i).unwrap())
                .collect();
            black_box(priorities);
        });
    });

    group.bench_function("SessionId::new", |b| {
        b.iter(|| {
            let ids: Vec<SessionId> = (0..100).map(|_| SessionId::new()).collect();
            black_box(ids);
        });
    });

    group.bench_function("StreamId::new", |b| {
        b.iter(|| {
            let ids: Vec<StreamId> = (0..100).map(|_| StreamId::new()).collect();
            black_box(ids);
        });
    });

    group.finish();
}

fn benchmark_uuid_string_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("uuid_conversion");

    let session_id = SessionId::new();
    let uuid = Uuid::new_v4();

    group.throughput(Throughput::Elements(1));
    group.bench_function("SessionId::as_uuid", |b| {
        b.iter(|| {
            let uuid = black_box(&session_id).as_uuid();
            black_box(uuid);
        });
    });

    group.bench_function("SessionId::from_uuid", |b| {
        b.iter(|| {
            let id = SessionId::from_uuid(black_box(uuid));
            black_box(id);
        });
    });

    group.bench_function("SessionId::to_string", |b| {
        b.iter(|| {
            let s = black_box(&session_id).to_string();
            black_box(s);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_custom_serde_serialization,
    benchmark_custom_serde_deserialization,
    benchmark_value_object_creation,
    benchmark_uuid_string_conversion
);
criterion_main!(benches);
