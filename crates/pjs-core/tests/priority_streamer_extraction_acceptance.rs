//! Acceptance tests for #770 (track A): `PriorityStreamer::analyze` stops
//! re-cloning every subtree once per ancestor. A `Set` that an ancestor `Set`
//! of at least the same effective priority has already written is a no-op and
//! is not emitted; every kept patch reproduces the state the previous
//! algorithm reached at the same point. The previous algorithm is reproduced
//! below as the oracle.

use pjson_rs::domain::value_objects::{JsonPath, Priority};
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::patch::WirePatch;
use pjson_rs::stream::priority::{JsonPatch, PriorityStreamFrame, PriorityStreamer};
use pjson_rs_domain::services::{PriorityHeuristicConfig, ValueShape, compute_priority};
use serde_json::{Map, Value, json};

type Emitted = (WirePatch, Priority);

fn skeletonize_arrays(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, v)| (key.clone(), skeletonize_arrays(v)))
                .collect(),
        ),
        Value::Array(_) => Value::Array(vec![]),
        other => other.clone(),
    }
}

fn price(heuristic: &PriorityHeuristicConfig, path: &JsonPath, value: &Value) -> Priority {
    compute_priority(heuristic, path, ValueShape::from(value))
}

fn price_array(heuristic: &PriorityHeuristicConfig, path: &JsonPath, len: usize) -> Priority {
    compute_priority(heuristic, path, ValueShape::Array { len })
}

/// The pre-#770 `extract_patches`, unchanged apart from the output type.
fn oracle_extract(
    heuristic: &PriorityHeuristicConfig,
    json: &Value,
    current_path: &JsonPath,
    patches: &mut Vec<Emitted>,
) {
    match json {
        Value::Object(map) => {
            for (key, value) in map {
                let field_path = current_path.append_key(key);
                let own_priority = price(heuristic, &field_path, value);

                let mut child_patches = Vec::new();
                oracle_extract(heuristic, value, &field_path, &mut child_patches);

                let append_ceiling = child_patches
                    .iter()
                    .filter(|(patch, _)| matches!(patch, WirePatch::Append { .. }))
                    .map(|(_, priority)| *priority)
                    .max();
                let priority =
                    append_ceiling.map_or(own_priority, |ceiling| own_priority.max(ceiling));

                patches.push((
                    WirePatch::Set {
                        path: field_path.clone(),
                        value: skeletonize_arrays(value),
                    },
                    priority,
                ));
                patches.extend(child_patches);
            }
        }
        Value::Array(arr) => {
            if arr.len() > 10 {
                let priority = price_array(heuristic, current_path, arr.len());
                for chunk in arr.chunks(100) {
                    patches.push((
                        WirePatch::Append {
                            path: current_path.clone(),
                            values: chunk.to_vec(),
                        },
                        priority,
                    ));
                }
            } else if !arr.is_empty() {
                patches.push((
                    WirePatch::Append {
                        path: current_path.clone(),
                        values: arr.clone(),
                    },
                    price_array(heuristic, current_path, arr.len()),
                ));
            }
        }
        _ => {}
    }
}

/// The pre-#770 patch order: root `Set` for a scalar, then a stable sort by priority.
fn oracle_patches(document: &Value) -> Vec<Emitted> {
    let heuristic = PriorityHeuristicConfig::default();
    let mut patches = Vec::new();
    if !matches!(document, Value::Object(_) | Value::Array(_)) {
        patches.push((
            WirePatch::Set {
                path: JsonPath::root(),
                value: skeletonize_arrays(document),
            },
            Priority::CRITICAL,
        ));
    }
    oracle_extract(&heuristic, document, &JsonPath::root(), &mut patches);
    patches.sort_by_key(|(_, priority)| std::cmp::Reverse(*priority));
    patches
}

struct Plan {
    skeleton: Value,
    patches: Vec<Emitted>,
}

fn plan_of(document: &Value) -> Plan {
    let mut plan = PriorityStreamer::new().analyze(document).expect("plan");
    let mut skeleton = None;
    let mut patches = Vec::new();
    while let Some(frame) = plan.next_frame() {
        match frame {
            PriorityStreamFrame::Skeleton { data, .. } => skeleton = Some(data),
            PriorityStreamFrame::Patch { patches: batch, .. } => {
                patches.extend(batch.into_iter().map(|p| (p.patch, p.priority)))
            }
            PriorityStreamFrame::Complete { .. } => {}
        }
    }
    Plan {
        skeleton: skeleton.expect("every plan starts with a skeleton"),
        patches,
    }
}

/// State after the skeleton and after each single patch.
fn states(skeleton: &Value, patches: &[Emitted]) -> Vec<Value> {
    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(PriorityStreamFrame::Skeleton {
        data: skeleton.clone(),
        priority: Priority::CRITICAL,
        complete: false,
    });
    reconstructor.process_all_frames().expect("skeleton");
    let mut out = Vec::with_capacity(patches.len());
    for (patch, priority) in patches {
        reconstructor.add_frame(PriorityStreamFrame::Patch {
            patches: vec![JsonPatch {
                patch: patch.clone(),
                priority: *priority,
            }],
            priority: *priority,
        });
        reconstructor.process_all_frames().expect("patch applies");
        out.push(reconstructor.current_state().clone());
    }
    out
}

fn set_path(patch: &WirePatch) -> Option<&JsonPath> {
    match patch {
        WirePatch::Set { path, .. } => Some(path),
        _ => None,
    }
}

fn is_prefix(ancestor: &JsonPath, path: &JsonPath) -> bool {
    ancestor.depth() < path.depth() && ancestor.segments() == &path.segments()[..ancestor.depth()]
}

/// Indices into `old` of each element of `new`, matched in order; `None` when
/// `new` is not a subsequence of `old`.
fn subsequence_indices(old: &[Emitted], new: &[Emitted]) -> Option<Vec<usize>> {
    let mut indices = Vec::with_capacity(new.len());
    let mut cursor = 0;
    for entry in new {
        let offset = old[cursor..]
            .iter()
            .position(|candidate| candidate == entry)?;
        indices.push(cursor + offset);
        cursor += offset + 1;
    }
    Some(indices)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

const KEYS: &[&str] = &[
    "id",
    "state",
    "name",
    "description",
    "tags",
    "items",
    "analytics",
    "metadata_x",
    "created_at",
    "k",
    "payload",
    "user_stats",
];

fn random_value(rng: &mut Rng, depth: usize) -> Value {
    let choice = if depth >= 5 {
        rng.below(5)
    } else {
        rng.below(8)
    };
    match choice {
        0 => Value::Null,
        1 => json!(rng.below(1000)),
        2 => json!(rng.below(1000) as f64 + 0.5),
        3 => Value::String("x".repeat(rng.below(80) as usize)),
        4 => Value::Bool(rng.below(2) == 0),
        5 | 6 => {
            let mut map = Map::new();
            for _ in 0..=rng.below(4) {
                let key = KEYS[rng.below(KEYS.len() as u64) as usize];
                map.insert(key.to_string(), random_value(rng, depth + 1));
            }
            Value::Object(map)
        }
        _ => {
            let len = if rng.below(3) == 0 {
                11 + rng.below(25)
            } else {
                rng.below(6)
            };
            Value::Array((0..len).map(|_| random_value(rng, depth + 1)).collect())
        }
    }
}

fn random_documents() -> Vec<Value> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut documents = Vec::new();
    while documents.len() < 400 {
        let value = random_value(&mut rng, 0);
        if value.is_object() || value.is_array() {
            documents.push(value);
        }
    }
    documents
}

fn nested(keys: &[&str], leaf: Value) -> Value {
    keys.iter()
        .rev()
        .fold(leaf, |inner, key| json!({ *key: inner }))
}

fn set_node_count(patches: &[Emitted]) -> usize {
    fn nodes(value: &Value) -> usize {
        match value {
            Value::Object(map) => 1 + map.values().map(nodes).sum::<usize>(),
            Value::Array(items) => 1 + items.iter().map(nodes).sum::<usize>(),
            _ => 1,
        }
    }
    patches
        .iter()
        .filter_map(|(patch, _)| match patch {
            WirePatch::Set { value, .. } => Some(nodes(value)),
            _ => None,
        })
        .sum()
}

#[test]
fn analyze_emits_a_subsequence_of_the_previous_patch_list_without_dropping_appends() {
    for document in random_documents() {
        let old = oracle_patches(&document);
        let new = plan_of(&document).patches;

        let indices = subsequence_indices(&old, &new)
            .unwrap_or_else(|| panic!("not a subsequence of the old plan for {document}"));
        let kept: std::collections::HashSet<usize> = indices.into_iter().collect();
        for (index, (patch, _)) in old.iter().enumerate() {
            if !kept.contains(&index) {
                assert!(
                    matches!(patch, WirePatch::Set { .. }),
                    "an Append was dropped for {document}: {patch:?}"
                );
            }
        }
    }
}

#[test]
fn analyze_keeps_the_state_reached_after_every_kept_patch() {
    for document in random_documents() {
        let plan = plan_of(&document);
        let old = oracle_patches(&document);
        let indices = subsequence_indices(&old, &plan.patches).expect("subsequence");

        let old_states = states(&plan.skeleton, &old);
        let new_states = states(&plan.skeleton, &plan.patches);

        for (new_state, old_index) in new_states.iter().zip(indices) {
            assert_eq!(
                new_state, &old_states[old_index],
                "state diverges at old patch {old_index} for {document}"
            );
        }
        assert_eq!(new_states.last().unwrap_or(&plan.skeleton), &document);
    }
}

#[test]
fn analyze_emits_no_set_under_an_ancestor_set_of_at_least_its_priority() {
    for document in random_documents() {
        let patches = plan_of(&document).patches;
        for (patch, priority) in &patches {
            let Some(path) = set_path(patch) else {
                continue;
            };
            for (other, other_priority) in &patches {
                let Some(ancestor) = set_path(other) else {
                    continue;
                };
                if is_prefix(ancestor, path) {
                    assert!(
                        other_priority < priority,
                        "Set at {path:?} ({priority:?}) sits under Set at {ancestor:?} ({other_priority:?}) in {document}"
                    );
                }
            }
        }
    }
}

#[test]
fn analyze_clones_a_leaf_once_per_strictly_increasing_priority_step_on_its_chain() {
    // Adversarial chain: own priority rises from BACKGROUND to CRITICAL going down.
    let keys = ["analytics", "created_at", "payload", "description", "state"];
    let document = nested(&keys, json!({"marker": "needle-needle-needle"}));
    let old = oracle_patches(&document);

    let mut running_max = None;
    let mut steps = 0usize;
    let mut path = JsonPath::root();
    for key in keys.iter().chain(["marker"].iter()) {
        path = path.append_key(key);
        let effective = old
            .iter()
            .find(|(patch, _)| set_path(patch) == Some(&path))
            .map(|(_, priority)| *priority)
            .expect("the old plan sets every field");
        if running_max.is_none_or(|max| effective > max) {
            running_max = Some(effective);
            steps += 1;
        }
    }

    let new = plan_of(&document).patches;
    let sets_with_leaf = new
        .iter()
        .filter(|(patch, _)| match patch {
            WirePatch::Set { value, .. } => value.to_string().contains("needle"),
            _ => false,
        })
        .count();

    assert!(
        steps >= 2,
        "the chain must be adversarial, got {steps} steps"
    );
    assert!(
        sets_with_leaf <= steps,
        "leaf cloned {sets_with_leaf} times, bound is {steps}"
    );
}

#[test]
fn analyze_on_a_deep_document_emits_far_fewer_set_nodes_than_the_previous_plan() {
    let depth = 64;
    let keys = vec!["k"; depth];
    let document = nested(&keys, json!({"list": (0..10_000).collect::<Vec<_>>()}));

    let old = oracle_patches(&document);
    let new = plan_of(&document).patches;

    assert!(
        set_node_count(&new) * 4 <= set_node_count(&old),
        "new plan clones {} Set nodes, old plan {}",
        set_node_count(&new),
        set_node_count(&old)
    );
    let appends = |patches: &[Emitted]| {
        patches
            .iter()
            .filter(|(patch, _)| matches!(patch, WirePatch::Append { .. }))
            .count()
    };
    assert_eq!(appends(&new), appends(&old));
}

#[test]
fn analyze_keeps_every_top_level_set_of_a_flat_document() {
    let document = json!({"state": "ok", "name": "n", "version": 1, "analytics": 2});
    let old = oracle_patches(&document);

    assert_eq!(plan_of(&document).patches, old);
}

#[test]
fn analyze_prices_appends_by_their_depth() {
    let document = json!({
        "list": [1, 2, 3],
        "a": {"list": [1, 2, 3], "b": {"list": [1, 2, 3], "c": {"list": [1, 2, 3],
            "d": {"list": [1, 2, 3], "e": {"list": [1, 2, 3], "f": {"list": [1, 2, 3]}}}}}}
    });
    let patches = plan_of(&document).patches;

    let rows = [
        ("$.list", 70),
        ("$.a.list", 60),
        ("$.a.b.list", 50),
        ("$.a.b.c.list", 50),
        ("$.a.b.c.d.list", 50),
        ("$.a.b.c.d.e.list", 40),
        ("$.a.b.c.d.e.f.list", 40),
    ];
    for (target, expected) in rows {
        let target = JsonPath::new(target).expect("valid path");
        let found: Vec<Priority> = patches
            .iter()
            .filter(|(patch, _)| matches!(patch, WirePatch::Append { path, .. } if *path == target))
            .map(|(_, priority)| *priority)
            .collect();
        assert_eq!(
            found,
            vec![Priority::new(expected).unwrap()],
            "Append at {target:?}"
        );
    }
}

#[test]
fn analyze_frames_still_rebuild_a_deep_document_after_set_elimination() {
    let document = nested(
        &["k"; 200],
        json!({"list": (0..40).collect::<Vec<_>>(), "id": 1}),
    );
    let plan = plan_of(&document);

    let states = states(&plan.skeleton, &plan.patches);

    assert_eq!(states.last().expect("patches"), &document);
}
