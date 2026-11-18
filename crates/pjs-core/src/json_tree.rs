//! Path resolution over `serde_json::Value`, shared by patch application in the
//! stream reconstructor and the WebSocket protocol and client.

use crate::{domain::PathSegment, error::PatchError};
use serde_json::Value;

/// Follows `path` from `state` and returns the node it addresses.
///
/// # Errors
///
/// [`PatchError::KeyNotFound`] / [`PatchError::IndexOutOfBounds`] when a step
/// is missing, [`PatchError::ExpectedObject`] / [`PatchError::ExpectedArray`]
/// when a step meets the wrong kind of container.
pub(crate) fn resolve_mut<'a>(
    state: &'a mut Value,
    path: &[PathSegment],
) -> Result<&'a mut Value, PatchError> {
    let mut node = state;
    for segment in path {
        node = match (node, segment) {
            (Value::Object(map), PathSegment::Key(key)) => map
                .get_mut(key)
                .ok_or_else(|| PatchError::KeyNotFound(key.clone()))?,
            (Value::Array(items), PathSegment::Index(index)) => {
                let len = items.len();
                items
                    .get_mut(*index)
                    .ok_or(PatchError::IndexOutOfBounds { index: *index, len })?
            }
            (_, PathSegment::Key(_)) => return Err(PatchError::ExpectedObject),
            (_, PathSegment::Index(_)) => return Err(PatchError::ExpectedArray),
        };
    }
    Ok(node)
}

/// Read-only counterpart of [`resolve_mut`]; `None` when any step does not
/// resolve.
#[cfg(any(test, feature = "websocket-client"))]
pub(crate) fn resolve<'a>(state: &'a Value, path: &[PathSegment]) -> Option<&'a Value> {
    path.iter()
        .try_fold(state, |node, segment| match (node, segment) {
            (Value::Object(map), PathSegment::Key(key)) => map.get(key),
            (Value::Array(items), PathSegment::Index(index)) => items.get(*index),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(k: &str) -> PathSegment {
        PathSegment::Key(k.to_owned())
    }

    #[test]
    fn resolve_mut_reports_each_failure_kind() {
        let mut state = json!({"a": [1]});
        assert_eq!(
            resolve_mut(&mut state, &[key("a"), PathSegment::Index(0)]).unwrap(),
            &json!(1)
        );
        assert_eq!(
            resolve_mut(&mut state, &[key("b")]).unwrap_err(),
            PatchError::KeyNotFound("b".to_owned())
        );
        assert_eq!(
            resolve_mut(&mut state, &[key("a"), PathSegment::Index(3)]).unwrap_err(),
            PatchError::IndexOutOfBounds { index: 3, len: 1 }
        );
        assert_eq!(
            resolve_mut(&mut state, &[PathSegment::Index(0)]).unwrap_err(),
            PatchError::ExpectedArray
        );
        assert_eq!(
            resolve_mut(&mut state, &[key("a"), key("x")]).unwrap_err(),
            PatchError::ExpectedObject
        );
    }

    #[test]
    fn resolve_follows_the_same_steps_read_only() {
        let state = json!({"a": [1]});
        assert_eq!(
            resolve(&state, &[key("a"), PathSegment::Index(0)]),
            Some(&json!(1))
        );
        assert_eq!(resolve(&state, &[key("b")]), None);
        assert_eq!(resolve(&state, &[PathSegment::Index(0)]), None);
    }
}
