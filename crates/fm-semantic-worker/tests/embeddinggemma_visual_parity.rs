//! Deterministic native RGB patch inputs versus Gemma's processor.
#![cfg(feature = "gemma-probe")]

use fm_semantic_worker::gemma_visual::{
    GemmaVisualError, VisualKind, prepare_frame, validate_processor_config,
};
use image::{ImageBuffer, ImageFormat, Rgb};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    kind: String,
    width: u32,
    height: u32,
    soft_tokens: usize,
    valid_patches: usize,
    positions: Vec<[i32; 2]>,
    values: Vec<(usize, f32)>,
}

#[test]
fn incompatible_image_processor_is_rejected() {
    let directory = tempfile::tempdir().expect("temporary processor");
    std::fs::write(directory.path().join("processor_config.json"), "{}").expect("config");
    assert!(matches!(
        validate_processor_config(directory.path()),
        Err(GemmaVisualError::ProcessorConfig)
    ));
}

#[test]
fn image_and_video_patches_match_upstream_geometry_and_pixels() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("embeddinggemma-visual-reference-v1.json"))
            .expect("reference");
    for case in cases {
        let rgb = ImageBuffer::from_fn(case.width, case.height, |x, y| {
            Rgb([
                ((x * 2 + y) % 256) as u8,
                ((x + y * 2) % 256) as u8,
                ((x + y) % 256) as u8,
            ])
        });
        let mut bytes = std::io::Cursor::new(Vec::new());
        rgb.write_to(&mut bytes, ImageFormat::Png).expect("PNG");
        let kind = match case.kind.as_str() {
            "image" => VisualKind::Image,
            "video" => VisualKind::VideoFrame,
            other => panic!("unexpected input kind {other}"),
        };
        let prepared = prepare_frame(bytes.get_ref(), kind).expect("prepare frame");
        assert_eq!(prepared.soft_tokens, case.soft_tokens);
        assert_eq!(prepared.valid_patches, case.valid_patches);
        for (position, expected) in [0, 1, 50, 500, prepared.positions.len() - 1]
            .into_iter()
            .zip(case.positions)
        {
            assert_eq!(prepared.positions[position], expected);
        }
        let differences: Vec<_> = case
            .values
            .iter()
            .filter(|(index, value)| (prepared.pixels[*index] - value).abs() >= 1.0 / 255.0 + 1e-6)
            .collect();
        assert!(
            differences.is_empty(),
            "{} {}x{}: {} pixels differ, first: {:?}",
            case.kind,
            case.width,
            case.height,
            differences.len(),
            differences
                .first()
                .map(|(index, value)| (index, prepared.pixels[*index], value,))
        );
    }
}
