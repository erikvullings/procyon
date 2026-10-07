//! Complete native PCM-to-vector versus the pinned upstream audio model.
#![cfg(feature = "gemma-probe")]

use std::path::PathBuf;

use fm_semantic_worker::gemma_native::{GemmaMedia, GemmaNativeEncoder};
use serde::Deserialize;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    samples: usize,
    vector: Vec<f32>,
    pcm16_vector: Vec<f32>,
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn encoded_pcm_matches_upstream_multimodal_vector() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let reference: Reference = serde_json::from_str(include_str!(
        "embeddinggemma-audio-vector-reference-v1.json"
    ))
    .expect("generated Python reference");
    assert_eq!(
        reference.revision,
        "914f7f89142e33e77833254d9c9b90c3cef7303b"
    );
    let pcm: Vec<_> = (0..reference.samples)
        .map(|sample| {
            let t = sample as f32 / 16000.0;
            0.3 * (std::f32::consts::TAU * 440.0 * t).sin()
                + 0.1 * (std::f32::consts::TAU * 880.0 * t).sin()
        })
        .collect();
    let encoder = GemmaNativeEncoder::open(
        &directory,
        768,
        GemmaMedia {
            images: false,
            audio: true,
            video: false,
        },
    )
    .expect("native multimodal model");
    let actual = encoder.encode_audio_pcm16k(&pcm).expect("audio vector");
    let cosine: f64 = actual
        .iter()
        .zip(&reference.vector)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    assert!(cosine > 0.99999, "full audio embedding cosine {cosine}");

    let data_len = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&16_000_u32.to_le_bytes());
    wav.extend_from_slice(&32_000_u32.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in pcm {
        wav.extend_from_slice(&((sample * 32767.0).round() as i16).to_le_bytes());
    }
    let actual_file = encoder.encode_audio_file(&wav).expect("decoded WAV vector");
    let cosine: f64 = actual_file
        .iter()
        .zip(&reference.pcm16_vector)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    assert!(cosine > 0.99999, "decoded WAV embedding cosine {cosine}");
}
