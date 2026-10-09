//! Real-checkpoint parity for the native multimodal projection stage.
#![cfg(feature = "gemma-probe")]

use std::path::PathBuf;

use fm_semantic_worker::gemma_multimodal::{GemmaModality, GemmaProjection};
use serde::Deserialize;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    modality: String,
    input: Vec<f32>,
    output: Vec<f32>,
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned, verified checkpoint"]
fn native_vision_and_audio_projections_match_python() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR to the verified checkpoint"),
    );
    let reference: Reference =
        serde_json::from_str(include_str!("embeddinggemma-multimodal-reference-v1.json"))
            .expect("generated reference");
    assert_eq!(
        reference.revision,
        "914f7f89142e33e77833254d9c9b90c3cef7303b"
    );
    for case in reference.cases {
        let modality = match case.modality.as_str() {
            "vision" => GemmaModality::Vision,
            "audio" => GemmaModality::Audio,
            other => panic!("unknown modality {other}"),
        };
        let projection = GemmaProjection::open(&directory, modality).expect("load projection");
        let output = projection.project(&case.input).expect("project soft token");
        assert_eq!(output.len(), case.output.len());
        let error = output
            .iter()
            .zip(&case.output)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            error < 1e-4,
            "{} projection differs from Python by {error}",
            case.modality
        );
    }
}
