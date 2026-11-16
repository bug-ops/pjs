#![no_main]

use libfuzzer_sys::fuzz_target;
use pjson_rs::{
    Error,
    infrastructure::websocket::{WirePatch, fuzzing::ClientPatchHarness},
};

const CAP_UNIT: usize = 16;

fuzz_target!(|data: &[u8]| {
    let Some((cap, body)) = data.split_first_chunk::<2>() else {
        return;
    };
    let max_bytes = usize::from(u16::from_le_bytes(*cap)) * CAP_UNIT;
    let Ok(patches) = serde_json::from_slice::<Vec<WirePatch>>(body) else {
        return;
    };

    let mut harness = ClientPatchHarness::new();
    for patch in patches {
        let before = harness.reconstructed().clone();
        match harness.apply_patches(vec![patch], max_bytes) {
            Ok(()) => {
                let actual = serde_json::to_vec(harness.reconstructed())
                    .expect("reconstructed data serializes")
                    .len();
                let tracked = harness.tracked_bytes();
                assert!(tracked >= actual, "undercount: {tracked} < {actual}");
                assert!(
                    tracked <= max_bytes,
                    "cap breached: {tracked} > {max_bytes}"
                );
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
});
