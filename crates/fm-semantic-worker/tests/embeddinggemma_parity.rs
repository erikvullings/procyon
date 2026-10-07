//! Explicit real-checkpoint comparison against Python CPU embeddings.
#![cfg(feature = "gemma-probe")]

use std::path::PathBuf;

use fm_semantic_worker::gemma_probe::{GemmaTextProbe, GemmaTextTask};
use serde::Deserialize;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    cases: Vec<Case>,
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
