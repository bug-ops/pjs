#![no_main]

use libfuzzer_sys::fuzz_target;
use pjson_rs::{
    Error,
    infrastructure::websocket::{WirePatch, fuzzing::ClientPatchHarness},
};

const CAP_UNIT: usize = 16;
const HEAD_BYTES: usize = 3;
const WHOLE_BATCH: u8 = 1;

fn assert_within_cap(harness: &ClientPatchHarness, max_bytes: usize) {
    let tracked = harness.tracked_bytes();
    assert!(
        tracked <= max_bytes,
        "cap breached: {tracked} > {max_bytes}"
    );
}

fn assert_no_undercount(harness: &ClientPatchHarness) {
    let actual = serde_json::to_vec(harness.reconstructed())
        .expect("reconstructed data serializes")
        .len();
    let tracked = harness.tracked_bytes();
    assert!(tracked >= actual, "undercount: {tracked} < {actual}");
}

fn apply_one_by_one(patches: Vec<WirePatch>, max_bytes: usize) {
    let mut harness = ClientPatchHarness::new();
    for patch in patches {
        let before = harness.reconstructed().clone();
        match harness.apply_patches(vec![patch], max_bytes) {
            Ok(()) => {
                assert_no_undercount(&harness);
                assert_within_cap(&harness, max_bytes);
            }
            Err(Error::LimitExceeded { .. }) => {
                assert_eq!(
                    harness.reconstructed(),
                    &before,
                    "refused patch mutated state"
                );
                return;
            }
            Err(_) => return,
        }
    }
}

// A failing patch may leave itself partly applied, so the undercount check
// holds after `Ok` and after a cap refusal (which applies nothing), not after
// other errors.
fn apply_whole_batch(patches: Vec<WirePatch>, max_bytes: usize) {
    let mut harness = ClientPatchHarness::new();
    match harness.apply_patches(patches, max_bytes) {
        Ok(()) | Err(Error::LimitExceeded { .. }) => assert_no_undercount(&harness),
        Err(_) => {}
    }
    assert_within_cap(&harness, max_bytes);
}

fuzz_target!(|data: &[u8]| {
    let Some((head, body)) = data.split_first_chunk::<HEAD_BYTES>() else {
        return;
    };
    let max_bytes = usize::from(u16::from_le_bytes([head[1], head[2]])) * CAP_UNIT;
    let Ok(patches) = serde_json::from_slice::<Vec<WirePatch>>(body) else {
        return;
    };

    if head[0] & WHOLE_BATCH == 0 {
        apply_one_by_one(patches, max_bytes);
    } else {
        apply_whole_batch(patches, max_bytes);
    }
});
