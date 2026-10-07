//! Checkpoint-pinned Transformers 5.19.0 audio feature extraction parity.
#![cfg(feature = "gemma-probe")]

#[allow(unreachable_pub)]
#[path = "../src/gemma_audio_features.rs"]
mod gemma_audio_features;

use std::path::PathBuf;

use gemma_audio_features::{GemmaAudioFeatureExtractor, GemmaAudioFeaturesError};
use serde::Deserialize;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    pcm: Vec<f32>,
    padded_samples: usize,
    frame_count: usize,
    valid_frames: usize,
    mask: Vec<bool>,
    features: Vec<f32>,
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn pcm_to_masked_mel_features_matches_python() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR to the verified checkpoint"),
    );
    let reference: Reference = serde_json::from_str(include_str!(
        "embeddinggemma-audio-features-reference-v1.json"
    ))
    .expect("generated Python reference");
    assert_eq!(
        reference.revision,
        "914f7f89142e33e77833254d9c9b90c3cef7303b"
    );
    let extractor = GemmaAudioFeatureExtractor::open(&directory).expect("pinned processor");
    for case in reference.cases {
        let actual = extractor
            .extract_pcm16k(&case.pcm, case.padded_samples)
            .expect("extract audio");
        assert_eq!(actual.frame_count, case.frame_count);
        assert_eq!(actual.valid_frames, case.valid_frames);
        assert_eq!(
            (0..actual.frame_count)
                .map(|index| index < actual.valid_frames)
                .collect::<Vec<_>>(),
            case.mask
        );
        assert_eq!(actual.features.len(), case.features.len());
        let max_error = actual
            .features
            .iter()
            .zip(&case.features)
            .map(|(native, python)| (native - python).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_error < 0.00001,
            "PCM {} samples: max feature error {max_error}",
            case.pcm.len()
        );
        eprintln!("PCM {} samples: max error {max_error}", case.pcm.len());
    }
    assert!(matches!(
        extractor.extract_pcm16k(&[f32::NAN; 320], 320),
        Err(GemmaAudioFeaturesError::NonFinite)
    ));
    assert!(matches!(
        extractor.extract_pcm16k(&[0.0; 160], 160),
        Err(GemmaAudioFeaturesError::InvalidLength)
    ));
    assert!(matches!(
        extractor.extract_pcm16k(&[0.0; 320], 480_001),
        Err(GemmaAudioFeaturesError::InvalidLength)
    ));
    assert!(matches!(
        extractor.extract_pcm16k(&[0.0; 320], 200),
        Err(GemmaAudioFeaturesError::InvalidLength)
    ));
}
