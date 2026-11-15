//! Reference applier for the `StreamPatch` wire grammar, shared by the #624
//! acceptance tests. It works on the raw JSON form `[op, path, arg]` so the
//! tests do not depend on the Rust layout of `WirePatch`.
#![allow(dead_code)]

use serde_json::{Map, Value};

/// Longest path a patch may carry; mirrors `MAX_PATCH_PATH_DEPTH` in the spec.
pub const MAX_PATCH_PATH_DEPTH: usize = 128;

fn resolve<'a>(state: &'a mut Value, path: &[Value]) -> &'a mut Value {
    let mut node = state;
    for segment in path {
        node = match (node, segment) {
            (Value::Object(map), Value::String(key)) => map
                .get_mut(key)
                .unwrap_or_else(|| panic!("missing key {key}")),
            (Value::Array(items), Value::Number(index)) => {
                let index = usize::try_from(index.as_u64().expect("index")).unwrap();
                items
                    .get_mut(index)
                    .unwrap_or_else(|| panic!("missing index {index}"))
            }
            (node, segment) => panic!("cannot resolve {segment} in {node}"),
        };
    }
    node
}

/// Applies one wire patch (`["set"|"merge"|"append"|"append_str", path, arg]`).
pub fn apply_wire_patch(state: &mut Value, op: &Value) {
    let parts = op.as_array().expect("patch is a 3-array");
    assert_eq!(parts.len(), 3, "patch {op}");
    let kind = parts[0].as_str().expect("op name");
    let path = parts[1].as_array().expect("path array");
    assert!(
        path.len() <= MAX_PATCH_PATH_DEPTH,
        "path deeper than the cap"
    );
    let arg = &parts[2];
    match kind {
        "set" => match path.split_last() {
            None => *state = arg.clone(),
            Some((last, parent_path)) => match (resolve(state, parent_path), last) {
                (Value::Object(map), Value::String(key)) => {
                    map.insert(key.clone(), arg.clone());
                }
                (Value::Array(items), Value::Number(index)) => {
                    let index = usize::try_from(index.as_u64().unwrap()).unwrap();
                    assert!(index < items.len(), "set index out of bounds");
                    items[index] = arg.clone();
                }
                other => panic!("cannot set into {other:?}"),
            },
        },
        "merge" => {
            let entries: &Map<String, Value> = arg.as_object().expect("merge entries");
            let target = resolve(state, path)
                .as_object_mut()
                .expect("merge target is an object");
            target.extend(entries.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        "append" => resolve(state, path)
            .as_array_mut()
            .expect("append target is an array")
            .extend(arg.as_array().expect("append values").iter().cloned()),
        "append_str" => {
            let target = resolve(state, path);
            let current = target
                .as_str()
                .expect("append_str target is a string")
                .to_owned();
            *target = Value::String(current + arg.as_str().expect("append_str text"));
        }
        other => panic!("unknown op {other}"),
    }
}
