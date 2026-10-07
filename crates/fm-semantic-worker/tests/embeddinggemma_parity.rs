//! Explicit real-checkpoint comparison against Python CPU embeddings.
#![cfg(feature = "gemma-probe")]

use std::path::PathBuf;

use fm_semantic_worker::gemma_native::{
    GemmaMedia, GemmaNativeEncoder, GemmaNativeError, GemmaNativeFiles,
};
use fm_semantic_worker::gemma_probe::{GemmaTextProbe, GemmaTextTask};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    cases: Vec<Case>,
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned, verified checkpoint"]
fn unified_native_encoder_preserves_text_roles_and_dimensions() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let reference: Reference =
        serde_json::from_str(include_str!("embeddinggemma-reference-v1.json"))
            .expect("generated Python reference");
    for case in reference.cases {
        let task = match case.task.as_str() {
            "search" => GemmaTextTask::Search,
            "question" => GemmaTextTask::Question,
            "code" => GemmaTextTask::Code,
            "document" => GemmaTextTask::Document,
            other => panic!("unknown reference task: {other}"),
        };
        let encoder = GemmaNativeEncoder::open(
            &directory,
            case.dimensions,
            GemmaMedia {
                images: false,
                audio: false,
                video: false,
            },
        )
        .expect("native text encoder");
        let actual = encoder
            .encode_text(task, &case.text, case.title.as_deref())
            .expect("native inference");
        let cosine = actual
            .iter()
            .zip(&case.vector)
            .map(|(left, right)| f64::from(*left) * f64::from(*right))
            .sum::<f64>();
        assert!(
            cosine > 0.99999,
            "{} at {} dimensions diverges from Python: cosine {cosine}",
            case.task,
            case.dimensions
        );
    }
}

#[derive(Deserialize)]
struct Case {
    task: String,
    dimensions: usize,
    text: String,
    title: Option<String>,
    vector: Vec<f32>,
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned, verified checkpoint"]
fn cpu_text_roles_and_dimensions_match_pinned_python_reference() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR to the verified checkpoint"),
    );
    let reference: Reference =
        serde_json::from_str(include_str!("embeddinggemma-reference-v1.json"))
            .expect("generated Python reference");
    assert_eq!(
        reference.revision,
        "914f7f89142e33e77833254d9c9b90c3cef7303b"
    );

    for case in reference.cases {
        let task = match case.task.as_str() {
            "search" => GemmaTextTask::Search,
            "question" => GemmaTextTask::Question,
            "code" => GemmaTextTask::Code,
            "document" => GemmaTextTask::Document,
            other => panic!("unknown reference task: {other}"),
        };
        let probe = GemmaTextProbe::open(&directory, case.dimensions).expect("native model loads");
        let actual = probe
            .encode(task, &case.text, case.title.as_deref())
            .expect("native inference");
        assert_eq!(actual.len(), case.vector.len());
        let cosine = actual
            .iter()
            .zip(&case.vector)
            .map(|(left, right)| f64::from(*left) * f64::from(*right))
            .sum::<f64>();
        assert!(
            cosine > 0.99999,
            "{} at {} dimensions diverges from Python: cosine {cosine}",
            case.task,
            case.dimensions
        );
    }
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned, verified checkpoint"]
fn cancelled_text_request_does_not_tokenize_or_infer() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let encoder = GemmaNativeEncoder::open(
        &directory,
        128,
        GemmaMedia {
            images: false,
            audio: false,
            video: false,
        },
    )
    .expect("native text model");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        encoder.encode_text_cancellable(GemmaTextTask::Search, "query", None, &cancellation),
        Err(GemmaNativeError::Cancelled)
    ));
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned, verified checkpoint"]
fn opens_original_checkpoint_files_from_independent_verified_locations() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let separate_config = tempfile::tempdir().expect("independent config location");
    let config = separate_config.path().join("config.json");
    std::fs::copy(directory.join("config.json"), &config).expect("copy small config");
    let files = GemmaNativeFiles {
        weights: directory.join("model.safetensors"),
        tokenizer: directory.join("tokenizer.json"),
        config,
        visual_processor: directory.join("processor_config.json"),
        audio_processor: directory.join("preprocessor_config.json"),
    };
    let encoder = GemmaNativeEncoder::open_files(
        &files,
        128,
        GemmaMedia {
            images: false,
            audio: false,
            video: false,
        },
    )
    .expect("load without co-located weights and config");
    assert_eq!(
        encoder
            .encode_text(GemmaTextTask::Search, "query", None)
            .expect("embed query")
            .len(),
        128
    );
}
