//! Lossless image metadata editing (task 0274).
//!
//! Only metadata containers are rewritten; the compressed image data is copied byte for byte.
//! EXIF (including GPS) is edited with `little_exif`, which keeps tags it does not understand.
//! For JPEG, XMP and an already present IPTC block are kept consistent with the edited fields so
//! readers that prefer them do not show stale values. Formats are explicit: [`editable_fields`].

use little_exif::exif_tag::ExifTag;
use little_exif::filetype::FileExtension;
use little_exif::ifd::ExifTagGroup;
use little_exif::metadata::Metadata;
use little_exif::rational::uR64;
use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, Event};

use crate::image_tags::{GpsPosition, detect_format};
use crate::photo::{CaptureTime, read_photo_info};

const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const PHOTOSHOP_HEADER: &[u8] = b"Photoshop 3.0\0";
const MAX_SEGMENT_PAYLOAD: usize = 65_533;

/// What to do with one field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Change<T> {
    /// Leave the stored value alone.
    #[default]
    Keep,
    /// Store this value.
    Set(T),
    /// Remove the stored value.
    Clear,
}

impl<T> Change<T> {
    fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

/// A requested metadata edit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageEdit {
    /// Remove EXIF, XMP and IPTC (the orientation is kept so the picture does not turn).
    pub remove_all: bool,
    /// Remove GPS coordinates from EXIF and XMP.
    pub remove_location: bool,
    /// New capture time.
    pub date_taken: Change<CaptureTime>,
    /// Shift the stored capture time by this many seconds (camera clock correction).
    pub shift_date_seconds: Option<i64>,
    /// New GPS position.
    pub gps: Change<GpsPosition>,
    /// Title (JPEG: XMP and IPTC).
    pub title: Change<String>,
    /// Description or caption.
    pub description: Change<String>,
    /// Copyright notice.
    pub copyright: Change<String>,
    /// Author or creator.
    pub author: Change<String>,
    /// Keywords (JPEG: XMP and IPTC).
    pub keywords: Change<Vec<String>>,
    /// Star rating 0-5 (JPEG: XMP).
    pub rating: Change<u8>,
    /// EXIF orientation 1-8.
    pub orientation: Change<u8>,
}

/// A field a format can store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditableField {
    /// Capture time and its shifting.
    DateTaken,
    /// GPS position and removing it.
    Gps,
    /// Description.
    Description,
    /// Copyright.
    Copyright,
    /// Author.
    Author,
    /// Orientation.
    Orientation,
    /// Title.
    Title,
    /// Keywords.
    Keywords,
    /// Rating.
    Rating,
}

impl EditableField {
    /// Stable identifier used on the wire.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::DateTaken => "dateTaken",
            Self::Gps => "gps",
            Self::Description => "description",
            Self::Copyright => "copyright",
            Self::Author => "author",
            Self::Orientation => "orientation",
            Self::Title => "title",
            Self::Keywords => "keywords",
            Self::Rating => "rating",
        }
    }
}

const EXIF_FIELDS: &[EditableField] = &[
    EditableField::DateTaken,
    EditableField::Gps,
    EditableField::Description,
    EditableField::Copyright,
    EditableField::Author,
    EditableField::Orientation,
];

const JPEG_FIELDS: &[EditableField] = &[
    EditableField::DateTaken,
    EditableField::Gps,
    EditableField::Description,
    EditableField::Copyright,
    EditableField::Author,
    EditableField::Orientation,
    EditableField::Title,
    EditableField::Keywords,
    EditableField::Rating,
];

/// Fields that can be edited in a container format; empty when the format cannot be written.
#[must_use]
pub fn editable_fields(format: Option<&str>) -> &'static [EditableField] {
    match format {
        Some("JPEG") => JPEG_FIELDS,
        Some("TIFF" | "WebP") => EXIF_FIELDS,
        _ => &[],
    }
}

/// Why an edit could not be applied.
#[derive(Debug, thiserror::Error)]
pub enum ImageEditError {
    /// The container cannot be rewritten safely.
    #[error("editing metadata is not supported for this file format")]
    UnsupportedFormat,
    /// The edit asks for a field the format cannot store.
    #[error("this format cannot store {0}")]
    UnsupportedField(&'static str),
    /// A value is out of range.
    #[error("invalid value: {0}")]
    InvalidValue(String),
    /// The file structure is damaged.
    #[error("the image structure is damaged: {0}")]
    Malformed(String),
    /// The metadata writer failed.
    #[error("writing metadata failed: {0}")]
    Writer(String),
}

fn file_extension(format: &str) -> FileExtension {
    match format {
        "JPEG" => FileExtension::JPEG,
        "TIFF" => FileExtension::TIFF,
        "WebP" => FileExtension::WEBP,
        _ => FileExtension::TIFF,
    }
}

fn used_fields(edit: &ImageEdit) -> Vec<EditableField> {
    let mut fields = Vec::new();
    let mut add = |used: bool, field| {
        if used {
            fields.push(field);
        }
    };
    add(
        !edit.date_taken.is_keep() || edit.shift_date_seconds.is_some(),
        EditableField::DateTaken,
    );
    add(!edit.gps.is_keep(), EditableField::Gps);
    add(!edit.description.is_keep(), EditableField::Description);
    add(!edit.copyright.is_keep(), EditableField::Copyright);
    add(!edit.author.is_keep(), EditableField::Author);
    add(!edit.orientation.is_keep(), EditableField::Orientation);
    add(!edit.title.is_keep(), EditableField::Title);
    add(!edit.keywords.is_keep(), EditableField::Keywords);
    add(!edit.rating.is_keep(), EditableField::Rating);
    fields
}

fn validate(edit: &ImageEdit) -> Result<(), ImageEditError> {
    if let Change::Set(rating) = edit.rating
        && rating > 5
    {
        return Err(ImageEditError::InvalidValue("rating must be 0-5".into()));
    }
    if let Change::Set(orientation) = edit.orientation
        && !(1..=8).contains(&orientation)
    {
        return Err(ImageEditError::InvalidValue(
            "orientation must be 1-8".into(),
        ));
    }
    if let Change::Set(gps) = edit.gps
        && !(gps.latitude.abs() <= 90.0 && gps.longitude.abs() <= 180.0)
    {
        return Err(ImageEditError::InvalidValue(
            "latitude must be within 90 and longitude within 180 degrees".into(),
        ));
    }
    if let Change::Set(time) = &edit.date_taken
        && !(time.year > 0
            && (1..=12).contains(&time.month)
            && (1..=31).contains(&time.day)
            && time.hour < 24
            && time.minute < 60
            && time.second < 60)
    {
        return Err(ImageEditError::InvalidValue("invalid date".into()));
    }
    Ok(())
}

/* ------------------------------ date arithmetic ------------------------------ */

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Moves a capture time by a number of seconds; `None` when the result leaves years 1-9999.
#[must_use]
pub fn shift_capture_time(time: CaptureTime, seconds: i64) -> Option<CaptureTime> {
    let day = days_from_civil(
        i64::from(time.year),
        i64::from(time.month),
        i64::from(time.day),
    );
    let total = day * 86_400
        + i64::from(time.hour) * 3600
        + i64::from(time.minute) * 60
        + i64::from(time.second)
        + seconds;
    let (year, month, day) = civil_from_days(total.div_euclid(86_400));
    let rest = total.rem_euclid(86_400);
    Some(CaptureTime {
        year: u16::try_from(year).ok().filter(|year| *year >= 1)?,
        month: u8::try_from(month).ok()?,
        day: u8::try_from(day).ok()?,
        hour: u8::try_from(rest / 3600).ok()?,
        minute: u8::try_from(rest % 3600 / 60).ok()?,
        second: u8::try_from(rest % 60).ok()?,
    })
}

fn exif_date(time: &CaptureTime) -> String {
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}\0",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    )
}

/* ---------------------------------- EXIF ---------------------------------- */

fn dms(value: f64) -> Vec<uR64> {
    let value = value.abs();
    let degrees = value.floor();
    let minutes_total = (value - degrees) * 60.0;
    let minutes = minutes_total.floor();
    let seconds = ((minutes_total - minutes) * 60.0 * 10_000.0).round();
    [(degrees, 1), (minutes, 1), (seconds, 10_000)]
        .into_iter()
        .map(|(numerator, denominator)| uR64 {
            nominator: numerator as u32,
            denominator,
        })
        .collect()
}

fn remove_gps_tags(metadata: &mut Metadata) {
    let hexes: Vec<u16> = metadata
        .into_iter()
        .filter(|tag| tag.get_group() == ExifTagGroup::GPS)
        .map(ExifTag::as_u16)
        .collect();
    for hex in hexes {
        metadata.remove_tag_by_hex_group(hex, ExifTagGroup::GPS);
    }
}

fn apply_exif(
    metadata: &mut Metadata,
    edit: &ImageEdit,
    existing_date: Option<CaptureTime>,
) -> Result<(), ImageEditError> {
    let set_date = |metadata: &mut Metadata, time: &CaptureTime| {
        metadata.set_tag(ExifTag::DateTimeOriginal(exif_date(time)));
        metadata.set_tag(ExifTag::CreateDate(exif_date(time)));
    };
    match &edit.date_taken {
        Change::Set(time) => set_date(metadata, time),
        Change::Clear => {
            metadata.remove_tag(ExifTag::DateTimeOriginal(String::new()));
            metadata.remove_tag(ExifTag::CreateDate(String::new()));
        }
        Change::Keep => {}
    }
    if let Some(seconds) = edit.shift_date_seconds
        && edit.date_taken.is_keep()
        && let Some(time) = existing_date
    {
        let shifted = shift_capture_time(time, seconds)
            .ok_or_else(|| ImageEditError::InvalidValue("shifted date is out of range".into()))?;
        set_date(metadata, &shifted);
    }
    if edit.remove_location {
        remove_gps_tags(metadata);
    }
    match edit.gps {
        Change::Set(position) => {
            remove_gps_tags(metadata);
            metadata.set_tag(ExifTag::GPSVersionID(vec![2, 3, 0, 0]));
            metadata.set_tag(ExifTag::GPSLatitudeRef(
                if position.latitude < 0.0 { "S" } else { "N" }.to_owned(),
            ));
            metadata.set_tag(ExifTag::GPSLatitude(dms(position.latitude)));
            metadata.set_tag(ExifTag::GPSLongitudeRef(
                if position.longitude < 0.0 { "W" } else { "E" }.to_owned(),
            ));
            metadata.set_tag(ExifTag::GPSLongitude(dms(position.longitude)));
        }
        Change::Clear => remove_gps_tags(metadata),
        Change::Keep => {}
    }
    let mut text = |change: &Change<String>, make: fn(String) -> ExifTag| match change {
        Change::Set(value) => metadata.set_tag(make(value.clone())),
        Change::Clear => {
            metadata.remove_tag(make(String::new()));
        }
        Change::Keep => {}
    };
    text(&edit.description, ExifTag::ImageDescription);
    text(&edit.copyright, ExifTag::Copyright);
    text(&edit.author, ExifTag::Artist);
    match edit.orientation {
        Change::Set(value) => metadata.set_tag(ExifTag::Orientation(vec![u16::from(value)])),
        Change::Clear => {
            metadata.remove_tag(ExifTag::Orientation(Vec::new()));
        }
        Change::Keep => {}
    }
    Ok(())
}

fn existing_orientation(bytes: &[u8]) -> Option<u16> {
    let exif = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    field
        .value
        .get_uint(0)
        .and_then(|value| u16::try_from(value).ok())
}

/* ----------------------------------- JPEG ----------------------------------- */

struct JpegSegment {
    marker: u8,
    /// Bytes between the length field and the next segment.
    payload: Vec<u8>,
}

/// Splits a JPEG into the segments before the scan, and the untouched rest (scan data onward).
fn split_jpeg(bytes: &[u8]) -> Result<(Vec<JpegSegment>, &[u8]), ImageEditError> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return Err(ImageEditError::Malformed(
            "missing JPEG start marker".into(),
        ));
    }
    let mut segments = Vec::new();
    let mut offset = 2;
    loop {
        if offset + 4 > bytes.len() || bytes[offset] != 0xFF {
            return Err(ImageEditError::Malformed("truncated JPEG header".into()));
        }
        let marker = bytes[offset + 1];
        if marker == 0xDA || marker == 0xD9 {
            return Ok((segments, &bytes[offset..]));
        }
        let length = usize::from(u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]));
        if length < 2 || offset + 2 + length > bytes.len() {
            return Err(ImageEditError::Malformed("bad JPEG segment length".into()));
        }
        segments.push(JpegSegment {
            marker,
            payload: bytes[offset + 4..offset + 2 + length].to_vec(),
        });
        offset += 2 + length;
    }
}

fn join_jpeg(segments: &[JpegSegment], rest: &[u8]) -> Result<Vec<u8>, ImageEditError> {
    let mut output = vec![0xFF, 0xD8];
    for segment in segments {
        let length = u16::try_from(segment.payload.len() + 2)
            .map_err(|_| ImageEditError::Writer("a metadata segment exceeds 64 KiB".into()))?;
        output.extend_from_slice(&[0xFF, segment.marker]);
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(&segment.payload);
    }
    output.extend_from_slice(rest);
    Ok(output)
}

fn is_exif(segment: &JpegSegment) -> bool {
    segment.marker == 0xE1 && segment.payload.starts_with(b"Exif\0")
}

fn is_xmp(segment: &JpegSegment) -> bool {
    segment.marker == 0xE1 && segment.payload.starts_with(XMP_HEADER)
}

fn strip_jpeg_metadata(bytes: &[u8]) -> Result<Vec<u8>, ImageEditError> {
    let (segments, rest) = split_jpeg(bytes)?;
    let kept: Vec<JpegSegment> = segments
        .into_iter()
        .filter(|segment| {
            !(is_exif(segment)
                || is_xmp(segment)
                || segment.marker == 0xE1
                || segment.marker == 0xEC
                || segment.marker == 0xED
                || segment.marker == 0xFE)
        })
        .collect();
    join_jpeg(&kept, rest)
}

/* ------------------------------------ XMP ------------------------------------ */

struct XmpChanges<'a> {
    title: &'a Change<String>,
    description: &'a Change<String>,
    rights: &'a Change<String>,
    creator: &'a Change<String>,
    keywords: &'a Change<Vec<String>>,
    rating: &'a Change<u8>,
    strip_gps: bool,
}

impl XmpChanges<'_> {
    fn touches_anything(&self) -> bool {
        self.strip_gps
            || !(self.title.is_keep()
                && self.description.is_keep()
                && self.rights.is_keep()
                && self.creator.is_keep()
                && self.keywords.is_keep()
                && self.rating.is_keep())
    }

    fn removes_property(&self, name: &str) -> bool {
        match name {
            "dc:title" => !self.title.is_keep(),
            "dc:description" => !self.description.is_keep(),
            "dc:rights" => !self.rights.is_keep(),
            "dc:creator" => !self.creator.is_keep(),
            "dc:subject" => !self.keywords.is_keep(),
            "xmp:Rating" => !self.rating.is_keep(),
            other => self.strip_gps && other.starts_with("exif:GPS"),
        }
    }

    fn new_elements(&self) -> String {
        let escape = |text: &str| quick_xml::escape::escape(text).into_owned();
        let alt = |name: &str, change: &Change<String>| match change {
            Change::Set(value) => format!(
                "<{name}><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></{name}>",
                escape(value)
            ),
            _ => String::new(),
        };
        let mut out = String::new();
        out.push_str(&alt("dc:title", self.title));
        out.push_str(&alt("dc:description", self.description));
        out.push_str(&alt("dc:rights", self.rights));
        if let Change::Set(author) = self.creator {
            out.push_str(&format!(
                "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
                escape(author)
            ));
        }
        if let Change::Set(keywords) = self.keywords
            && !keywords.is_empty()
        {
            out.push_str("<dc:subject><rdf:Bag>");
            for keyword in keywords {
                out.push_str(&format!("<rdf:li>{}</rdf:li>", escape(keyword)));
            }
            out.push_str("</rdf:Bag></dc:subject>");
        }
        if let Change::Set(rating) = self.rating {
            out.push_str(&format!("<xmp:Rating>{rating}</xmp:Rating>"));
        }
        out
    }
}

const FRESH_XMP: &str = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\"></rdf:Description></rdf:RDF></x:xmpmeta>";

fn xmp_error(error: impl std::fmt::Display) -> ImageEditError {
    ImageEditError::Malformed(format!("XMP: {error}"))
}

/// Rewrites the `x:xmpmeta` document: managed properties are replaced, everything else is kept.
fn rewrite_xmp(document: &str, changes: &XmpChanges<'_>) -> Result<String, ImageEditError> {
    let mut reader = Reader::from_str(document);
    let mut writer = Writer::new(Vec::new());
    let mut depth = 0_usize;
    let mut skip_depth: Option<usize> = None;
    let mut first_description_done = false;
    let new_elements = changes.new_elements();
    let name_of = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    loop {
        let event = reader.read_event().map_err(xmp_error)?;
        if let Some(skip_at) = skip_depth {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == skip_at {
                        skip_depth = None;
                    }
                }
                Event::Eof => return Err(xmp_error("unexpected end")),
                _ => {}
            }
            continue;
        }
        match event {
            Event::Eof => break,
            Event::Start(ref start) | Event::Empty(ref start)
                if depth == 2 && name_of(start.name().as_ref()) == "rdf:Description" =>
            {
                let is_start = matches!(event, Event::Start(_));
                let mut description = BytesStart::new("rdf:Description");
                let mut have_dc = false;
                let mut have_xmp = false;
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(xmp_error)?;
                    let key = name_of(attribute.key.as_ref());
                    if changes.removes_property(&key) {
                        continue;
                    }
                    have_dc |= key == "xmlns:dc";
                    have_xmp |= key == "xmlns:xmp";
                    let value = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(xmp_error)?
                        .into_owned();
                    description.push_attribute((key.as_str(), value.as_str()));
                }
                let inject = !first_description_done;
                if inject {
                    if !have_dc {
                        description
                            .push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
                    }
                    if !have_xmp {
                        description.push_attribute(("xmlns:xmp", "http://ns.adobe.com/xap/1.0/"));
                    }
                }
                first_description_done = true;
                if is_start {
                    writer
                        .write_event(Event::Start(description))
                        .map_err(xmp_error)?;
                    if inject {
                        writer.get_mut().extend_from_slice(new_elements.as_bytes());
                    }
                    depth += 1;
                } else if inject && !new_elements.is_empty() {
                    writer
                        .write_event(Event::Start(description))
                        .map_err(xmp_error)?;
                    writer.get_mut().extend_from_slice(new_elements.as_bytes());
                    writer
                        .write_event(Event::End(BytesEnd::new("rdf:Description")))
                        .map_err(xmp_error)?;
                } else {
                    writer
                        .write_event(Event::Empty(description))
                        .map_err(xmp_error)?;
                }
            }
            Event::Start(ref start)
                if depth == 3 && changes.removes_property(&name_of(start.name().as_ref())) =>
            {
                skip_depth = Some(depth);
                depth += 1;
            }
            Event::Empty(ref start)
                if depth == 3 && changes.removes_property(&name_of(start.name().as_ref())) => {}
            Event::Start(_) => {
                depth += 1;
                writer.write_event(event).map_err(xmp_error)?;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                writer.write_event(event).map_err(xmp_error)?;
            }
            other => {
                writer.write_event(other).map_err(xmp_error)?;
            }
        }
    }
    String::from_utf8(writer.into_inner()).map_err(xmp_error)
}

/// Rewrites or creates the XMP segment of a JPEG.
fn edit_jpeg_xmp(
    segments: &mut Vec<JpegSegment>,
    changes: &XmpChanges<'_>,
) -> Result<(), ImageEditError> {
    if !changes.touches_anything() {
        return Ok(());
    }
    let existing = segments.iter().position(is_xmp);
    let (prefix, document, suffix) = match existing {
        Some(index) => {
            let body = std::str::from_utf8(&segments[index].payload[XMP_HEADER.len()..])
                .map_err(|_| ImageEditError::Malformed("XMP is not UTF-8".into()))?;
            match (body.find("<x:xmpmeta"), body.find("</x:xmpmeta>")) {
                (Some(start), Some(end)) => {
                    let end = end + "</x:xmpmeta>".len();
                    (
                        body[..start].to_owned(),
                        body[start..end].to_owned(),
                        body[end..].to_owned(),
                    )
                }
                _ => (String::new(), FRESH_XMP.to_owned(), String::new()),
            }
        }
        None if changes.strip_gps && !changes_set_anything(changes) => return Ok(()),
        None => (String::new(), FRESH_XMP.to_owned(), String::new()),
    };
    let rewritten = rewrite_xmp(&document, changes)?;
    let mut payload = XMP_HEADER.to_vec();
    payload.extend_from_slice(prefix.as_bytes());
    payload.extend_from_slice(rewritten.as_bytes());
    payload.extend_from_slice(suffix.as_bytes());
    if payload.len() > MAX_SEGMENT_PAYLOAD {
        return Err(ImageEditError::Writer(
            "the XMP block would exceed the 64 KiB JPEG segment limit".into(),
        ));
    }
    let segment = JpegSegment {
        marker: 0xE1,
        payload,
    };
    match existing {
        Some(index) => segments[index] = segment,
        None => {
            let at = segments
                .iter()
                .rposition(|segment| segment.marker == 0xE0 || is_exif(segment))
                .map_or(0, |index| index + 1);
            segments.insert(at, segment);
        }
    }
    Ok(())
}

fn changes_set_anything(changes: &XmpChanges<'_>) -> bool {
    matches!(changes.title, Change::Set(_))
        || matches!(changes.description, Change::Set(_))
        || matches!(changes.rights, Change::Set(_))
        || matches!(changes.creator, Change::Set(_))
        || matches!(changes.keywords, Change::Set(_))
        || matches!(changes.rating, Change::Set(_))
}

/* ------------------------------------ IPTC ----------------------------------- */

const IPTC_UTF8_CHARSET: &[u8] = b"\x1b%G";

struct IptcDataset {
    record: u8,
    dataset: u8,
    value: Vec<u8>,
}

fn parse_iptc(data: &[u8]) -> Vec<IptcDataset> {
    let mut datasets = Vec::new();
    let mut rest = data;
    while rest.len() >= 5 && rest[0] == 0x1C {
        let length = usize::from(u16::from_be_bytes([rest[3], rest[4]]));
        if length & 0x8000 != 0 || rest.len() < 5 + length {
            break;
        }
        datasets.push(IptcDataset {
            record: rest[1],
            dataset: rest[2],
            value: rest[5..5 + length].to_vec(),
        });
        rest = &rest[5 + length..];
    }
    datasets
}

fn serialise_iptc(datasets: &[IptcDataset]) -> Vec<u8> {
    let mut out = Vec::new();
    for dataset in datasets {
        out.extend_from_slice(&[0x1C, dataset.record, dataset.dataset]);
        out.extend_from_slice(&(dataset.value.len() as u16).to_be_bytes());
        out.extend_from_slice(&dataset.value);
    }
    out
}

/// Updates the IPTC-IIM datasets of an existing Photoshop APP13 block, if there is one.
fn edit_jpeg_iptc(segments: &mut [JpegSegment], edit: &ImageEdit) {
    let managed: [(u8, bool); 5] = [
        (5, !edit.title.is_keep()),
        (25, !edit.keywords.is_keep()),
        (80, !edit.author.is_keep()),
        (116, !edit.copyright.is_keep()),
        (120, !edit.description.is_keep()),
    ];
    if managed.iter().all(|(_, changed)| !changed) {
        return;
    }
    for segment in segments
        .iter_mut()
        .filter(|segment| segment.marker == 0xED && segment.payload.starts_with(PHOTOSHOP_HEADER))
    {
        let mut resources = Vec::new();
        let mut rest = &segment.payload[PHOTOSHOP_HEADER.len()..];
        let mut valid = true;
        let mut iptc_index: Option<usize> = None;
        while rest.len() >= 12 && rest.starts_with(b"8BIM") {
            let id = u16::from_be_bytes([rest[4], rest[5]]);
            let name_block = (1 + usize::from(rest[6])).next_multiple_of(2);
            let size_at = 6 + name_block;
            if rest.len() < size_at + 4 {
                valid = false;
                break;
            }
            let size = u32::from_be_bytes([
                rest[size_at],
                rest[size_at + 1],
                rest[size_at + 2],
                rest[size_at + 3],
            ]) as usize;
            let data_at = size_at + 4;
            if rest.len() < data_at + size {
                valid = false;
                break;
            }
            if id == 0x0404 {
                iptc_index = Some(resources.len());
            }
            resources.push((
                rest[..size_at].to_vec(),
                rest[data_at..data_at + size].to_vec(),
            ));
            rest = &rest[(data_at + size.next_multiple_of(2)).min(rest.len())..];
        }
        let Some(index) = iptc_index.filter(|_| valid) else {
            continue;
        };
        let mut datasets = parse_iptc(&resources[index].1);
        let charset = datasets
            .iter()
            .find(|dataset| dataset.record == 1 && dataset.dataset == 90)
            .map(|dataset| dataset.value.clone());
        let utf8 = charset
            .as_deref()
            .is_none_or(|value| value == IPTC_UTF8_CHARSET);
        datasets.retain(|dataset| {
            !(dataset.record == 2
                && managed
                    .iter()
                    .any(|(id, changed)| *changed && *id == dataset.dataset))
        });
        if utf8 {
            let mut add = |dataset: u8, value: &str| {
                datasets.push(IptcDataset {
                    record: 2,
                    dataset,
                    value: value.as_bytes().iter().copied().take(2000).collect(),
                });
            };
            if let Change::Set(value) = &edit.title {
                add(5, value);
            }
            if let Change::Set(value) = &edit.author {
                add(80, value);
            }
            if let Change::Set(value) = &edit.copyright {
                add(116, value);
            }
            if let Change::Set(value) = &edit.description {
                add(120, value);
            }
            if let Change::Set(values) = &edit.keywords {
                for value in values {
                    add(25, value);
                }
            }
            if charset.is_none() {
                datasets.push(IptcDataset {
                    record: 1,
                    dataset: 90,
                    value: IPTC_UTF8_CHARSET.to_vec(),
                });
            }
        }
        datasets.sort_by_key(|dataset| (dataset.record, dataset.dataset));
        resources[index].1 = serialise_iptc(&datasets);
        let mut payload = PHOTOSHOP_HEADER.to_vec();
        for (header, mut data) in resources {
            let mut header = header;
            // The stored header ends before the size field, which is rebuilt here.
            header.extend_from_slice(&(data.len() as u32).to_be_bytes());
            payload.extend_from_slice(&header);
            if data.len() % 2 == 1 {
                data.push(0);
            }
            payload.extend_from_slice(&data);
        }
        segment.payload = payload;
    }
}

/* ----------------------------------- entry ----------------------------------- */

/// Applies `edit` to the bytes of an image and returns the rewritten file.
///
/// # Errors
///
/// Fails without producing output when the format is unsupported, a requested field cannot be
/// stored in this format, a value is invalid, or the file structure is damaged.
pub fn apply_image_edit(bytes: &[u8], edit: &ImageEdit) -> Result<Vec<u8>, ImageEditError> {
    let format = detect_format(bytes);
    let allowed = editable_fields(format);
    let Some(format) = format.filter(|_| !allowed.is_empty()) else {
        return Err(ImageEditError::UnsupportedFormat);
    };
    validate(edit)?;
    for field in used_fields(edit) {
        if !allowed.contains(&field) {
            return Err(ImageEditError::UnsupportedField(field.id()));
        }
    }
    let file_type = file_extension(format);
    let existing_date = read_photo_info(bytes).date_taken;

    if edit.remove_all {
        let orientation = existing_orientation(bytes);
        let mut stripped = if format == "JPEG" {
            strip_jpeg_metadata(bytes)?
        } else {
            let mut buffer = bytes.to_vec();
            Metadata::clear_metadata(&mut buffer, file_type)
                .map_err(|error| ImageEditError::Writer(error.to_string()))?;
            buffer
        };
        if let Some(orientation) = orientation
            && orientation != 1
        {
            let mut metadata = Metadata::new();
            metadata.set_tag(ExifTag::Orientation(vec![orientation]));
            metadata
                .write_to_vec(&mut stripped, file_type)
                .map_err(|error| ImageEditError::Writer(error.to_string()))?;
        }
        return Ok(stripped);
    }

    let mut buffer = bytes.to_vec();
    let mut metadata =
        Metadata::new_from_vec(&buffer, file_type).unwrap_or_else(|_| Metadata::new());
    apply_exif(&mut metadata, edit, existing_date)?;
    metadata
        .write_to_vec(&mut buffer, file_type)
        .map_err(|error| ImageEditError::Writer(error.to_string()))?;

    if format == "JPEG" {
        let (mut segments, rest) = split_jpeg(&buffer)?;
        let changes = XmpChanges {
            title: &edit.title,
            description: &edit.description,
            rights: &edit.copyright,
            creator: &edit.author,
            keywords: &edit.keywords,
            rating: &edit.rating,
            strip_gps: edit.remove_location || matches!(edit.gps, Change::Clear),
        };
        edit_jpeg_xmp(&mut segments, &changes)?;
        edit_jpeg_iptc(&mut segments, edit);
        buffer = join_jpeg(&segments, rest)?;
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::image_tags::read_image_tags;

    fn encode(format: image::ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(9, 5)
            .write_to(&mut Cursor::new(&mut bytes), format)
            .expect("encode");
        bytes
    }

    /// A JPEG with an Exif Make tag, an existing XMP rating + GPS and an IPTC caption.
    fn rich_jpeg() -> Vec<u8> {
        let base = encode(image::ImageFormat::Jpeg);
        let mut with_make = base.clone();
        let mut metadata = Metadata::new();
        metadata.set_tag(ExifTag::Make("ACME".to_owned()));
        metadata.set_tag(ExifTag::Orientation(vec![6]));
        metadata
            .write_to_vec(&mut with_make, FileExtension::JPEG)
            .expect("write exif");

        let mut iptc = Vec::new();
        for (dataset, value) in [(120_u8, "old caption"), (15, "NEWS")] {
            iptc.extend_from_slice(&[0x1C, 2, dataset]);
            iptc.extend_from_slice(&(value.len() as u16).to_be_bytes());
            iptc.extend_from_slice(value.as_bytes());
        }
        let mut photoshop = PHOTOSHOP_HEADER.to_vec();
        photoshop.extend_from_slice(b"8BIM\x04\x04\0\0");
        photoshop.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
        photoshop.extend_from_slice(&iptc);
        if iptc.len() % 2 == 1 {
            photoshop.push(0);
        }
        let mut xmp = XMP_HEADER.to_vec();
        xmp.extend_from_slice(
            br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/" xmlns:tiff="http://ns.adobe.com/tiff/1.0/" xmp:Rating="1" exif:GPSLatitude="52,30.0N" tiff:Make="ACME"><exif:GPSLongitude>4,54.0E</exif:GPSLongitude></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#,
        );
        let (mut segments, rest) = split_jpeg(&with_make).expect("valid jpeg");
        let after_exif = segments.iter().position(is_exif).expect("exif segment") + 1;
        segments.insert(
            after_exif,
            JpegSegment {
                marker: 0xE1,
                payload: xmp,
            },
        );
        segments.insert(
            after_exif,
            JpegSegment {
                marker: 0xED,
                payload: photoshop,
            },
        );
        join_jpeg(&segments, rest).expect("join")
    }

    fn value(tags: &crate::image_tags::ImageTags, namespace: &str, name: &str) -> Option<String> {
        tags.groups
            .iter()
            .find(|group| group.namespace == namespace)?
            .tags
            .iter()
            .find(|tag| tag.name == name)
            .map(|tag| tag.value.clone())
    }

    fn scan_data(jpeg: &[u8]) -> &[u8] {
        let (_, rest) = split_jpeg(jpeg).expect("valid jpeg");
        rest
    }

    #[test]
    fn edits_jpeg_fields_losslessly_and_keeps_unrelated_tags() {
        let original = rich_jpeg();
        let edit = ImageEdit {
            date_taken: Change::Set(CaptureTime {
                year: 2020,
                month: 2,
                day: 29,
                hour: 23,
                minute: 59,
                second: 58,
            }),
            gps: Change::Set(GpsPosition {
                latitude: -33.8567,
                longitude: 151.2153,
            }),
            title: Change::Set("Sun & <sea>".into()),
            description: Change::Set("A caption".into()),
            copyright: Change::Set("(c) Me".into()),
            author: Change::Set("Erik".into()),
            keywords: Change::Set(vec!["alpha".into(), "beta".into()]),
            rating: Change::Set(4),
            ..ImageEdit::default()
        };
        let edited = apply_image_edit(&original, &edit).expect("edit");

        assert_eq!(
            scan_data(&edited),
            scan_data(&original),
            "pixel data untouched"
        );
        let tags = read_image_tags(&edited);
        assert_eq!(value(&tags, "EXIF", "Make").as_deref(), Some("ACME"));
        assert_eq!(
            value(&tags, "EXIF", "Orientation").as_deref(),
            Some("row 0 at right and column 0 at top")
        );
        assert!(
            value(&tags, "EXIF", "DateTimeOriginal")
                .is_some_and(|date| date.starts_with("2020-02-29 23:59:58"))
        );
        let gps = tags.gps.expect("gps");
        assert!((gps.latitude + 33.8567).abs() < 1e-4);
        assert!((gps.longitude - 151.2153).abs() < 1e-4);
        assert_eq!(
            value(&tags, "EXIF", "ImageDescription").as_deref(),
            Some("A caption")
        );
        assert_eq!(
            value(&tags, "XMP", "dc:title").as_deref(),
            Some("Sun & <sea>")
        );
        assert_eq!(
            value(&tags, "XMP", "dc:subject").as_deref(),
            Some("alpha, beta")
        );
        assert_eq!(value(&tags, "XMP", "xmp:Rating").as_deref(), Some("4"));
        assert_eq!(value(&tags, "XMP", "tiff:Make").as_deref(), Some("ACME"));
        assert_eq!(
            value(&tags, "IPTC", "Caption-Abstract").as_deref(),
            Some("A caption")
        );
        assert_eq!(value(&tags, "IPTC", "Category").as_deref(), Some("NEWS"));
        assert_eq!(value(&tags, "IPTC", "Keywords").as_deref(), Some("alpha"));
    }

    #[test]
    fn remove_location_strips_exif_and_xmp_coordinates_only() {
        let original = apply_image_edit(
            &rich_jpeg(),
            &ImageEdit {
                gps: Change::Set(GpsPosition {
                    latitude: 10.0,
                    longitude: 20.0,
                }),
                ..ImageEdit::default()
            },
        )
        .expect("add gps");
        assert!(read_image_tags(&original).gps.is_some());

        let edited = apply_image_edit(
            &original,
            &ImageEdit {
                remove_location: true,
                ..ImageEdit::default()
            },
        )
        .expect("strip");
        let tags = read_image_tags(&edited);
        assert!(tags.gps.is_none());
        assert!(tags.groups.iter().all(|group| group.namespace != "GPS"));
        assert_eq!(value(&tags, "XMP", "exif:GPSLatitude"), None);
        assert_eq!(value(&tags, "XMP", "exif:GPSLongitude"), None);
        assert_eq!(value(&tags, "XMP", "tiff:Make").as_deref(), Some("ACME"));
        assert_eq!(value(&tags, "EXIF", "Make").as_deref(), Some("ACME"));
        assert_eq!(scan_data(&edited), scan_data(&original));
    }

    #[test]
    fn clearing_a_field_removes_it_everywhere() {
        let edited = apply_image_edit(
            &rich_jpeg(),
            &ImageEdit {
                rating: Change::Clear,
                description: Change::Clear,
                ..ImageEdit::default()
            },
        )
        .expect("edit");
        let tags = read_image_tags(&edited);
        assert_eq!(value(&tags, "XMP", "xmp:Rating"), None);
        assert_eq!(value(&tags, "IPTC", "Caption-Abstract"), None);
        assert_eq!(value(&tags, "IPTC", "Category").as_deref(), Some("NEWS"));
    }

    #[test]
    fn remove_all_strips_everything_but_keeps_the_orientation() {
        let original = rich_jpeg();
        let edited = apply_image_edit(
            &original,
            &ImageEdit {
                remove_all: true,
                ..ImageEdit::default()
            },
        )
        .expect("strip");
        let tags = read_image_tags(&edited);
        assert_eq!(value(&tags, "EXIF", "Make"), None);
        assert!(
            tags.groups
                .iter()
                .all(|group| !matches!(group.namespace.as_str(), "XMP" | "IPTC" | "GPS"))
        );
        assert_eq!(
            value(&tags, "EXIF", "Orientation").as_deref(),
            Some("row 0 at right and column 0 at top")
        );
        assert_eq!(scan_data(&edited), scan_data(&original));
    }

    #[test]
    fn shifts_the_stored_capture_time() {
        let original = apply_image_edit(
            &encode(image::ImageFormat::Jpeg),
            &ImageEdit {
                date_taken: Change::Set(CaptureTime {
                    year: 2023,
                    month: 12,
                    day: 31,
                    hour: 23,
                    minute: 30,
                    second: 0,
                }),
                ..ImageEdit::default()
            },
        )
        .expect("set date");
        let edited = apply_image_edit(
            &original,
            &ImageEdit {
                shift_date_seconds: Some(3600),
                ..ImageEdit::default()
            },
        )
        .expect("shift");
        assert_eq!(
            read_photo_info(&edited).date_taken,
            Some(CaptureTime {
                year: 2024,
                month: 1,
                day: 1,
                hour: 0,
                minute: 30,
                second: 0
            })
        );
    }

    #[test]
    fn shift_capture_time_handles_leap_days_and_negatives() {
        let time = CaptureTime {
            year: 2024,
            month: 3,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
        };
        let back = shift_capture_time(time, -1).expect("shifted");
        assert_eq!(
            (back.month, back.day, back.hour, back.second),
            (2, 29, 23, 59)
        );
    }

    /// A 1x1 grey TIFF with the baseline tags and resolution the metadata writer requires.
    fn tiff() -> Vec<u8> {
        let data_start = 8 + 2 + 11 * 12 + 4;
        let entries: [(u16, u16, u32, u32); 11] = [
            (0x0100, 3, 1, 1),
            (0x0101, 3, 1, 1),
            (0x0102, 3, 1, 8),
            (0x0103, 3, 1, 1),
            (0x0106, 3, 1, 1),
            (0x0111, 4, 1, data_start as u32),
            (0x0116, 3, 1, 1),
            (0x0117, 4, 1, 1),
            (0x011A, 5, 1, data_start as u32 + 1),
            (0x011B, 5, 1, data_start as u32 + 9),
            (0x0128, 3, 1, 2),
        ];
        let mut t = b"II*\0".to_vec();
        t.extend_from_slice(&8_u32.to_le_bytes());
        t.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, kind, count, value) in entries {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&kind.to_le_bytes());
            t.extend_from_slice(&count.to_le_bytes());
            t.extend_from_slice(&value.to_le_bytes());
        }
        t.extend_from_slice(&0_u32.to_le_bytes());
        t.push(0x7F);
        for _ in 0..2 {
            t.extend_from_slice(&72_u32.to_le_bytes());
            t.extend_from_slice(&1_u32.to_le_bytes());
        }
        t
    }

    #[test]
    fn tiff_and_webp_support_exif_fields_only() {
        for (name, bytes) in [("TIFF", tiff()), ("WebP", encode(image::ImageFormat::WebP))] {
            let edited = apply_image_edit(
                &bytes,
                &ImageEdit {
                    description: Change::Set("hello".into()),
                    author: Change::Set("Erik".into()),
                    ..ImageEdit::default()
                },
            )
            .unwrap_or_else(|error| panic!("{name} edit: {error}"));
            let tags = read_image_tags(&edited);
            assert_eq!(
                value(&tags, "EXIF", "ImageDescription").as_deref(),
                Some("hello"),
                "{name}"
            );
            assert_eq!(
                value(&tags, "EXIF", "Artist").as_deref(),
                Some("Erik"),
                "{name}"
            );
            assert!(
                matches!(
                    apply_image_edit(
                        &bytes,
                        &ImageEdit {
                            title: Change::Set("x".into()),
                            ..ImageEdit::default()
                        }
                    ),
                    Err(ImageEditError::UnsupportedField("title"))
                ),
                "{name}"
            );
        }
    }

    #[test]
    fn png_is_reported_as_unsupported() {
        assert!(matches!(
            apply_image_edit(&encode(image::ImageFormat::Png), &ImageEdit::default()),
            Err(ImageEditError::UnsupportedFormat)
        ));
    }

    #[test]
    fn rejects_unsupported_formats_and_invalid_values() {
        assert!(matches!(
            apply_image_edit(b"plain text", &ImageEdit::default()),
            Err(ImageEditError::UnsupportedFormat)
        ));
        assert!(matches!(
            apply_image_edit(
                &rich_jpeg(),
                &ImageEdit {
                    rating: Change::Set(9),
                    ..ImageEdit::default()
                }
            ),
            Err(ImageEditError::InvalidValue(_))
        ));
        assert!(editable_fields(Some("HEIC")).is_empty());
    }
}
