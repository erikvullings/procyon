//! Offline, real-checkpoint audio tower parity against Transformers 5.19.0.
#![cfg(feature = "gemma-probe")]

#[allow(unreachable_pub)]
#[path = "../src/gemma_audio.rs"]
mod gemma_audio;

use std::path::PathBuf;

use gemma_audio::{GemmaAudioError, GemmaAudioTower};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    frame_count: usize,
    valid_frames: usize,
    features: Vec<f32>,
    output_mask: Vec<bool>,
    output: Vec<Vec<f32>>,
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn checkpoint_audio_soft_tokens_match_python_fp32() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR to the verified checkpoint"),
    );
    let reference: Reference =
        serde_json::from_str(include_str!("embeddinggemma-audio-reference-v1.json"))
            .expect("golden vectors");
    assert_eq!(
        reference.revision,
        "914f7f89142e33e77833254d9c9b90c3cef7303b"
    );
    let tower = GemmaAudioTower::open(&directory).expect("load native audio tower");
    assert!(matches!(
        tower.encode_features(&[0.0; 128], 0, 0),
        Err(GemmaAudioError::Features)
    ));
    assert!(matches!(
        tower.encode_features(&[0.0; 128], 1, 2),
        Err(GemmaAudioError::Features)
    ));
    assert!(matches!(
        tower.encode_features(&[0.0; 128], 3001, 1),
        Err(GemmaAudioError::Features)
    ));
    assert!(matches!(
        tower.encode_features(&[f32::NAN; 128], 1, 1),
        Err(GemmaAudioError::NonFinite)
    ));
    for case in reference.cases {
        let actual = tower
            .encode_features(&case.features, case.frame_count, case.valid_frames)
            .expect("encode masked features");
        let expected: Vec<_> = case
            .output
            .iter()
            .zip(&case.output_mask)
            .filter_map(|(token, valid)| valid.then_some(token))
            .collect();
        assert_eq!(actual.len(), expected.len());
        let mut max_error = 0.0_f32;
        for (index, (a, b)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(a.len(), 1536);
            let error = a
                .iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0_f32, f32::max);
            max_error = max_error.max(error);
            assert!(
                error < 0.0002,
                "audio token {index} ({}/{} frames) differs by {error}",
                case.valid_frames,
                case.frame_count
            );
        }
        eprintln!(
            "audio {} / {} frames: max absolute error {max_error}",
            case.valid_frames, case.frame_count
        );
    }
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn cancelled_audio_does_not_encode_features() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let tower = GemmaAudioTower::open(&directory).expect("load native audio tower");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        tower.encode_features_cancellable(&[0.0; 128], 1, 1, &cancellation),
        Err(GemmaAudioError::Cancelled)
    ));
}
