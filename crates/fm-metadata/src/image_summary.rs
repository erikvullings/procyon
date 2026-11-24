//! A short display summary of an image, read from its leading bytes (task 0347).
//!
//! Provider-neutral and bounded like [`crate::read_image_tags`]; malformed or non-image input
//! yields an empty summary rather than an error.

use std::io::Cursor;

use exif::{In, Reader, Tag, Value};

use crate::image_tags::read_image_tags;
use crate::photo::{CaptureTime, read_photo_info};

/// What a grid caption or tooltip shows about an image.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageSummary {
    /// Pixel width.
    pub width: Option<u32>,
    /// Pixel height.
    pub height: Option<u32>,
    /// Bits per pixel, for example 24 for 8-bit RGB.
    pub bit_depth: Option<u8>,
    /// When the picture was taken.
    pub date_taken: Option<CaptureTime>,
    /// Camera maker and model as one label.
    pub camera: Option<String>,
    /// Aperture as an f-number.
    pub aperture: Option<f64>,
    /// Exposure time in seconds.
    pub shutter_seconds: Option<f64>,
    /// ISO sensitivity.
    pub iso: Option<u32>,
    /// Focal length in millimetres.
    pub focal_length_mm: Option<f64>,
    /// Exposure bias in EV steps.
    pub exposure_bias_ev: Option<f64>,
    /// Whether the flash fired.
    pub flash_fired: Option<bool>,
    /// True for automatic white balance, false for manual.
    pub white_balance_auto: Option<bool>,
    /// XMP rating 0-5.
    pub rating: Option<u8>,
}

/// Pixel size and bits per pixel from a JPEG's start-of-frame segment.
fn jpeg_frame(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut offset = 2;
    while offset + 4 <= bytes.len() && bytes[offset] == 0xFF {
        let marker = bytes[offset + 1];
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]));
        if length < 2 {
            return None;
        }
        let is_frame = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_frame {
            // Segment data: precision(1) height(2) width(2) components(1).
            let data = bytes.get(offset + 4..offset + 10)?;
            let precision = data[0];
            let height = u32::from(u16::from_be_bytes([data[1], data[2]]));
            let width = u32::from(u16::from_be_bytes([data[3], data[4]]));
            let bits = precision.checked_mul(data[5])?;
            return Some((width, height, bits));
        }
        offset += 2 + length;
    }
    None
}

/// Bits per pixel of a PNG, from its IHDR chunk.
fn png_bit_depth(bytes: &[u8]) -> Option<u8> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    // IHDR data starts at 16: width(4) height(4) depth(1) colour type(1).
    let depth = *bytes.get(24)?;
    let channels = match *bytes.get(25)? {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        6 => 4,
        _ => return None,
    };
    depth.checked_mul(channels)
}

fn rational(field: &exif::Field) -> Option<f64> {
    match &field.value {
        Value::Rational(parts) => parts
            .first()
            .filter(|part| part.denom != 0)
            .map(exif::Rational::to_f64),
        Value::SRational(parts) => parts
            .first()
            .filter(|part| part.denom != 0)
            .map(exif::SRational::to_f64),
        _ => None,
    }
}

/// Reads the display summary of an image from its leading bytes.
#[must_use]
pub fn read_image_summary(bytes: &[u8]) -> ImageSummary {
    let info = read_photo_info(bytes);
    let frame = jpeg_frame(bytes);
    let mut summary = ImageSummary {
        width: info.width.or(frame.map(|(width, _, _)| width)),
        height: info.height.or(frame.map(|(_, height, _)| height)),
        bit_depth: frame
            .map(|(_, _, bits)| bits)
            .or_else(|| png_bit_depth(bytes)),
        date_taken: info.date_taken,
        camera: info.camera(),
        ..ImageSummary::default()
    };
    if let Ok(exif) = Reader::new().read_from_container(&mut Cursor::new(bytes)) {
        let get = |tag| exif.get_field(tag, In::PRIMARY);
        summary.aperture = get(Tag::FNumber).and_then(rational);
        summary.shutter_seconds = get(Tag::ExposureTime).and_then(rational);
        summary.iso = get(Tag::PhotographicSensitivity).and_then(|field| field.value.get_uint(0));
        summary.focal_length_mm = get(Tag::FocalLength).and_then(rational);
        summary.exposure_bias_ev = get(Tag::ExposureBiasValue).and_then(rational);
        summary.flash_fired = get(Tag::Flash)
            .and_then(|field| field.value.get_uint(0))
            .map(|flash| flash & 1 == 1);
        summary.white_balance_auto = get(Tag::WhiteBalance)
            .and_then(|field| field.value.get_uint(0))
            .map(|balance| balance == 0);
    }
    summary.rating = xmp_rating(bytes);
    summary
}

/// The `xmp:Rating` value (0-5) when the file carries an XMP packet with one.
fn xmp_rating(bytes: &[u8]) -> Option<u8> {
    read_image_tags(bytes)
        .groups
        .iter()
        .filter(|group| group.namespace == "XMP")
        .flat_map(|group| group.tags.iter())
        .find(|tag| tag.name == "xmp:Rating")
        .and_then(|tag| tag.value.trim().parse::<u8>().ok())
        .filter(|rating| *rating <= 5)
}
