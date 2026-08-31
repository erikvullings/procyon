//! First-frame thumbnail extraction for H.264-in-MP4/MOV video (task 0134
//! follow-up). Pure-Rust demuxing (`mp4`) plus a from-source-compiled H.264
//! decoder (`openh264`, BSD-2-Clause, no runtime external tool - compiled
//! into the binary the same way `rars`/`sevenz-rust2` already are for
//! archive support) rather than shelling out to `ffmpeg`.
//!
//! Scope is deliberately narrow: only the first keyframe of the first H.264
//! video track is decoded. Other codecs (VP9, HEVC, AV1) and non-ISO-BMFF
//! containers (MKV, WebM, AVI) are reported as [`ThumbnailError::UnsupportedFormat`]
//! rather than half-implemented - the same "report false" convention as
//! every other thumbnail format here.

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
        let mut encoder = Encoder::new().expect("create encoder");
        let yuv = YUVBuffer::new(width as usize, height as usize);
        let bitstream = encoder.encode(&yuv).expect("encode frame");

        let mut sps = Vec::new();
        let mut pps = Vec::new();
        let mut slice_nals: Vec<Vec<u8>> = Vec::new();
        for layer_index in 0..bitstream.num_layers() {
            let layer = bitstream.layer(layer_index).expect("layer must exist");
            for nal_index in 0..layer.nal_count() {
                let nal = strip_start_code(layer.nal_unit(nal_index).expect("nal must exist"));
                match nal[0] & 0x1F {
                    7 => sps = nal.to_vec(),
                    8 => pps = nal.to_vec(),
                    _ => slice_nals.push(nal.to_vec()),
                }
            }
        }
        assert!(!sps.is_empty(), "encoder must emit an SPS");
        assert!(!pps.is_empty(), "encoder must emit a PPS");
        assert!(
            !slice_nals.is_empty(),
            "encoder must emit at least one slice NAL"
        );

        let avc_config = mp4::AvcConfig {
            width: width as u16,
            height: height as u16,
            seq_param_set: sps,
            pic_param_set: pps,
        };
        let mut sample_bytes = Vec::new();
        for nal in &slice_nals {
            sample_bytes.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            sample_bytes.extend_from_slice(nal);
        }

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
        writer
            .write_sample(
                1,
                &mp4::Mp4Sample {
                    start_time: 0,
                    duration: 1000,
                    rendering_offset: 0,
                    is_sync: true,
                    bytes: sample_bytes.into(),
                },
            )
            .expect("write sample");
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
}
