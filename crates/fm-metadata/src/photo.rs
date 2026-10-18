//! Photo properties from EXIF and image headers (task 0267, reused by the image metadata viewer).
//!
//! Works on in-memory bytes (the head of the file is enough: EXIF sits in the first segments), so
//! it is provider-neutral. Malformed or non-photo input yields an empty [`PhotoInfo`].

use std::io::Cursor;

use exif::{In, Reader, Tag, Value};

/// A calendar date and time as recorded by the camera (no time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureTime {
    /// Four-digit year.
    pub year: u16,
    /// Month, 1-12.
    pub month: u8,
    /// Day of the month, 1-31.
    pub day: u8,
    /// Hour, 0-23.
    pub hour: u8,
    /// Minute, 0-59.
    pub minute: u8,
    /// Second, 0-59.
    pub second: u8,
}

/// Properties of a photo.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhotoInfo {
    /// When the picture was taken.
    pub date_taken: Option<CaptureTime>,
    /// Camera maker.
    pub camera_make: Option<String>,
    /// Camera model.
    pub camera_model: Option<String>,
    /// Pixel width.
    pub width: Option<u32>,
    /// Pixel height.
    pub height: Option<u32>,
}

impl PhotoInfo {
    /// The camera as a single label: the model, prefixed by the maker unless it repeats it.
    #[must_use]
    pub fn camera(&self) -> Option<String> {
        match (&self.camera_make, &self.camera_model) {
            (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
                Some(model.clone())
            }
            (Some(make), Some(model)) => Some(format!("{make} {model}")),
            (None, Some(model)) => Some(model.clone()),
            (Some(make), None) => Some(make.clone()),
            (None, None) => None,
        }
    }
}

fn parse_capture_time(text: &str) -> Option<CaptureTime> {
    // EXIF format: "YYYY:MM:DD HH:MM:SS".
    let (date, time) = text.trim().split_once(' ')?;
    let mut date_parts = date.split(':');
    let mut time_parts = time.split(':');
    let capture = CaptureTime {
        year: date_parts.next()?.parse().ok()?,
        month: date_parts.next()?.parse().ok()?,
        day: date_parts.next()?.parse().ok()?,
        hour: time_parts.next()?.parse().ok()?,
        minute: time_parts.next()?.parse().ok()?,
        second: time_parts.next()?.parse().ok()?,
    };
    let valid = capture.year > 0
        && (1..=12).contains(&capture.month)
        && (1..=31).contains(&capture.day)
        && capture.hour < 24
        && capture.minute < 60
        && capture.second < 60;
    valid.then_some(capture)
}

fn ascii(field: &exif::Field) -> Option<String> {
    match &field.value {
        Value::Ascii(values) => values
            .first()
            .map(|bytes| {
                String::from_utf8_lossy(bytes)
                    .trim_matches(['\0', ' '])
                    .to_owned()
            })
            .filter(|text| !text.is_empty()),
        _ => None,
    }
}

fn dimension(field: &exif::Field) -> Option<u32> {
    field.value.get_uint(0)
}

/// Reads EXIF and header properties from the leading bytes of an image file.
#[must_use]
pub fn read_photo_info(bytes: &[u8]) -> PhotoInfo {
    let mut info = PhotoInfo::default();
    if let Ok(exif) = Reader::new().read_from_container(&mut Cursor::new(bytes)) {
        let text = |tag| exif.get_field(tag, In::PRIMARY).and_then(ascii);
        info.date_taken = text(Tag::DateTimeOriginal)
            .or_else(|| text(Tag::DateTimeDigitized))
            .or_else(|| text(Tag::DateTime))
            .and_then(|value| parse_capture_time(&value));
        info.camera_make = text(Tag::Make);
        info.camera_model = text(Tag::Model);
        info.width = exif
            .get_field(Tag::PixelXDimension, In::PRIMARY)
            .and_then(dimension);
        info.height = exif
            .get_field(Tag::PixelYDimension, In::PRIMARY)
            .and_then(dimension);
    }
    if (info.width.is_none() || info.height.is_none())
        && let Ok(reader) = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()
        && let Ok((width, height)) = reader.into_dimensions()
    {
        info.width = Some(width);
        info.height = Some(height);
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal JPEG with an APP1 EXIF block holding Make, Model and DateTimeOriginal.
    fn jpeg_with_exif() -> Vec<u8> {
        // TIFF header (little endian) + IFD0 with Make, Model, ExifIFDPointer; Exif IFD with
        // DateTimeOriginal.
        let make = b"ACME\0";
        let model = b"Cam 9000\0";
        let date = b"2024:03:05 14:07:09\0";
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II*\0");
        tiff.extend_from_slice(&8_u32.to_le_bytes());
        // IFD0: 3 entries.
        let ifd0_len = 2 + 3 * 12 + 4;
        let make_offset = 8 + ifd0_len;
        let model_offset = make_offset + make.len();
        let exif_ifd_offset = model_offset + model.len();
        tiff.extend_from_slice(&3_u16.to_le_bytes());
        for (tag, kind, count, value) in [
            (0x010F_u16, 2_u16, make.len() as u32, make_offset as u32),
            (0x0110, 2, model.len() as u32, model_offset as u32),
            (0x8769, 4, 1, exif_ifd_offset as u32),
        ] {
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&kind.to_le_bytes());
            tiff.extend_from_slice(&count.to_le_bytes());
            tiff.extend_from_slice(&value.to_le_bytes());
        }
        tiff.extend_from_slice(&0_u32.to_le_bytes());
        tiff.extend_from_slice(make);
        tiff.extend_from_slice(model);
        // Exif IFD: 1 entry.
        let date_offset = exif_ifd_offset + 2 + 12 + 4;
        tiff.extend_from_slice(&1_u16.to_le_bytes());
        tiff.extend_from_slice(&0x9003_u16.to_le_bytes());
        tiff.extend_from_slice(&2_u16.to_le_bytes());
        tiff.extend_from_slice(&(date.len() as u32).to_le_bytes());
        tiff.extend_from_slice(&(date_offset as u32).to_le_bytes());
        tiff.extend_from_slice(&0_u32.to_le_bytes());
        tiff.extend_from_slice(date);

        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&app1);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn reads_camera_and_capture_time_from_exif() {
        let info = read_photo_info(&jpeg_with_exif());
        assert_eq!(info.camera().as_deref(), Some("ACME Cam 9000"));
        assert_eq!(
            info.date_taken,
            Some(CaptureTime {
                year: 2024,
                month: 3,
                day: 5,
                hour: 14,
                minute: 7,
                second: 9
            })
        );
    }

    #[test]
    fn camera_does_not_repeat_the_maker() {
        let info = PhotoInfo {
            camera_make: Some("Canon".into()),
            camera_model: Some("Canon EOS R5".into()),
            ..PhotoInfo::default()
        };
        assert_eq!(info.camera().as_deref(), Some("Canon EOS R5"));
    }

    #[test]
    fn reads_dimensions_from_a_png_header() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(7, 3)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode png");
        let info = read_photo_info(&png);
        assert_eq!((info.width, info.height), (Some(7), Some(3)));
        assert_eq!(info.date_taken, None);
    }

    #[test]
    fn garbage_yields_an_empty_info() {
        assert_eq!(
            read_photo_info(b"definitely not an image"),
            PhotoInfo::default()
        );
    }

    #[test]
    fn rejects_impossible_dates() {
        assert_eq!(parse_capture_time("2024:13:40 00:00:00"), None);
        assert_eq!(parse_capture_time("garbage"), None);
    }
}
