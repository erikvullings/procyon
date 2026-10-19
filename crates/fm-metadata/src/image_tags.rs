//! Every readable metadata tag of an image, grouped by namespace (task 0273).
//!
//! Works on the leading bytes of a file, so it is provider-neutral and bounded. Covers EXIF and GPS
//! (JPEG, TIFF and TIFF-based RAW, PNG, WebP, HEIC/AVIF), IPTC-IIM (JPEG), XMP (any container,
//! found by its packet markers), ICC profile headers and PNG text chunks. Malformed input yields
//! fewer groups, never an error.

use std::io::{Cursor, Read};

use exif::{Context, In, Reader, Tag, Value};
use quick_xml::events::Event;

/// Longest raw value kept per tag.
const MAX_RAW_CHARS: usize = 512;
/// Longest decompressed PNG text chunk kept.
const MAX_INFLATED_BYTES: u64 = 64 * 1024;

/// One metadata tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageTag {
    /// Tag name, for example `Make` or `dc:title`.
    pub name: String,
    /// Human-readable value.
    pub value: String,
    /// The raw stored value when it differs from [`Self::value`].
    pub raw: Option<String>,
}

/// Tags of one namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageTagGroup {
    /// Namespace such as `EXIF`, `GPS`, `IPTC`, `XMP`, `ICC` or `File`.
    pub namespace: String,
    /// The tags, in file order.
    pub tags: Vec<ImageTag>,
}

/// A decimal GPS position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsPosition {
    /// Degrees north (negative is south).
    pub latitude: f64,
    /// Degrees east (negative is west).
    pub longitude: f64,
}

/// Everything found in an image's metadata.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageTags {
    /// Container format detected from the magic bytes, or `None` when not a known image.
    pub format: Option<&'static str>,
    /// Tag groups; only non-empty groups are present.
    pub groups: Vec<ImageTagGroup>,
    /// GPS position when the EXIF GPS tags are complete.
    pub gps: Option<GpsPosition>,
}

/// Detects the container format from the magic bytes.
#[must_use]
pub fn detect_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        Some("JPEG")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("PNG")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("WebP")
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some("TIFF")
    } else if bytes.starts_with(b"GIF8") {
        Some("GIF")
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        match &bytes[8..12] {
            b"avif" | b"avis" => Some("AVIF"),
            b"heic" | b"heix" | b"hevc" | b"mif1" | b"msf1" => Some("HEIC"),
            _ => None,
        }
    } else {
        None
    }
}

/// Reads every tag from the leading bytes of an image file.
#[must_use]
pub fn read_image_tags(bytes: &[u8]) -> ImageTags {
    let mut tags = ImageTags {
        format: detect_format(bytes),
        ..ImageTags::default()
    };
    if tags.format.is_none() {
        return tags;
    }
    let mut groups: Vec<ImageTagGroup> = Vec::new();
    if let Ok(exif) = Reader::new().read_from_container(&mut Cursor::new(bytes)) {
        for field in exif.fields() {
            let namespace = match (field.ifd_num, field.tag.context()) {
                (_, Context::Gps) => "GPS",
                (In::THUMBNAIL, _) => "EXIF thumbnail",
                _ => "EXIF",
            };
            let value = match &field.value {
                Value::Undefined(data, _) if data.len() > 64 => format!("{} bytes", data.len()),
                _ => field.display_value().with_unit(&exif).to_string(),
            };
            let raw = raw_of(&field.value);
            push(
                &mut groups,
                namespace,
                field.tag.to_string(),
                value,
                Some(raw),
            );
        }
        tags.gps = gps_of(&exif);
    }
    let format = tags.format;
    if format == Some("JPEG") {
        for segment in jpeg_segments(bytes) {
            if segment.marker == 0xED {
                read_iptc(segment.data, &mut groups);
            }
        }
        if let Some(icc) = jpeg_icc_profile(bytes) {
            read_icc(&icc, &mut groups);
        }
    }
    if format == Some("PNG") {
        read_png_chunks(bytes, &mut groups);
    }
    read_xmp(bytes, &mut groups);
    tags.groups = groups;
    tags
}

fn push(
    groups: &mut Vec<ImageTagGroup>,
    namespace: &str,
    name: String,
    value: String,
    raw: Option<String>,
) {
    let raw = raw.filter(|raw| *raw != value);
    let tag = ImageTag { name, value, raw };
    match groups.iter_mut().find(|group| group.namespace == namespace) {
        Some(group) => group.tags.push(tag),
        None => groups.push(ImageTagGroup {
            namespace: namespace.to_owned(),
            tags: vec![tag],
        }),
    }
}

fn raw_of(value: &Value) -> String {
    let mut text = match value {
        Value::Ascii(parts) => parts
            .iter()
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>()
            .join(", "),
        other => format!("{other:?}"),
    };
    if text.chars().count() > MAX_RAW_CHARS {
        text = text.chars().take(MAX_RAW_CHARS).collect::<String>() + "...";
    }
    text
}

fn gps_of(exif: &exif::Exif) -> Option<GpsPosition> {
    let coordinate = |tag: Tag, reference: Tag, negative: &str| -> Option<f64> {
        let field = exif.get_field(tag, In::PRIMARY)?;
        let Value::Rational(parts) = &field.value else {
            return None;
        };
        if parts.len() < 3 {
            return None;
        }
        let value = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
        let reference = exif
            .get_field(reference, In::PRIMARY)
            .map(|field| field.display_value().to_string())
            .unwrap_or_default();
        let value = if reference.trim_matches('"').eq_ignore_ascii_case(negative) {
            -value
        } else {
            value
        };
        value.is_finite().then_some(value)
    };
    Some(GpsPosition {
        latitude: coordinate(Tag::GPSLatitude, Tag::GPSLatitudeRef, "S")?,
        longitude: coordinate(Tag::GPSLongitude, Tag::GPSLongitudeRef, "W")?,
    })
}

struct Segment<'a> {
    marker: u8,
    data: &'a [u8],
}

/// Application segments (APP0-APP15) before the image data.
fn jpeg_segments(bytes: &[u8]) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut offset = 2;
    while offset + 4 <= bytes.len() && bytes[offset] == 0xFF {
        let marker = bytes[offset + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let length = usize::from(u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]));
        if length < 2 || offset + 2 + length > bytes.len() {
            break;
        }
        if (0xE0..=0xEF).contains(&marker) {
            segments.push(Segment {
                marker,
                data: &bytes[offset + 4..offset + 2 + length],
            });
        }
        offset += 2 + length;
    }
    segments
}

fn iptc_name(record: u8, dataset: u8) -> String {
    let name = match (record, dataset) {
        (2, 5) => "ObjectName",
        (2, 15) => "Category",
        (2, 20) => "SupplementalCategory",
        (2, 25) => "Keywords",
        (2, 40) => "SpecialInstructions",
        (2, 55) => "DateCreated",
        (2, 60) => "TimeCreated",
        (2, 80) => "By-line",
        (2, 85) => "By-lineTitle",
        (2, 90) => "City",
        (2, 92) => "Sub-location",
        (2, 95) => "Province-State",
        (2, 101) => "Country",
        (2, 103) => "OriginalTransmissionReference",
        (2, 105) => "Headline",
        (2, 110) => "Credit",
        (2, 115) => "Source",
        (2, 116) => "CopyrightNotice",
        (2, 120) => "Caption-Abstract",
        (2, 122) => "Writer-Editor",
        _ => return format!("{record}:{dataset}"),
    };
    name.to_owned()
}

/// Reads IPTC-IIM datasets from a Photoshop APP13 segment.
fn read_iptc(segment: &[u8], groups: &mut Vec<ImageTagGroup>) {
    let Some(mut rest) = segment.strip_prefix(b"Photoshop 3.0\0") else {
        return;
    };
    while rest.len() >= 12 && rest.starts_with(b"8BIM") {
        let id = u16::from_be_bytes([rest[4], rest[5]]);
        let name_length = usize::from(rest[6]);
        let name_block = (1 + name_length).next_multiple_of(2);
        let size_at = 6 + name_block;
        if rest.len() < size_at + 4 {
            return;
        }
        let size = u32::from_be_bytes([
            rest[size_at],
            rest[size_at + 1],
            rest[size_at + 2],
            rest[size_at + 3],
        ]) as usize;
        let data_at = size_at + 4;
        if rest.len() < data_at + size {
            return;
        }
        if id == 0x0404 {
            read_iptc_datasets(&rest[data_at..data_at + size], groups);
        }
        rest = &rest[(data_at + size.next_multiple_of(2)).min(rest.len())..];
    }
}

fn read_iptc_datasets(mut data: &[u8], groups: &mut Vec<ImageTagGroup>) {
    while data.len() >= 5 && data[0] == 0x1C {
        let (record, dataset) = (data[1], data[2]);
        let length = usize::from(u16::from_be_bytes([data[3], data[4]]));
        if length & 0x8000 != 0 || data.len() < 5 + length {
            return;
        }
        let value = String::from_utf8_lossy(&data[5..5 + length])
            .trim_end_matches('\0')
            .to_owned();
        if record == 2 && !value.is_empty() {
            push(groups, "IPTC", iptc_name(record, dataset), value, None);
        }
        data = &data[5 + length..];
    }
}

/// The ICC profile of a JPEG, reassembled from its APP2 chunks.
fn jpeg_icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut chunks: Vec<(u8, &[u8])> = jpeg_segments(bytes)
        .into_iter()
        .filter(|segment| segment.marker == 0xE2)
        .filter_map(|segment| {
            let rest = segment.data.strip_prefix(b"ICC_PROFILE\0")?;
            Some((*rest.first()?, rest.get(2..)?))
        })
        .collect();
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|(sequence, _)| *sequence);
    Some(
        chunks
            .into_iter()
            .flat_map(|(_, data)| data.iter().copied())
            .collect(),
    )
}

fn four_cc(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

/// Reads the header and description of an ICC profile.
fn read_icc(profile: &[u8], groups: &mut Vec<ImageTagGroup>) {
    if profile.len() < 132 || &profile[36..40] != b"acsp" {
        return;
    }
    let size = u32::from_be_bytes([profile[0], profile[1], profile[2], profile[3]]);
    let mut add = |name: &str, value: String| push(groups, "ICC", name.to_owned(), value, None);
    if let Some(description) = icc_description(profile) {
        add("Description", description);
    }
    add("Profile size", format!("{size} bytes"));
    add("Version", format!("{}.{}", profile[8], profile[9] >> 4));
    add("Device class", four_cc(&profile[12..16]));
    add("Color space", four_cc(&profile[16..20]));
    add("Connection space", four_cc(&profile[20..24]));
}

fn icc_description(profile: &[u8]) -> Option<String> {
    let count = u32::from_be_bytes(profile.get(128..132)?.try_into().ok()?) as usize;
    for index in 0..count.min(100) {
        let entry = profile.get(132 + index * 12..144 + index * 12)?;
        if &entry[..4] != b"desc" {
            continue;
        }
        let offset = u32::from_be_bytes(entry[4..8].try_into().ok()?) as usize;
        let size = u32::from_be_bytes(entry[8..12].try_into().ok()?) as usize;
        let tag = profile.get(offset..offset.checked_add(size)?)?;
        return match tag.get(..4)? {
            b"desc" => {
                let length = u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?) as usize;
                let text = tag.get(12..12 + length)?;
                Some(
                    String::from_utf8_lossy(text)
                        .trim_end_matches('\0')
                        .to_owned(),
                )
            }
            b"mluc" => {
                let length = u32::from_be_bytes(tag.get(20..24)?.try_into().ok()?) as usize;
                let at = u32::from_be_bytes(tag.get(24..28)?.try_into().ok()?) as usize;
                let (pairs, _) = tag.get(at..at.checked_add(length)?)?.as_chunks::<2>();
                let units: Vec<u16> = pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
                Some(String::from_utf16_lossy(&units))
            }
            _ => None,
        };
    }
    None
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .take(MAX_INFLATED_BYTES)
        .read_to_end(&mut output)
        .ok()?;
    Some(output)
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// Reads the header, text chunks and colour profile of a PNG.
fn read_png_chunks(bytes: &[u8], groups: &mut Vec<ImageTagGroup>) {
    let mut offset = 8;
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let kind = &bytes[offset + 4..offset + 8];
        let Some(data) = bytes.get(offset + 8..offset + 8 + length) else {
            return;
        };
        match kind {
            b"IHDR" if data.len() >= 13 => {
                let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                push(
                    groups,
                    "File",
                    "Dimensions".into(),
                    format!("{width} x {height}"),
                    None,
                );
                push(
                    groups,
                    "File",
                    "Bit depth".into(),
                    data[8].to_string(),
                    None,
                );
            }
            b"tEXt" => {
                if let Some((key, text)) = split_nul(data) {
                    push(groups, "PNG", latin1(key), latin1(text), None);
                }
            }
            b"zTXt" => {
                if let Some((key, rest)) = split_nul(data)
                    && let Some(text) = rest.get(1..).and_then(inflate)
                {
                    push(groups, "PNG", latin1(key), latin1(&text), None);
                }
            }
            b"iTXt" => {
                if let Some((key, rest)) = split_nul(data)
                    && key != b"XML:com.adobe.xmp"
                    && rest.len() >= 2
                {
                    let compressed = rest[0] == 1;
                    let after_flags = &rest[2..];
                    let text = split_nul(after_flags)
                        .and_then(|(_, tail)| split_nul(tail))
                        .map(|(_, text)| text);
                    let text = match (text, compressed) {
                        (Some(text), false) => Some(text.to_vec()),
                        (Some(text), true) => inflate(text),
                        (None, _) => None,
                    };
                    if let Some(text) = text {
                        push(
                            groups,
                            "PNG",
                            latin1(key),
                            String::from_utf8_lossy(&text).into_owned(),
                            None,
                        );
                    }
                }
            }
            b"iCCP" => {
                if let Some((_, rest)) = split_nul(data)
                    && let Some(profile) = rest.get(1..).and_then(inflate)
                {
                    read_icc(&profile, groups);
                }
            }
            b"IDAT" => return,
            _ => {}
        }
        offset += 12 + length;
    }
}

fn split_nul(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = data.iter().position(|byte| *byte == 0)?;
    Some((&data[..at], &data[at + 1..]))
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

/// Finds the XMP packet by its markers, so it works in every container.
fn xmp_packet(bytes: &[u8]) -> Option<&[u8]> {
    let start = find(bytes, b"<x:xmpmeta", 0)?;
    let end_marker = b"</x:xmpmeta>";
    let end = find(bytes, end_marker, start)? + end_marker.len();
    Some(&bytes[start..end])
}

fn read_xmp(bytes: &[u8], groups: &mut Vec<ImageTagGroup>) {
    let Some(packet) = xmp_packet(bytes) else {
        return;
    };
    let Ok(text) = std::str::from_utf8(packet) else {
        return;
    };
    let mut reader = quick_xml::Reader::from_str(text);
    let mut depth = 0_usize;
    // (name, collected values) of the property being read, at depth 3 below x:xmpmeta/RDF/Description.
    let mut property: Option<(String, Vec<String>)> = None;
    let mut properties: Vec<(String, Vec<String>)> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(event @ (Event::Start(_) | Event::Empty(_))) => {
                let is_start = matches!(event, Event::Start(_));
                let (Event::Start(start) | Event::Empty(start)) = event else {
                    continue;
                };
                let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
                if is_start
                    && name == "rdf:li"
                    && let Some((_, values)) = property.as_mut()
                    && values.last().is_none_or(|last| !last.is_empty())
                {
                    values.push(String::new());
                }
                if name == "rdf:Description" && depth == 2 {
                    for attribute in start.attributes().flatten() {
                        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
                        if key.starts_with("xmlns") || key == "rdf:about" {
                            continue;
                        }
                        if let Ok(value) = attribute.decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        ) {
                            properties.push((key, vec![value.into_owned()]));
                        }
                    }
                } else if is_start && depth == 3 && property.is_none() && !name.starts_with("rdf:")
                {
                    property = Some((name, vec![String::new()]));
                }
                if is_start {
                    depth += 1;
                }
            }
            Ok(Event::Text(text)) => {
                if let (Some((_, values)), Ok(text)) = (property.as_mut(), text.decode())
                    && let Some(last) = values.last_mut()
                {
                    last.push_str(&text);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let (Some((_, values)), Ok(name)) = (property.as_mut(), reference.decode())
                    && let Some(last) = values.last_mut()
                {
                    last.push_str(match name.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 3
                    && let Some((name, values)) = property.take()
                {
                    let values: Vec<String> = values
                        .into_iter()
                        .map(|value| value.trim().to_owned())
                        .filter(|value| !value.is_empty())
                        .collect();
                    if !values.is_empty() {
                        properties.push((name, values));
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    for (name, values) in properties {
        push(groups, "XMP", name, values.join(", "), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group<'a>(tags: &'a ImageTags, namespace: &str) -> Option<&'a ImageTagGroup> {
        tags.groups
            .iter()
            .find(|group| group.namespace == namespace)
    }

    fn segment(marker: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, marker];
        out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn exif_tiff() -> Vec<u8> {
        // Little-endian TIFF: IFD0 {Make "ACME"}, GPS IFD {LatRef S, Lat 10/20/30, LonRef E, Lon 5/0/0}.
        let mut t = Vec::new();
        t.extend_from_slice(b"II*\0");
        t.extend_from_slice(&8_u32.to_le_bytes());
        let entry = |t: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32| {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&kind.to_le_bytes());
            t.extend_from_slice(&count.to_le_bytes());
            t.extend_from_slice(&value.to_le_bytes());
        };
        // IFD0 at 8: 2 entries = 2 + 24 + 4 = 30 bytes -> ends at 38.
        t.extend_from_slice(&2_u16.to_le_bytes());
        entry(&mut t, 0x010F, 2, 4, u32::from_le_bytes(*b"ACM\0"));
        entry(&mut t, 0x8825, 4, 1, 38);
        t.extend_from_slice(&0_u32.to_le_bytes());
        // GPS IFD at 38: 4 entries = 2 + 48 + 4 = 54 -> ends at 92; rationals at 92 and 116.
        t.extend_from_slice(&4_u16.to_le_bytes());
        entry(&mut t, 1, 2, 2, u32::from_le_bytes([b'S', 0, 0, 0]));
        entry(&mut t, 2, 5, 3, 92);
        entry(&mut t, 3, 2, 2, u32::from_le_bytes([b'E', 0, 0, 0]));
        entry(&mut t, 4, 5, 3, 116);
        t.extend_from_slice(&0_u32.to_le_bytes());
        for (numerator, denominator) in [(10_u32, 1_u32), (20, 1), (30, 1), (5, 1), (0, 1), (0, 1)]
        {
            t.extend_from_slice(&numerator.to_le_bytes());
            t.extend_from_slice(&denominator.to_le_bytes());
        }
        t
    }

    fn jpeg() -> Vec<u8> {
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&exif_tiff());
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(segment(0xE1, &app1));

        let mut iptc = Vec::new();
        for (dataset, value) in [(25_u8, "alpha"), (25, "beta"), (116, "(c) Me")] {
            iptc.extend_from_slice(&[0x1C, 2, dataset]);
            iptc.extend_from_slice(&(value.len() as u16).to_be_bytes());
            iptc.extend_from_slice(value.as_bytes());
        }
        let mut photoshop = b"Photoshop 3.0\0".to_vec();
        photoshop.extend_from_slice(b"8BIM\x04\x04\0\0");
        photoshop.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
        photoshop.extend_from_slice(&iptc);
        if iptc.len() % 2 == 1 {
            photoshop.push(0);
        }
        jpeg.extend(segment(0xED, &photoshop));

        let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp.extend_from_slice(
            br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmp:Rating="4"><dc:title><rdf:Alt><rdf:li xml:lang="x-default">Sun &amp; sea</rdf:li></rdf:Alt></dc:title><dc:subject><rdf:Bag><rdf:li>a</rdf:li><rdf:li>b</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#,
        );
        jpeg.extend(segment(0xE1, &xmp));
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn reads_exif_gps_iptc_and_xmp_from_a_jpeg() {
        let tags = read_image_tags(&jpeg());
        assert_eq!(tags.format, Some("JPEG"));
        let exif = group(&tags, "EXIF").expect("exif group");
        assert!(exif.tags.iter().any(|tag| tag.name == "Make"));
        assert!(group(&tags, "GPS").is_some());
        let position = tags.gps.expect("gps");
        assert!((position.latitude + (10.0 + 20.0 / 60.0 + 30.0 / 3600.0)).abs() < 1e-9);
        assert!((position.longitude - 5.0).abs() < 1e-9);

        let iptc = group(&tags, "IPTC").expect("iptc group");
        let keywords: Vec<_> = iptc
            .tags
            .iter()
            .filter(|tag| tag.name == "Keywords")
            .map(|tag| tag.value.as_str())
            .collect();
        assert_eq!(keywords, ["alpha", "beta"]);
        assert!(
            iptc.tags
                .iter()
                .any(|tag| tag.name == "CopyrightNotice" && tag.value == "(c) Me")
        );

        let xmp = group(&tags, "XMP").expect("xmp group");
        let value = |name: &str| {
            xmp.tags
                .iter()
                .find(|tag| tag.name == name)
                .map(|tag| tag.value.as_str())
        };
        assert_eq!(value("xmp:Rating"), Some("4"));
        assert_eq!(value("dc:title"), Some("Sun & sea"));
        assert_eq!(value("dc:subject"), Some("a, b"));
    }

    #[test]
    fn reads_png_text_chunks_and_dimensions() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let chunk = |png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            png.extend_from_slice(&[0; 4]);
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&7_u32.to_be_bytes());
        ihdr.extend_from_slice(&3_u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"tEXt", b"Author\0Erik");
        let tags = read_image_tags(&png);
        assert_eq!(tags.format, Some("PNG"));
        let file = group(&tags, "File").expect("file group");
        assert_eq!(file.tags[0].value, "7 x 3");
        let text = group(&tags, "PNG").expect("png group");
        assert_eq!(text.tags[0].name, "Author");
        assert_eq!(text.tags[0].value, "Erik");
    }

    #[test]
    fn unknown_formats_report_no_format() {
        let tags = read_image_tags(b"definitely not an image");
        assert_eq!(tags.format, None);
        assert!(tags.groups.is_empty());
    }

    #[test]
    fn truncated_input_never_panics() {
        let full = jpeg();
        for length in 0..full.len() {
            let _ = read_image_tags(&full[..length]);
        }
    }
}
