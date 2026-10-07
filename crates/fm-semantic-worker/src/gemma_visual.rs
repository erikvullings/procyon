//! Offline, bounded visual preprocessing for image and sampled video frames.

use std::fs::File;
use std::io::Cursor;
use std::path::Path;

use image::{ImageFormat, ImageReader, Limits, RgbImage};

const PATCH_SIDE: usize = 16;
const POOL_SIDE: usize = 3;
const PATCH_VALUES: usize = PATCH_SIDE * PATCH_SIDE * 3;
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE_PIXELS: u64 = 25_000_000;

/// Rejects checkpoints whose image/video preprocessing differs from this native path.
pub fn validate_processor_config(directory: &Path) -> Result<(), GemmaVisualError> {
    validate_processor_config_file(&directory.join("processor_config.json"))
}

/// Verify the pinned visual processor supplied as an independent original file.
pub fn validate_processor_config_file(path: &Path) -> Result<(), GemmaVisualError> {
    let config: serde_json::Value = serde_json::from_reader(File::open(path)?)?;
    for (name, limit, processor) in [
        ("image_processor", 280, "Gemma4ImageProcessor"),
        ("video_processor", 140, "EmbeddingGemma2VideoProcessor"),
    ] {
        let options = &config[name];
        let type_key = if name == "image_processor" {
            "image_processor_type"
        } else {
            "video_processor_type"
        };
        if options[type_key] != processor
            || options["max_soft_tokens"] != limit
            || options["patch_size"] != PATCH_SIDE
            || options["pooling_kernel_size"] != POOL_SIDE
            || options["resample"] != 3
            || options["do_convert_rgb"] != true
            || options["do_resize"] != true
            || options["do_rescale"] != true
            || options["do_normalize"] != false
            || options["image_mean"] != serde_json::json!([0.0, 0.0, 0.0])
            || options["image_std"] != serde_json::json!([1.0, 1.0, 1.0])
            || !options["rescale_factor"]
                .as_f64()
                .is_some_and(|factor| (factor - 1.0 / 255.0).abs() < 1e-12)
        {
            return Err(GemmaVisualError::ProcessorConfig);
        }
    }
    let video = &config["video_processor"];
    if video["fps"] != 1
        || video["max_frames"] != 32
        || video["overflow_strategy"] != "uniform"
        || video["add_timestamps"] != false
        || video["do_sample_frames"] != true
    {
        return Err(GemmaVisualError::ProcessorConfig);
    }
    Ok(())
}

/// The visual route determines the checkpoint's maximum soft-token count.
#[derive(Debug, Clone, Copy)]
pub enum VisualKind {
    /// One standalone image.
    Image,
    /// One already sampled frame in a video sequence.
    VideoFrame,
}

impl VisualKind {
    fn soft_token_limit(self) -> usize {
        match self {
            Self::Image => 280,
            Self::VideoFrame => 140,
        }
    }
}

/// Padded vision-tower input and its real patch count.
pub struct PreparedFrame {
    /// Channel-last 16x16 RGB patches, scaled to [0, 1].
    pub pixels: Vec<f32>,
    /// Image-grid positions in (x, y) order; padded positions are (-1, -1).
    pub positions: Vec<[i32; 2]>,
    /// Number of populated patches before padding.
    pub valid_patches: usize,
    /// Number of projected vision soft tokens.
    pub soft_tokens: usize,
}

/// Converts a PNG/JPEG image or previously sampled video frame into vision patches.
pub fn prepare_frame(encoded: &[u8], kind: VisualKind) -> Result<PreparedFrame, GemmaVisualError> {
    prepare_frame_cancellable(encoded, kind, || false)
}

/// Prepare an image while honoring cancellation during resize and patch extraction.
pub fn prepare_frame_cancellable(
    encoded: &[u8],
    kind: VisualKind,
    is_cancelled: impl Fn() -> bool,
) -> Result<PreparedFrame, GemmaVisualError> {
    if is_cancelled() {
        return Err(GemmaVisualError::Cancelled);
    }
    if encoded.is_empty() || encoded.len() > MAX_SOURCE_BYTES {
        return Err(GemmaVisualError::SourceTooLarge);
    }
    let reader = ImageReader::new(Cursor::new(encoded)).with_guessed_format()?;
    let Some(format @ (ImageFormat::Png | ImageFormat::Jpeg)) = reader.format() else {
        return Err(GemmaVisualError::UnsupportedFormat);
    };
    let (width, height) =
        ImageReader::with_format(Cursor::new(encoded), format).into_dimensions()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
        return Err(GemmaVisualError::SourceTooLarge);
    }
    let mut reader = ImageReader::with_format(Cursor::new(encoded), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(8_192);
    limits.max_image_height = Some(8_192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let source = reader.decode()?.to_rgb8();
    if is_cancelled() {
        return Err(GemmaVisualError::Cancelled);
    }
    let maximum = kind.soft_token_limit() * POOL_SIDE * POOL_SIDE;
    let (target_height, target_width) = resized_shape(height, width, maximum)?;
    let resized = if source.width() == target_width && source.height() == target_height {
        source
    } else {
        resize_bicubic(&source, target_width, target_height, &is_cancelled)?
    };
    let grid_width = target_width as usize / PATCH_SIDE;
    let grid_height = target_height as usize / PATCH_SIDE;
    let valid = grid_width * grid_height;
    let mut pixels = vec![0.0; maximum * PATCH_VALUES];
    let mut positions = vec![[-1; 2]; maximum];
    for patch_y in 0..grid_height {
        if is_cancelled() {
            return Err(GemmaVisualError::Cancelled);
        }
        for patch_x in 0..grid_width {
            let index = patch_y * grid_width + patch_x;
            positions[index] = [patch_x as i32, patch_y as i32];
            let target = &mut pixels[index * PATCH_VALUES..(index + 1) * PATCH_VALUES];
            for y in 0..PATCH_SIDE {
                for x in 0..PATCH_SIDE {
                    let sample = resized.get_pixel(
                        (patch_x * PATCH_SIDE + x) as u32,
                        (patch_y * PATCH_SIDE + y) as u32,
                    );
                    let offset = (y * PATCH_SIDE + x) * 3;
                    for channel in 0..3 {
                        target[offset + channel] = f32::from(sample[channel]) / 255.0;
                    }
                }
            }
        }
    }
    Ok(PreparedFrame {
        pixels,
        positions,
        valid_patches: valid,
        soft_tokens: valid / (POOL_SIDE * POOL_SIDE),
    })
}

fn cubic(distance: f64) -> f64 {
    let x = distance.abs();
    if x < 1.0 {
        1.5 * x * x * x - 2.5 * x * x + 1.0
    } else if x < 2.0 {
        -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
    } else {
        0.0
    }
}

fn weights(input: u32, output: u32) -> Vec<Vec<(usize, f64)>> {
    let ratio = f64::from(input) / f64::from(output);
    let support = ratio.max(1.0);
    (0..output)
        .map(|index| {
            let center = (f64::from(index) + 0.5) * ratio - 0.5;
            let low = (center - 2.0 * support).floor() as i64;
            let high = (center + 2.0 * support).ceil() as i64;
            let mut values: Vec<_> = (low..=high)
                .filter_map(|sample| {
                    let weight = cubic((center - sample as f64) / support);
                    (weight != 0.0 && sample >= 0 && sample < i64::from(input))
                        .then_some((sample as usize, weight))
                })
                .collect();
            let sum: f64 = values.iter().map(|(_, weight)| weight).sum();
            for (_, weight) in &mut values {
                *weight /= sum;
            }
            values
        })
        .collect()
}

fn resize_bicubic(
    source: &RgbImage,
    width: u32,
    height: u32,
    is_cancelled: &impl Fn() -> bool,
) -> Result<RgbImage, GemmaVisualError> {
    let output_width = width as usize;
    let output_height = height as usize;
    let vertical = weights(source.height(), height);
    let horizontal = weights(source.width(), width);
    // Torchvision v2 rounds/clamps each uint8 bicubic pass, with half values
    // rounded away from zero; ties-to-even changes downstream embeddings.
    let mut intermediate = vec![0_u8; output_width * source.height() as usize * 3];
    for (y, row) in intermediate.chunks_exact_mut(output_width * 3).enumerate() {
        if is_cancelled() {
            return Err(GemmaVisualError::Cancelled);
        }
        for (x, samples) in horizontal.iter().enumerate() {
            for channel in 0..3 {
                let sum: f64 = samples
                    .iter()
                    .map(|&(column, weight)| {
                        f64::from(source.get_pixel(column as u32, y as u32)[channel]) * weight
                    })
                    .sum();
                row[x * 3 + channel] = sum.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    let mut result = vec![0_u8; output_width * output_height * 3];
    for (y, row) in result.chunks_exact_mut(output_width * 3).enumerate() {
        if is_cancelled() {
            return Err(GemmaVisualError::Cancelled);
        }
        for x in 0..output_width {
            for channel in 0..3 {
                let sum: f64 = vertical[y]
                    .iter()
                    .map(|&(source_y, weight)| {
                        f64::from(intermediate[(source_y * output_width + x) * 3 + channel])
                            * weight
                    })
                    .sum();
                row[x * 3 + channel] = sum.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(RgbImage::from_raw(width, height, result).expect("validated image dimensions"))
}

fn resized_shape(
    height: u32,
    width: u32,
    max_patches: usize,
) -> Result<(u32, u32), GemmaVisualError> {
    let budget = (max_patches * PATCH_SIDE * PATCH_SIDE) as f64;
    let scale = (budget / (f64::from(height) * f64::from(width))).sqrt();
    let multiple = (PATCH_SIDE * POOL_SIDE) as u32;
    let side_limit = ((max_patches / (POOL_SIDE * POOL_SIDE)) as u32) * multiple;
    let mut h = ((scale * f64::from(height) / f64::from(multiple)).floor() as u32) * multiple;
    let mut w = ((scale * f64::from(width) / f64::from(multiple)).floor() as u32) * multiple;
    if h == 0 && w == 0 {
        return Err(GemmaVisualError::UnsupportedDimensions);
    }
    if h == 0 {
        h = multiple;
        w = (width / height).saturating_mul(multiple).min(side_limit);
    } else if w == 0 {
        w = multiple;
        h = (height / width).saturating_mul(multiple).min(side_limit);
    }
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > budget as u64 {
        return Err(GemmaVisualError::UnsupportedDimensions);
    }
    Ok((h, w))
}

/// Unsafe or unsupported visual input.
#[derive(Debug, thiserror::Error)]
pub enum GemmaVisualError {
    /// The owning job cancelled visual preprocessing.
    #[error("Gemma visual preprocessing cancelled")]
    Cancelled,
    /// Local processor configuration is incompatible with the native implementation.
    #[error("unsupported Gemma image/video processor configuration")]
    ProcessorConfig,
    /// Processor configuration is invalid JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Encoded image exceeds the input bound.
    #[error("visual input exceeds the configured size limit")]
    SourceTooLarge,
    /// Decoder format is not supported in this native path.
    #[error("visual input must be PNG or JPEG")]
    UnsupportedFormat,
    /// The image aspect ratio cannot fit the model's patch grid.
    #[error("visual dimensions do not fit the checkpoint patch budget")]
    UnsupportedDimensions,
    /// Decode failed.
    #[error(transparent)]
    Decode(#[from] image::ImageError),
    /// Format detection failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
