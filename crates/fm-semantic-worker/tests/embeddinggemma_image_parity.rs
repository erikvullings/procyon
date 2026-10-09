//! Full offline RGB-to-embedding comparison against pinned upstream inference.
#![cfg(feature = "gemma-probe")]

use std::path::PathBuf;

use fm_metadata::VideoSamplingError;
use fm_semantic_worker::gemma_audio::GemmaAudioError;
use fm_semantic_worker::gemma_audio_decode::AudioDecodeError;
use fm_semantic_worker::gemma_audio_features::GemmaAudioFeaturesError;
use fm_semantic_worker::gemma_fusion::GemmaFusionEncoder;
use fm_semantic_worker::gemma_fusion::GemmaFusionError;
use fm_semantic_worker::gemma_multimodal::{GemmaModality, GemmaProjection};
use fm_semantic_worker::gemma_native::{GemmaMedia, GemmaNativeEncoder, GemmaNativeError};
use fm_semantic_worker::gemma_vision::{GemmaVisionError, GemmaVisionTower};
use fm_semantic_worker::gemma_visual::{GemmaVisualError, VisualKind, prepare_frame};
use image::{ImageBuffer, ImageFormat, Rgb};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    ids: Vec<u32>,
    vector: Vec<f32>,
    soft_tokens: Option<Vec<Vec<f32>>>,
}

fn patterned_png() -> Vec<u8> {
    let image = ImageBuffer::from_fn(128, 96, |x, y| {
        Rgb([
            ((x * 2 + y) % 256) as u8,
            ((x + y * 2) % 256) as u8,
            ((x + y) % 256) as u8,
        ])
    });
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, ImageFormat::Png).expect("PNG");
    png.into_inner()
}

#[test]
fn all_native_stages_report_a_single_cancellation_error() {
    assert!(matches!(
        GemmaNativeError::from(VideoSamplingError::Cancelled),
        GemmaNativeError::Cancelled
    ));
    assert!(matches!(
        GemmaNativeError::from(GemmaVisualError::Cancelled),
        GemmaNativeError::Cancelled
    ));
    assert!(matches!(
        GemmaNativeError::from(GemmaVisionError::Cancelled),
        GemmaNativeError::Cancelled
    ));
    assert!(matches!(
        GemmaNativeError::from(AudioDecodeError::Cancelled),
        GemmaNativeError::Cancelled
    ));
    assert!(matches!(
        GemmaNativeError::from(GemmaAudioFeaturesError::Cancelled),
        GemmaNativeError::Cancelled
    ));
    assert!(matches!(
        GemmaNativeError::from(GemmaAudioError::Cancelled),
        GemmaNativeError::Cancelled
    ));
    assert!(matches!(
        GemmaNativeError::from(GemmaFusionError::Cancelled),
        GemmaNativeError::Cancelled
    ));
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn encoded_image_and_video_frame_match_upstream_multimodal_vectors() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let png = patterned_png();
    let encoder = GemmaNativeEncoder::open(
        &directory,
        768,
        GemmaMedia {
            images: true,
            audio: false,
            video: true,
        },
    )
    .expect("native multimodal model");
    for (video_frames, fixture) in [
        (0, include_str!("embeddinggemma-image-reference-v1.json")),
        (1, include_str!("embeddinggemma-video-reference-v1.json")),
        (
            2,
            include_str!("embeddinggemma-video-two-frames-reference-v1.json"),
        ),
    ] {
        let reference: Reference = serde_json::from_str(fixture).expect("Python reference");
        assert_eq!(
            reference.revision,
            "914f7f89142e33e77833254d9c9b90c3cef7303b"
        );
        let actual = if video_frames == 0 {
            encoder.encode_image(&png).expect("image vector")
        } else {
            let frames: Vec<_> = (0..video_frames)
                .map(|second| (png.as_slice(), second * 1_000))
                .collect();
            encoder.encode_video_frames(&frames).expect("video vector")
        };
        let cosine: f64 = actual
            .iter()
            .zip(&reference.vector)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
        eprintln!("video_frames={video_frames} full embedding cosine {cosine}");
        assert!(
            cosine > 0.99999,
            "video_frames={video_frames} embedding differs from Python: cosine {cosine}"
        );
    }
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn video_soft_tokens_and_fusion_match_upstream() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let reference: Reference =
        serde_json::from_str(include_str!("embeddinggemma-video-reference-v1.json"))
            .expect("Python video reference");
    let expected = reference
        .soft_tokens
        .expect("upstream projected video tokens");
    let frame = prepare_frame(&patterned_png(), VisualKind::VideoFrame).expect("video frame");
    let tower = GemmaVisionTower::open(&directory).expect("vision tower");
    let projection = GemmaProjection::open(&directory, GemmaModality::Vision).expect("projection");
    let actual: Vec<_> = tower
        .encode_patches(&frame.pixels, &frame.positions, frame.valid_patches)
        .expect("vision tokens")
        .iter()
        .map(|token| projection.project(token).expect("projected token"))
        .collect();
    assert_eq!(actual.len(), expected.len());
    let max_delta = actual
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max_delta < 0.02,
        "video projected token max absolute error {max_delta}"
    );

    let media_id = reference.ids[2];
    let replacements: Vec<_> = reference
        .ids
        .iter()
        .enumerate()
        .filter(|(_, id)| **id == media_id)
        .zip(&expected)
        .map(|((position, _), token)| (position, token.as_slice()))
        .collect();
    let language = GemmaFusionEncoder::open(&directory).expect("language model");
    let vector = language
        .encode(&reference.ids, &replacements, 768)
        .expect("fuse upstream soft tokens");
    let cosine: f64 = vector
        .iter()
        .zip(&reference.vector)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    eprintln!("video upstream-token fusion cosine {cosine}");
    assert!(cosine > 0.99999, "video fusion cosine {cosine}");
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn h264_video_file_retains_frame_timestamps() {
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
            video: true,
        },
    )
    .expect("native video encoder");
    let embedding = encoder
        .encode_h264_video(include_bytes!("fixtures/gemma-video-2s.mp4"))
        .expect("decode and embed MP4");
    assert_eq!(embedding.timestamps_ms, [0, 1000]);
    assert_eq!(embedding.vector.len(), 128);
    let norm: f64 = embedding
        .vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn cancelled_image_request_does_not_decode_or_infer() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let encoder = GemmaNativeEncoder::open(
        &directory,
        768,
        GemmaMedia {
            images: true,
            audio: false,
            video: false,
        },
    )
    .expect("native image encoder");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        encoder.encode_image_cancellable(&patterned_png(), &cancellation),
        Err(GemmaNativeError::Cancelled)
    ));
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn cancelled_video_request_does_not_sample_or_infer() {
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
            video: true,
        },
    )
    .expect("native video encoder");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        encoder.encode_h264_video_cancellable(
            include_bytes!("fixtures/gemma-video-2s.mp4"),
            &cancellation
        ),
        Err(GemmaNativeError::Cancelled)
    ));
}
