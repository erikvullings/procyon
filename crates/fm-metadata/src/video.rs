//! Thumbnail extraction and bounded frame sampling for H.264-in-MP4/MOV video.
//! Pure-Rust demuxing (`mp4`) plus a from-source-compiled H.264
//! decoder (`openh264`, BSD-2-Clause, no runtime external tool - compiled
//! into the binary the same way `rars`/`sevenz-rust2` already are for
//! archive support) rather than shelling out to `ffmpeg`.
//!
//! Thumbnails use the first keyframe; semantic sampling decodes inter-frames
//! uniformly across longer clips. Other codecs (VP9, HEVC, AV1) and non-ISO-BMFF
//! containers (MKV, WebM, AVI) are reported as [`ThumbnailError::UnsupportedFormat`]
//! rather than half-implemented - the same "report false" convention as
//! every other thumbnail format here.

// The sampling API stays private until its caller is integrated in lib.rs.
#![allow(dead_code)]

use std::io::Cursor;

use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

use crate::thumbnail::{GeneratedThumbnail, MAX_SOURCE_BYTES, ThumbnailError, ThumbnailSize};

/// Container extensions this module will attempt to demux (without the
/// leading dot, case-insensitive). Actual success additionally requires an
/// H.264 video track - see the module docs.
pub const SUPPORTED_VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov"];

/// Whether `extension` is a container [`generate_video_thumbnail`] will
/// attempt to open.
pub fn is_supported_video_extension(extension: &str) -> bool {
    let lower = extension.to_ascii_lowercase();
    SUPPORTED_VIDEO_EXTENSIONS.contains(&lower.as_str())
}

/// Converts one AVCC (length-prefixed) sample into an Annex-B bitstream,
/// prefixing it with the track's SPS/PPS (required by openh264 to decode a
/// standalone frame - MP4 stores them once in the container header, not per
/// sample). Simplified from `openh264`'s own `examples/mp4/
/// mp4_bitstream_converter.rs`: that example tracks SPS/PPS-seen state
/// across an entire stream to avoid repeating them on every sample; since
/// this only ever decodes one keyframe, unconditionally prepending them is
/// simpler and equally correct (a decoder tolerates redundant parameter
/// sets).
fn avcc_sample_to_annex_b(
    sample: &[u8],
    length_size: u8,
    sps: &[Vec<u8>],
    pps: &[Vec<u8>],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(sample.len() + 32);
    for unit in sps.iter().chain(pps.iter()) {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unit);
    }
    let mut stream = sample;
    let length_size = length_size as usize;
    while stream.len() > length_size {
        let mut nal_size: u32 = 0;
        for byte in &stream[..length_size] {
            nal_size = (nal_size << 8) | u32::from(*byte);
        }
        stream = &stream[length_size..];
        let nal_size = nal_size as usize;
        if nal_size == 0 || nal_size > stream.len() {
            break;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&stream[..nal_size]);
        stream = &stream[nal_size..];
    }
    out
}

/// Decodes the first keyframe of the first H.264 track in an MP4/MOV
/// container and downscales it the same way [`crate::generate_image_thumbnail`]
/// does for a plain image.
pub fn generate_video_thumbnail(
    bytes: &[u8],
    size: ThumbnailSize,
) -> Result<GeneratedThumbnail, ThumbnailError> {
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(ThumbnailError::SourceTooLarge {
            size: bytes.len() as u64,
            limit: MAX_SOURCE_BYTES,
        });
    }

    let mut reader = mp4::Mp4Reader::read_header(Cursor::new(bytes), bytes.len() as u64)
        .map_err(|_| ThumbnailError::UnsupportedFormat)?;

    let track_id = reader
        .tracks()
        .values()
        .find(|track| matches!(track.media_type(), Ok(mp4::MediaType::H264)))
        .map(mp4::Mp4Track::track_id)
        .ok_or(ThumbnailError::UnsupportedFormat)?;
    let track = &reader.tracks()[&track_id];
    let avcc = &track
        .trak
        .mdia
        .minf
        .stbl
        .stsd
        .avc1
        .as_ref()
        .ok_or(ThumbnailError::UnsupportedFormat)?
        .avcc;
    let length_size = avcc.length_size_minus_one + 1;
    let sps: Vec<Vec<u8>> = avcc
        .sequence_parameter_sets
        .iter()
        .map(|unit| unit.bytes.clone())
        .collect();
    let pps: Vec<Vec<u8>> = avcc
        .picture_parameter_sets
        .iter()
        .map(|unit| unit.bytes.clone())
        .collect();
    if sps.is_empty() || pps.is_empty() {
        return Err(ThumbnailError::UnsupportedFormat);
    }

    let sample_count = reader
        .sample_count(track_id)
        .map_err(|_| ThumbnailError::UnsupportedFormat)?;

    let mut decoder = Decoder::new().map_err(|_| ThumbnailError::UnsupportedFormat)?;
    for sample_id in 1..=sample_count {
        let Ok(Some(sample)) = reader.read_sample(track_id, sample_id) else {
            continue;
        };
        if !sample.is_sync {
            continue;
        }
        let annex_b = avcc_sample_to_annex_b(&sample.bytes, length_size, &sps, &pps);
        let Ok(Some(decoded)) = decoder.decode(&annex_b) else {
            continue;
        };
        let (width, height) = decoded.dimensions();
        let mut rgb = vec![0_u8; width * height * 3];
        decoded.write_rgb8(&mut rgb);
        let image = image::RgbImage::from_raw(width as u32, height as u32, rgb)
            .ok_or(ThumbnailError::UnsupportedFormat)?;
        let thumbnail = image::DynamicImage::ImageRgb8(image)
            .thumbnail(size.max_dimension(), size.max_dimension());
        let mut out = Vec::new();
        thumbnail
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
            .map_err(ThumbnailError::Encode)?;
        return Ok(GeneratedThumbnail {
            bytes: out,
            content_type: "image/jpeg",
        });
    }
    Err(ThumbnailError::UnsupportedFormat)
}

const MAX_VIDEO_DURATION_MS: u128 = 600_000;
const MAX_VIDEO_FRAMES: usize = 32;
const MAX_DECODED_SAMPLES: u32 = 2048;
// H.264 1080p is commonly coded in 1920x1088 macroblocks then cropped.
const MAX_FRAME_PIXELS: u64 = 1920 * 1088;
const MAX_SAMPLE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ENCODED_FRAME_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_FRAME_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAME_EDGE: u32 = 768;

/// A sampled H.264 frame, suitable for bounded image preprocessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampledVideoFrame {
    /// Presentation timestamp, relative to the video track, in milliseconds.
    pub timestamp_ms: u64,
    /// Encoded JPEG bytes (at most 768 pixels on the longest edge).
    pub bytes: Vec<u8>,
    /// MIME type for the encoded frame.
    pub content_type: &'static str,
}

/// An unsupported video or a violated ingestion budget.
#[derive(Debug, thiserror::Error)]
pub enum VideoSamplingError {
    /// The source exceeds the same bounded-read limit as thumbnails.
    #[error("video exceeds {limit} source bytes ({size})")]
    SourceTooLarge {
        /// Actual bytes.
        size: u64,
        /// Maximum accepted bytes.
        limit: u64,
    },
    /// Only non-fragmented MP4/MOV with H.264 and no frame reordering is supported.
    #[error("unsupported video container, codec, or reordered frames")]
    UnsupportedFormat,
    /// Long media is not currently eligible for ingestion.
    #[error("video duration {duration_ms}ms exceeds {limit_ms}ms")]
    DurationTooLong {
        /// Declared video track duration.
        duration_ms: u128,
        /// Maximum supported duration.
        limit_ms: u128,
    },
    /// Track or decoded dimensions exceed the per-frame pixel budget.
    #[error("video frame exceeds {MAX_FRAME_PIXELS} pixels ({width}x{height})")]
    PixelBudgetExceeded {
        /// Video width.
        width: usize,
        /// Video height.
        height: usize,
    },
    /// The encoded sample or decoding work exceeds its budget.
    #[error("video decode or output budget exceeded")]
    BudgetExceeded,
    /// The supported stream could not be decoded.
    #[error("video frame decoding failed")]
    DecodeFailed,
    /// A decoded frame could not be encoded.
    #[error("video frame encoding failed: {0}")]
    Encode(#[from] image::ImageError),
}

fn within_pixel_budget(width: usize, height: usize) -> bool {
    width > 0
        && height > 0
        && (width as u64)
            .checked_mul(height as u64)
            .is_some_and(|pixels| pixels <= MAX_FRAME_PIXELS)
}

struct SpsBits {
    bytes: Vec<u8>,
    offset: usize,
}

impl SpsBits {
    fn new(nal: &[u8]) -> Self {
        let mut bytes = Vec::with_capacity(nal.len());
        let mut zeroes = 0;
        for &byte in nal {
            if zeroes >= 2 && byte == 3 {
                zeroes = 0;
                continue;
            }
            bytes.push(byte);
            zeroes = if byte == 0 { zeroes + 1 } else { 0 };
        }
        Self { bytes, offset: 0 }
    }

    fn bit(&mut self) -> Option<u32> {
        let byte = *self.bytes.get(self.offset / 8)?;
        let bit = (byte >> (7 - self.offset % 8)) & 1;
        self.offset += 1;
        Some(u32::from(bit))
    }

    fn bits(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Some(value)
    }

    fn unsigned(&mut self) -> Option<u32> {
        let mut leading = 0;
        while self.bit()? == 0 {
            leading += 1;
            if leading > 30 {
                return None;
            }
        }
        Some((1u32 << leading) - 1 + self.bits(leading)?)
    }

    fn signed(&mut self) -> Option<i32> {
        let value = self.unsigned()? as i32;
        Some(if value % 2 == 0 {
            -(value / 2)
        } else {
            (value + 1) / 2
        })
    }
}

// Parse coded dimensions from SPS before handing untrusted bytes to OpenH264.
// Cropping can reduce displayed dimensions but not decoder allocation size.
fn coded_sps_dimensions(sps: &[u8]) -> Option<(usize, usize)> {
    let mut bits = SpsBits::new(sps);
    if bits.bits(8)? & 0x1f != 7 {
        return None;
    }
    let profile = bits.bits(8)?;
    bits.bits(16)?; // constraints and level
    bits.unsigned()?; // sequence_parameter_set_id
    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&profile) {
        let chroma = bits.unsigned()?;
        if chroma > 3 {
            return None;
        }
        if chroma == 3 {
            bits.bit()?;
        }
        bits.unsigned()?; // bit_depth_luma_minus8
        bits.unsigned()?; // bit_depth_chroma_minus8
        bits.bit()?; // qpprime_y_zero_transform_bypass_flag
        if bits.bit()? != 0 {
            for index in 0..if chroma == 3 { 12 } else { 8 } {
                if bits.bit()? != 0 {
                    let mut last = 8i32;
                    let mut next = 8i32;
                    for _ in 0..if index < 6 { 16 } else { 64 } {
                        if next != 0 {
                            next = (last + bits.signed()?).rem_euclid(256);
                        }
                        last = if next == 0 { last } else { next };
                    }
                }
            }
        }
    }
    bits.unsigned()?; // log2_max_frame_num_minus4
    match bits.unsigned()? {
        0 => {
            bits.unsigned()?; // log2_max_pic_order_cnt_lsb_minus4
        }
        1 => {
            bits.bit()?;
            bits.signed()?;
            bits.signed()?;
            let cycle = bits.unsigned()?;
            if cycle > 256 {
                return None;
            }
            for _ in 0..cycle {
                bits.signed()?;
            }
        }
        2 => {}
        _ => return None,
    }
    bits.unsigned()?; // max_num_ref_frames
    bits.bit()?; // gaps_in_frame_num_value_allowed_flag
    let width = u64::from(bits.unsigned()?)
        .checked_add(1)?
        .checked_mul(16)?;
    let height = u64::from(bits.unsigned()?)
        .checked_add(1)?
        .checked_mul(16)?;
    let frame_only = bits.bit()?;
    let height = height.checked_mul(2 - u64::from(frame_only))?;
    Some((width.try_into().ok()?, height.try_into().ok()?))
}

fn checked_annex_b(
    sample: &[u8],
    length_size: usize,
    sps: &[u8],
    pps: &[u8],
) -> Result<Vec<u8>, VideoSamplingError> {
    if !(1..=4).contains(&length_size)
        || sps.is_empty()
        || pps.is_empty()
        || sps.len() + pps.len() > 8192
        || sample.len() > MAX_SAMPLE_BYTES
    {
        return Err(VideoSamplingError::BudgetExceeded);
    }
    let mut out = Vec::with_capacity(sample.len() + 8192);
    for unit in [sps, pps] {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unit);
    }
    let mut remaining = sample;
    while !remaining.is_empty() {
        if remaining.len() < length_size {
            return Err(VideoSamplingError::DecodeFailed);
        }
        let mut length = 0usize;
        for byte in &remaining[..length_size] {
            length = (length << 8) | usize::from(*byte);
        }
        remaining = &remaining[length_size..];
        if length == 0
            || length > remaining.len()
            || out.len().saturating_add(length + 4) > 4 * MAX_SAMPLE_BYTES
        {
            return Err(VideoSamplingError::DecodeFailed);
        }
        if remaining[0] & 0x1f == 7 {
            let (width, height) = coded_sps_dimensions(&remaining[..length])
                .ok_or(VideoSamplingError::UnsupportedFormat)?;
            if !within_pixel_budget(width, height) {
                return Err(VideoSamplingError::PixelBudgetExceeded { width, height });
            }
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&remaining[..length]);
        remaining = &remaining[length..];
    }
    Ok(out)
}

/// Sample at one frame per second for short clips, up to 32 frames. For longer
/// clips, distribute 32 target timestamps uniformly across the entire track.
///
/// Decodes inter-frames sequentially from the first sync sample (not just
/// keyframes). A track with more than 2048 samples is rejected, not partially
/// indexed. The 25 MiB source, ten-minute track, 1080p and encoded-output limits are enforced
/// before allocation/decoding wherever the container exposes the metadata.
/// Streams with composition offsets (B-frame reordering) are rejected rather
/// than assigning an incorrect source timestamp to a decoded frame.
pub fn sample_video_frames(bytes: &[u8]) -> Result<Vec<SampledVideoFrame>, VideoSamplingError> {
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(VideoSamplingError::SourceTooLarge {
            size: bytes.len() as u64,
            limit: MAX_SOURCE_BYTES,
        });
    }
    let mut reader = mp4::Mp4Reader::read_header(Cursor::new(bytes), bytes.len() as u64)
        .map_err(|_| VideoSamplingError::UnsupportedFormat)?;
    if reader.is_fragmented() {
        return Err(VideoSamplingError::UnsupportedFormat);
    }
    let track_id = reader
        .tracks()
        .values()
        .filter(|track| matches!(track.media_type(), Ok(mp4::MediaType::H264)))
        .map(mp4::Mp4Track::track_id)
        .min()
        .ok_or(VideoSamplingError::UnsupportedFormat)?;
    let track = &reader.tracks()[&track_id];
    let timescale = track.timescale();
    if timescale == 0 {
        return Err(VideoSamplingError::UnsupportedFormat);
    }
    let duration_ms = u128::from(track.trak.mdia.mdhd.duration) * 1000 / u128::from(timescale);
    if duration_ms > MAX_VIDEO_DURATION_MS {
        return Err(VideoSamplingError::DurationTooLong {
            duration_ms,
            limit_ms: MAX_VIDEO_DURATION_MS,
        });
    }
    let width = track.width() as usize;
    let height = track.height() as usize;
    if !within_pixel_budget(width, height) {
        return Err(VideoSamplingError::PixelBudgetExceeded { width, height });
    }
    let avcc = &track
        .trak
        .mdia
        .minf
        .stbl
        .stsd
        .avc1
        .as_ref()
        .ok_or(VideoSamplingError::UnsupportedFormat)?
        .avcc;
    let sps: &[u8] = avcc
        .sequence_parameter_sets
        .first()
        .ok_or(VideoSamplingError::UnsupportedFormat)?
        .bytes
        .as_ref();
    let pps: &[u8] = avcc
        .picture_parameter_sets
        .first()
        .ok_or(VideoSamplingError::UnsupportedFormat)?
        .bytes
        .as_ref();
    if sps.len() + pps.len() > 8192 {
        return Err(VideoSamplingError::BudgetExceeded);
    }
    let (coded_width, coded_height) =
        coded_sps_dimensions(sps).ok_or(VideoSamplingError::UnsupportedFormat)?;
    if !within_pixel_budget(coded_width, coded_height) {
        return Err(VideoSamplingError::PixelBudgetExceeded {
            width: coded_width,
            height: coded_height,
        });
    }
    let sps = sps.to_vec();
    let pps = pps.to_vec();
    let length_size = (avcc.length_size_minus_one + 1) as usize;
    let sample_count = reader
        .sample_count(track_id)
        .map_err(|_| VideoSamplingError::UnsupportedFormat)?;
    if sample_count > MAX_DECODED_SAMPLES {
        return Err(VideoSamplingError::BudgetExceeded);
    }
    let uniform = duration_ms > (MAX_VIDEO_FRAMES as u128) * 1000;
    let frame_count = (duration_ms.div_ceil(1000) as usize).clamp(1, MAX_VIDEO_FRAMES);
    let targets: Vec<u128> = (0..frame_count)
        .map(|index| {
            if uniform {
                index as u128 * duration_ms.saturating_sub(1) / (MAX_VIDEO_FRAMES - 1) as u128
            } else {
                index as u128 * 1000
            }
        })
        .collect();
    let mut decoder = Decoder::new().map_err(|_| VideoSamplingError::DecodeFailed)?;
    let mut frames = Vec::new();
    let mut total_output_bytes = 0usize;
    let mut next_target = 0usize;
    let mut previous_timestamp = None;
    let mut started = false;
    for sample_id in 1..=sample_count {
        let sample = reader
            .read_sample(track_id, sample_id)
            .map_err(|_| VideoSamplingError::DecodeFailed)?
            .ok_or(VideoSamplingError::DecodeFailed)?;
        if sample.rendering_offset != 0 {
            return Err(VideoSamplingError::UnsupportedFormat);
        }
        let timestamp_ms = (sample.start_time as u128) * 1000 / u128::from(timescale);
        if previous_timestamp.is_some_and(|previous| timestamp_ms < previous) {
            return Err(VideoSamplingError::UnsupportedFormat);
        }
        previous_timestamp = Some(timestamp_ms);
        if timestamp_ms > duration_ms + 1000 {
            return Err(VideoSamplingError::UnsupportedFormat);
        }
        if !started {
            if !sample.is_sync {
                continue;
            }
            started = true;
        }
        if sample.bytes.len() > MAX_SAMPLE_BYTES {
            return Err(VideoSamplingError::BudgetExceeded);
        }
        let annex_b = checked_annex_b(&sample.bytes, length_size, &sps, &pps)?;
        let decoded = decoder
            .decode(&annex_b)
            .map_err(|_| VideoSamplingError::DecodeFailed)?
            .ok_or(VideoSamplingError::DecodeFailed)?;
        let matches_target = targets
            .get(next_target)
            .is_some_and(|target| timestamp_ms >= *target);
        while targets
            .get(next_target)
            .is_some_and(|target| timestamp_ms >= *target)
        {
            next_target += 1;
        }
        let is_last = uniform && sample_id == sample_count;
        if (!matches_target && !is_last)
            || frames
                .last()
                .is_some_and(|frame: &SampledVideoFrame| frame.timestamp_ms == timestamp_ms as u64)
        {
            continue;
        }
        let (decoded_width, decoded_height) = decoded.dimensions();
        if !within_pixel_budget(decoded_width, decoded_height) {
            return Err(VideoSamplingError::PixelBudgetExceeded {
                width: decoded_width,
                height: decoded_height,
            });
        }
        let rgb_len = decoded_width
            .checked_mul(decoded_height)
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or(VideoSamplingError::BudgetExceeded)?;
        let mut rgb = vec![0; rgb_len];
        decoded.write_rgb8(&mut rgb);
        let image = image::RgbImage::from_raw(decoded_width as u32, decoded_height as u32, rgb)
            .ok_or(VideoSamplingError::DecodeFailed)?;
        let image = image::DynamicImage::ImageRgb8(image);
        let image = if image.width() > MAX_FRAME_EDGE || image.height() > MAX_FRAME_EDGE {
            image.thumbnail(MAX_FRAME_EDGE, MAX_FRAME_EDGE)
        } else {
            image
        };
        let mut encoded = Vec::new();
        image.write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Jpeg)?;
        if is_last && frames.len() == MAX_VIDEO_FRAMES {
            total_output_bytes -= frames.pop().expect("last frame exists").bytes.len();
        }
        total_output_bytes += encoded.len();
        if encoded.len() > MAX_ENCODED_FRAME_BYTES || total_output_bytes > MAX_TOTAL_FRAME_BYTES {
            return Err(VideoSamplingError::BudgetExceeded);
        }
        frames.push(SampledVideoFrame {
            timestamp_ms: timestamp_ms as u64,
            bytes: encoded,
            content_type: "image/jpeg",
        });
    }
    if frames.is_empty() {
        return Err(VideoSamplingError::DecodeFailed);
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;

    /// Strips a NAL unit's Annex-B start code (openh264's own `nal_unit()`
    /// includes it as part of the slice).
    fn strip_start_code(nal: &[u8]) -> &[u8] {
        if let Some(stripped) = nal.strip_prefix(&[0, 0, 0, 1]) {
            stripped
        } else if let Some(stripped) = nal.strip_prefix(&[0, 0, 1]) {
            stripped
        } else {
            nal
        }
    }

    /// Encodes one 64x64 keyframe with the real openh264 encoder, splits its
    /// Annex-B output into SPS/PPS/slice NALs, and muxes them into a
    /// minimal-but-real MP4 container using the `mp4` crate's own writer -
    /// end-to-end through the exact demux/decode path production code uses,
    /// not a hand-rolled or pre-baked fixture.
    fn encode_fixture_mp4(width: u32, height: u32) -> Vec<u8> {
        encode_fixture_mp4_frames(width, height, &[0], 1000)
    }

    fn encode_fixture_mp4_frames(
        width: u32,
        height: u32,
        times: &[u32],
        last_duration: u32,
    ) -> Vec<u8> {
        let mut encoder = Encoder::new().expect("create encoder");
        let mut sps = Vec::new();
        let mut pps = Vec::new();
        let mut frames = Vec::new();
        for (index, &time) in times.iter().enumerate() {
            let pixels = (width * height) as usize;
            let mut values = vec![((40 + index * 11) % 215) as u8; pixels];
            values.extend(vec![128; pixels / 2]);
            let yuv = YUVBuffer::from_vec(values, width as usize, height as usize);
            let bitstream = encoder.encode(&yuv).expect("encode frame");
            let mut sample_bytes = Vec::new();
            let mut is_sync = false;
            for layer_index in 0..bitstream.num_layers() {
                let layer = bitstream.layer(layer_index).expect("layer must exist");
                for nal_index in 0..layer.nal_count() {
                    let nal = strip_start_code(layer.nal_unit(nal_index).expect("nal must exist"));
                    match nal[0] & 0x1F {
                        7 => sps = nal.to_vec(),
                        8 => pps = nal.to_vec(),
                        5 => is_sync = true,
                        _ => {}
                    }
                    if !matches!(nal[0] & 0x1F, 7 | 8) {
                        sample_bytes.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                        sample_bytes.extend_from_slice(nal);
                    }
                }
            }
            frames.push((time, is_sync, sample_bytes));
        }
        assert!(!sps.is_empty(), "encoder must emit an SPS");
        assert!(!pps.is_empty(), "encoder must emit a PPS");
        assert!(frames.iter().all(|(_, _, bytes)| !bytes.is_empty()));

        let avc_config = mp4::AvcConfig {
            width: width as u16,
            height: height as u16,
            seq_param_set: sps,
            pic_param_set: pps,
        };
        let mut out = Cursor::new(Vec::new());
        let config = mp4::Mp4Config {
            major_brand: str::parse("isom").expect("valid brand"),
            minor_version: 0,
            compatible_brands: vec![str::parse("isom").expect("valid brand")],
            timescale: 1000,
        };
        let mut writer = mp4::Mp4Writer::write_start(&mut out, &config).expect("write start");
        writer
            .add_track(&mp4::TrackConfig::from(avc_config))
            .expect("add track");
        for (index, (time, is_sync, sample_bytes)) in frames.into_iter().enumerate() {
            writer
                .write_sample(
                    1,
                    &mp4::Mp4Sample {
                        start_time: time as u64,
                        duration: times
                            .get(index + 1)
                            .map_or(last_duration, |next| next - time),
                        rendering_offset: 0,
                        is_sync,
                        bytes: sample_bytes.into(),
                    },
                )
                .expect("write sample");
        }
        writer.write_end().expect("write end");
        out.into_inner()
    }

    #[test]
    fn generates_a_thumbnail_from_the_first_keyframe_of_an_mp4() {
        let bytes = encode_fixture_mp4(64, 64);
        let thumbnail =
            generate_video_thumbnail(&bytes, ThumbnailSize::Small).expect("generate thumbnail");
        assert_eq!(thumbnail.content_type, "image/jpeg");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert!(decoded.width() <= ThumbnailSize::Small.max_dimension());
        assert!(decoded.height() <= ThumbnailSize::Small.max_dimension());
    }

    #[test]
    fn rejects_a_non_video_container_as_unsupported() {
        let error = generate_video_thumbnail(b"not an mp4 file", ThumbnailSize::Small).unwrap_err();
        assert!(matches!(error, ThumbnailError::UnsupportedFormat));
    }

    #[test]
    fn rejects_a_source_file_over_the_byte_budget_before_parsing() {
        let bytes = vec![0_u8; (MAX_SOURCE_BYTES + 1) as usize];
        let error = generate_video_thumbnail(&bytes, ThumbnailSize::Small).unwrap_err();
        assert!(matches!(error, ThumbnailError::SourceTooLarge { .. }));
    }

    #[test]
    fn recognizes_supported_video_extensions_case_insensitively() {
        for extension in ["mp4", "MP4", "m4v", "mov", "MOV"] {
            assert!(is_supported_video_extension(extension), "{extension}");
        }
        assert!(!is_supported_video_extension("mkv"));
        assert!(!is_supported_video_extension("webm"));
    }

    #[test]
    fn samples_inter_frames_by_media_timestamp_at_most_once_per_second() {
        let bytes = encode_fixture_mp4_frames(64, 64, &[0, 500, 1000, 1550, 2000, 3150], 1000);
        let mut reader =
            mp4::Mp4Reader::read_header(Cursor::new(&bytes), bytes.len() as u64).expect("fixture");
        assert!(
            !reader
                .read_sample(1, 3)
                .expect("third sample")
                .expect("frame")
                .is_sync,
            "1-second sample must actually depend on a prior keyframe"
        );
        let sampled = sample_video_frames(&bytes).expect("sample inter-frame video");
        assert_eq!(
            sampled
                .iter()
                .map(|frame| frame.timestamp_ms)
                .collect::<Vec<_>>(),
            [0, 1000, 2000, 3150]
        );
        assert!(sampled.iter().any(|frame| frame.timestamp_ms > 0));
        for frame in sampled {
            assert_eq!(frame.content_type, "image/jpeg");
            let decoded = image::load_from_memory(&frame.bytes).expect("JPEG frame");
            assert_eq!((decoded.width(), decoded.height()), (64, 64));
        }
    }

    #[test]
    fn rejects_oversized_sources_and_long_videos_before_decoding() {
        assert!(matches!(
            sample_video_frames(&vec![0; (MAX_SOURCE_BYTES + 1) as usize]),
            Err(VideoSamplingError::SourceTooLarge { .. })
        ));
        let bytes = encode_fixture_mp4_frames(64, 64, &[0], 601_000);
        assert!(matches!(
            sample_video_frames(&bytes),
            Err(VideoSamplingError::DurationTooLong { .. })
        ));
        assert!(matches!(
            sample_video_frames(b"not an mp4"),
            Err(VideoSamplingError::UnsupportedFormat)
        ));
        let mut unsupported_codec = encode_fixture_mp4(64, 64);
        let box_name = unsupported_codec
            .windows(4)
            .position(|bytes| bytes == b"avc1")
            .expect("H.264 sample entry");
        unsupported_codec[box_name..box_name + 4].copy_from_slice(b"hvc1");
        assert!(matches!(
            sample_video_frames(&unsupported_codec),
            Err(VideoSamplingError::UnsupportedFormat)
        ));
    }

    #[test]
    fn rejects_oversized_video_frames_from_track_metadata() {
        let bytes = encode_fixture_mp4(2048, 2048);
        let reader =
            mp4::Mp4Reader::read_header(Cursor::new(&bytes), bytes.len() as u64).expect("fixture");
        let sps = reader.tracks()[&1]
            .sequence_parameter_set()
            .expect("encoded SPS");
        assert_eq!(coded_sps_dimensions(sps), Some((2048, 2048)));
        assert!(matches!(
            sample_video_frames(&bytes),
            Err(VideoSamplingError::PixelBudgetExceeded { .. })
        ));
    }

    #[test]
    fn samples_long_video_uniformly_through_its_last_frame() {
        let times: Vec<_> = (0..64).map(|second| second * 1000).collect();
        let bytes = encode_fixture_mp4_frames(64, 64, &times, 1000);
        let frames = sample_video_frames(&bytes).expect("sample bounded video");
        assert_eq!(frames.len(), 32);
        assert_eq!(frames.first().expect("first frame").timestamp_ms, 0);
        assert_eq!(frames.last().expect("last frame").timestamp_ms, 63_000);
        assert!(frames[15].timestamp_ms >= 25_000);
        assert!(frames[15].timestamp_ms <= 40_000);
        assert!(frames.windows(2).all(|pair| {
            pair[1].timestamp_ms > pair[0].timestamp_ms
                && pair[1].timestamp_ms - pair[0].timestamp_ms <= 3000
        }));
    }

    #[test]
    fn refuses_video_that_cannot_be_decoded_within_the_whole_clip_budget() {
        let times: Vec<_> = (0..=MAX_DECODED_SAMPLES).map(|index| index * 250).collect();
        let bytes = encode_fixture_mp4_frames(64, 64, &times, 250);
        assert!(matches!(
            sample_video_frames(&bytes),
            Err(VideoSamplingError::BudgetExceeded)
        ));
    }

    #[test]
    fn rejects_malformed_samples_before_h264_decoder() {
        assert!(matches!(
            checked_annex_b(&[0, 0, 0, 8, 1], 4, &[0x67], &[0x68]),
            Err(VideoSamplingError::DecodeFailed)
        ));
        assert!(matches!(
            checked_annex_b(&[0, 0, 0, 1, 1], 0, &[0x67], &[0x68]),
            Err(VideoSamplingError::BudgetExceeded)
        ));
        assert_eq!(coded_sps_dimensions(&[0x67]), None);
        assert!(matches!(
            checked_annex_b(&[0, 0, 0, 1, 0x67], 4, &[0x67], &[0x68]),
            Err(VideoSamplingError::UnsupportedFormat)
        ));
    }
}
