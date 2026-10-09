//! Pinned upstream vision-tower parity; explicitly opt in to the local checkpoint.
#![cfg(feature = "gemma-probe")]

#[allow(dead_code)]
#[path = "../src/gemma_compute.rs"]
mod gemma_compute;
#[allow(dead_code, unreachable_pub)]
#[path = "../src/gemma_vision.rs"]
mod gemma_vision;

use std::path::PathBuf;

use gemma_vision::GemmaVisionTower;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct Reference {
    revision: String,
    transformers: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    frame: usize,
    output: Vec<Vec<f32>>,
}

fn patches(frame: usize) -> (Vec<f32>, Vec<[i32; 2]>) {
    let width = if frame == 3 { 6 } else { 3 };
    let mut pixels = Vec::with_capacity(3 * width * 768);
    for py in 0..3 {
        for px in 0..width {
            for y in 0..16 {
                for x in 0..16 {
                    for channel in 0..3 {
                        let v = ((px * 16 + x) * 7 + (py * 16 + y) * 3 + channel * 53 + frame * 29)
                            % 256;
                        pixels.push(v as f32 / 255.0);
                    }
                }
            }
        }
    }
    let positions = (0..3)
        .flat_map(|y| (0..width).map(move |x| [x as i32, y]))
        .collect();
    (pixels, positions)
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn native_vision_matches_upstream_real_image_and_video_frame() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR to pinned local checkpoint"),
    );
    let reference: Reference =
        serde_json::from_str(include_str!("embeddinggemma-vision-reference-v1.json"))
            .expect("generated Python vision reference");
    assert_eq!(
        reference.revision,
        "914f7f89142e33e77833254d9c9b90c3cef7303b"
    );
    assert_eq!(reference.transformers, "5.19.0");
    let tower = GemmaVisionTower::open(&directory).expect("load real vision tower");
    for case in reference.cases {
        let (mut pixels, mut positions) = patches(case.frame);
        let valid = positions.len();
        if case.frame == 2 {
            pixels.extend(vec![0.0; pixels.len()]);
            positions.extend(vec![[-1, -1]; valid]);
        }

        let actual = tower
            .encode_patches(&pixels, &positions, valid)
            .expect("encode frame");
        assert_eq!(actual.len(), case.output.len());
        for (actual, expected) in actual.iter().zip(case.output) {
            let max_error = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            println!("frame {} max absolute error {max_error}", case.frame);
            assert!(
                max_error < 0.05,
                "frame {} max error {max_error}",
                case.frame
            );
        }
    }
}

#[test]
#[ignore = "requires PROCYON_GEMMA_PROBE_MODEL_DIR with the pinned checkpoint"]
fn cancelled_vision_does_not_encode_a_frame() {
    let directory = PathBuf::from(
        std::env::var_os("PROCYON_GEMMA_PROBE_MODEL_DIR")
            .expect("set PROCYON_GEMMA_PROBE_MODEL_DIR"),
    );
    let tower = GemmaVisionTower::open(&directory).expect("load vision tower");
    let (pixels, positions) = patches(0);
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        tower.encode_patches_cancellable(&pixels, &positions, positions.len(), &cancellation),
        Err(gemma_vision::GemmaVisionError::Cancelled)
    ));
}
