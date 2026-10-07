//! Real-checkpoint comparison of bidirectional text and injected media tokens.
#![cfg(feature = "gemma-probe")]

use std::path::PathBuf;

use fm_semantic_worker::gemma_fusion::GemmaFusionEncoder;
use serde::Deserialize;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    cases: Vec<Case>,
    long_video: LongVideo,
}

#[derive(Deserialize)]
struct LongVideo {
    ids: Vec<u32>,
    soft_token: Vec<f32>,
    vector: Vec<f32>,
}

#[derive(Deserialize)]
struct Case {
    modality: String,
    ids: Vec<u32>,
    position: usize,
    soft_token: Vec<f32>,
    vector: Vec<f32>,
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the verified checkpoint"]
fn image_and_audio_tokens_match_python_bidirectional_embeddings() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let reference: Reference =
        serde_json::from_str(include_str!("embeddinggemma-fusion-reference-v1.json"))
            .expect("generated reference");
    assert_eq!(
        reference.revision,
        "914f7f89142e33e77833254d9c9b90c3cef7303b"
    );
    let encoder = GemmaFusionEncoder::open(&directory).expect("load native encoder");
    for case in reference.cases {
        let actual = encoder
            .encode(&case.ids, &[(case.position, &case.soft_token)], 768)
            .expect("encode media token");
        let cosine = actual
            .iter()
            .zip(&case.vector)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum::<f64>();
        assert!(
            cosine > 0.99999,
            "{} embedding differs from Python: cosine {cosine}",
            case.modality
        );
    }
    let long = reference.long_video;
    let replacements: Vec<_> = long
        .ids
        .iter()
        .enumerate()
        .filter(|(_, id)| **id == 258884)
        .map(|(index, _)| (index, long.soft_token.as_slice()))
        .collect();
    let actual = encoder
        .encode(&long.ids, &replacements, 768)
        .expect("long video token sequence");
    let cosine: f64 = actual
        .iter()
        .zip(&long.vector)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    assert!(cosine > 0.99999, "long video fusion cosine {cosine}");
}
