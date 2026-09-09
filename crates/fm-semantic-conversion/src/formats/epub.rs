//! EPUB package conversion through its declared OPF manifest and spine.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::builder::DocumentBuilder;
use crate::formats::html::{self, HtmlContext};
use crate::formats::package::{self, Package, PackageError};
use crate::text::decode;

const MAX_METADATA_PART_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CHAPTER_PART_BYTES: u64 = 64 * 1024 * 1024;
const CONTAINER_PART: &str = "META-INF/container.xml";
const ENCRYPTION_PART: &str = "META-INF/encryption.xml";

#[derive(Debug)]
struct ManifestItem {
    href: String,
    media_type: String,
    navigation: bool,
}

#[derive(Debug)]
struct PackageDocument {
    manifest: HashMap<String, ManifestItem>,
    spine: Vec<String>,
}

#[derive(Debug)]
struct SpineResource {
    spine_index: u32,
    path: String,
}

pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    archive: &mut Package<'_>,
) -> Result<(), PackageError> {
    let part_names = package::part_names(archive);
    let container = required_part(archive, CONTAINER_PART, MAX_METADATA_PART_BYTES)?;
    let rootfile = parse_container(builder, &container)?;
    let opf = required_part(archive, &rootfile, MAX_METADATA_PART_BYTES)?;
    let package_document = parse_package_document(builder, &opf, &rootfile)?;
    builder
        .tracker()
        .charge_items(package_document.spine.len() as u64)
        .map_err(PackageError::from)?;

    let opf_directory = rootfile
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let spine_resources = readable_spine_resources(builder, &package_document, opf_directory)?;
    if spine_resources.is_empty() {
        return Err(PackageError::Malformed(
            "the EPUB spine contains no readable XHTML or HTML resources".to_owned(),
        ));
    }

    if part_names.iter().any(|name| name == ENCRYPTION_PART) {
        let encryption = required_part(archive, ENCRYPTION_PART, MAX_METADATA_PART_BYTES)?;
        let targets = parse_encryption(builder, &encryption)?;
        for target in &targets {
            if !part_names.iter().any(|name| name == target) {
                return Err(PackageError::Malformed(format!(
                    "EPUB encryption metadata references missing part '{target}'"
                )));
            }
        }
        if let Some(resource) = spine_resources
            .iter()
            .find(|resource| targets.contains(&resource.path))
        {
            return Err(PackageError::Encrypted(format!(
                "EPUB spine resource '{}' is encrypted",
                resource.path
            )));
        }
    }

    for resource in spine_resources {
        builder.checkpoint().map_err(PackageError::from)?;
        if builder.is_saturated() {
            break;
        }
        let chapter = required_part(archive, &resource.path, MAX_CHAPTER_PART_BYTES)?;
        let decoded = decode(&chapter, None);
        if decoded.lossy {
            return Err(PackageError::Malformed(format!(
                "EPUB spine resource '{}' is not valid text",
                resource.path
            )));
        }
        html::convert_with_context(
            builder,
            &decoded.text,
            HtmlContext::EpubSpine {
                spine_index: resource.spine_index,
            },
        )
        .map_err(PackageError::from)?;
    }
    Ok(())
}

fn readable_spine_resources(
    builder: &DocumentBuilder<'_, '_>,
    package: &PackageDocument,
    opf_directory: &str,
) -> Result<Vec<SpineResource>, PackageError> {
    let mut resources = Vec::new();
    for (spine_index, idref) in package.spine.iter().enumerate() {
        builder.checkpoint().map_err(PackageError::from)?;
        let item = package.manifest.get(idref).ok_or_else(|| {
            PackageError::Malformed(format!(
                "EPUB spine references missing manifest item '{idref}'"
            ))
        })?;
        if item.navigation
            || !matches!(
                item.media_type.as_str(),
                "application/xhtml+xml" | "text/html"
            )
        {
            continue;
        }
        resources.push(SpineResource {
            spine_index: spine_index as u32,
            path: resolve_part_path(opf_directory, &item.href)?,
        });
    }
    Ok(resources)
}

fn required_part(
    archive: &mut Package<'_>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, PackageError> {
    package::read_part(archive, name, limit)?
        .ok_or_else(|| PackageError::Malformed(format!("the EPUB package has no '{name}' part")))
}

fn parse_container(
    builder: &mut DocumentBuilder<'_, '_>,
    bytes: &[u8],
) -> Result<String, PackageError> {
    let mut reader = package::xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut depth = 0_u32;
    let mut rootfile: Option<String> = None;
    loop {
        builder.checkpoint().map_err(PackageError::from)?;
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            PackageError::Malformed(format!("{CONTAINER_PART} is not well formed: {error}"))
        })?;
        package::inspect_event(&event, &mut depth, builder.tracker())?;
        match &event {
            Event::Start(start) | Event::Empty(start)
                if package::local_name(start.name().as_ref()) == "rootfile" =>
            {
                let path =
                    attribute(&reader, start, "full-path", CONTAINER_PART)?.ok_or_else(|| {
                        PackageError::Malformed("EPUB rootfile has no full-path".to_owned())
                    })?;
                let path = normalize_package_path(&path)?;
                if rootfile.is_none() {
                    rootfile = Some(path);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    package::ensure_balanced(depth, CONTAINER_PART)?;
    rootfile.ok_or_else(|| {
        PackageError::Malformed("EPUB container declares no OPF rootfile".to_owned())
    })
}

fn parse_package_document(
    builder: &mut DocumentBuilder<'_, '_>,
    bytes: &[u8],
    name: &str,
) -> Result<PackageDocument, PackageError> {
    let mut reader = package::xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut depth = 0_u32;
    let mut in_manifest = false;
    let mut in_spine = false;
    let mut manifest = HashMap::new();
    let mut spine = Vec::new();
    loop {
        builder.checkpoint().map_err(PackageError::from)?;
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            PackageError::Malformed(format!("{name} is not well formed: {error}"))
        })?;
        package::inspect_event(&event, &mut depth, builder.tracker())?;
        match &event {
            Event::Start(start) => {
                let local = package::local_name(start.name().as_ref());
                if local == "manifest" {
                    in_manifest = true;
                } else if local == "spine" {
                    in_spine = true;
                } else {
                    parse_opf_entry(
                        &reader,
                        start,
                        &local,
                        in_manifest,
                        in_spine,
                        name,
                        &mut manifest,
                        &mut spine,
                    )?;
                }
            }
            Event::Empty(start) => {
                let local = package::local_name(start.name().as_ref());
                parse_opf_entry(
                    &reader,
                    start,
                    &local,
                    in_manifest,
                    in_spine,
                    name,
                    &mut manifest,
                    &mut spine,
                )?;
            }
            Event::End(end) => match package::local_name(end.name().as_ref()).as_str() {
                "manifest" => in_manifest = false,
                "spine" => in_spine = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    package::ensure_balanced(depth, name)?;
    if manifest.is_empty() {
        return Err(PackageError::Malformed(
            "the EPUB OPF manifest is empty".to_owned(),
        ));
    }
    if spine.is_empty() {
        return Err(PackageError::Malformed(
            "the EPUB OPF spine is empty".to_owned(),
        ));
    }
    Ok(PackageDocument { manifest, spine })
}

#[allow(clippy::too_many_arguments)]
fn parse_opf_entry(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    local: &str,
    in_manifest: bool,
    in_spine: bool,
    name: &str,
    manifest: &mut HashMap<String, ManifestItem>,
    spine: &mut Vec<String>,
) -> Result<(), PackageError> {
    if local == "item" && in_manifest {
        let id = required_attribute(reader, start, "id", name)?;
        let href = required_attribute(reader, start, "href", name)?;
        let media_type = required_attribute(reader, start, "media-type", name)?
            .trim()
            .to_ascii_lowercase();
        let navigation = attribute(reader, start, "properties", name)?
            .is_some_and(|properties| properties.split_whitespace().any(|value| value == "nav"));
        if manifest
            .insert(
                id.clone(),
                ManifestItem {
                    href,
                    media_type,
                    navigation,
                },
            )
            .is_some()
        {
            return Err(PackageError::Malformed(format!(
                "EPUB manifest contains duplicate id '{id}'"
            )));
        }
    } else if local == "itemref" && in_spine {
        spine.push(required_attribute(reader, start, "idref", name)?);
    }
    Ok(())
}

fn required_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    wanted: &str,
    part: &str,
) -> Result<String, PackageError> {
    let value = attribute(reader, start, wanted, part)?.ok_or_else(|| {
        PackageError::Malformed(format!(
            "EPUB element '{}' in '{part}' has no '{wanted}' attribute",
            package::local_name(start.name().as_ref())
        ))
    })?;
    if value.trim().is_empty() {
        return Err(PackageError::Malformed(format!(
            "EPUB element '{}' in '{part}' has an empty '{wanted}' attribute",
            package::local_name(start.name().as_ref())
        )));
    }
    Ok(value)
}

fn attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    wanted: &str,
    part: &str,
) -> Result<Option<String>, PackageError> {
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| {
            PackageError::Malformed(format!("{part} has an invalid attribute: {error}"))
        })?;
        if package::local_name(attribute.key.as_ref()) == wanted {
            let value = attribute
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|error| {
                    PackageError::Malformed(format!(
                        "{part} has an invalid '{wanted}' attribute: {error}"
                    ))
                })?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn parse_encryption(
    builder: &mut DocumentBuilder<'_, '_>,
    bytes: &[u8],
) -> Result<Vec<String>, PackageError> {
    let mut reader = package::xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut depth = 0_u32;
    let mut in_encrypted_data = false;
    let mut references_in_current = 0_u32;
    let mut encrypted_data_count = 0_u32;
    let mut targets = Vec::new();
    loop {
        builder.checkpoint().map_err(PackageError::from)?;
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            PackageError::Malformed(format!("{ENCRYPTION_PART} is not well formed: {error}"))
        })?;
        package::inspect_event(&event, &mut depth, builder.tracker())?;
        match &event {
            Event::Start(start) => match package::local_name(start.name().as_ref()).as_str() {
                "EncryptedData" => {
                    if in_encrypted_data {
                        return Err(PackageError::Malformed(
                            "EPUB encryption metadata nests EncryptedData elements".to_owned(),
                        ));
                    }
                    in_encrypted_data = true;
                    references_in_current = 0;
                    encrypted_data_count += 1;
                }
                "CipherReference" => parse_cipher_reference(
                    &reader,
                    start,
                    in_encrypted_data,
                    &mut references_in_current,
                    &mut targets,
                )?,
                _ => {}
            },
            Event::Empty(start) => match package::local_name(start.name().as_ref()).as_str() {
                "EncryptedData" => {
                    return Err(PackageError::Malformed(
                        "EPUB EncryptedData has no CipherReference".to_owned(),
                    ));
                }
                "CipherReference" => parse_cipher_reference(
                    &reader,
                    start,
                    in_encrypted_data,
                    &mut references_in_current,
                    &mut targets,
                )?,
                _ => {}
            },
            Event::End(end) if package::local_name(end.name().as_ref()) == "EncryptedData" => {
                if !in_encrypted_data || references_in_current == 0 {
                    return Err(PackageError::Malformed(
                        "EPUB EncryptedData has no CipherReference".to_owned(),
                    ));
                }
                in_encrypted_data = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    package::ensure_balanced(depth, ENCRYPTION_PART)?;
    if in_encrypted_data || encrypted_data_count == 0 {
        return Err(PackageError::Malformed(
            "EPUB encryption metadata contains no complete EncryptedData entry".to_owned(),
        ));
    }
    Ok(targets)
}

fn parse_cipher_reference(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    in_encrypted_data: bool,
    references_in_current: &mut u32,
    targets: &mut Vec<String>,
) -> Result<(), PackageError> {
    if !in_encrypted_data {
        return Err(PackageError::Malformed(
            "EPUB CipherReference is outside EncryptedData".to_owned(),
        ));
    }
    let uri = required_attribute(reader, start, "URI", ENCRYPTION_PART)?;
    targets.push(normalize_encryption_target(&uri)?);
    *references_in_current += 1;
    Ok(())
}

fn resolve_part_path(base: &str, href: &str) -> Result<String, PackageError> {
    let decoded = decode_percent_escapes(href)?;
    let path = decoded
        .split_once('#')
        .map_or(decoded.as_str(), |(path, _)| path);
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', '?', ':']) {
        return Err(PackageError::Malformed(format!(
            "EPUB resource reference '{href}' is not a local package path"
        )));
    }
    let joined = if base.is_empty() {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    };
    normalize_decoded_package_path(&joined)
}

fn normalize_package_path(path: &str) -> Result<String, PackageError> {
    let decoded = decode_percent_escapes(path)?;
    normalize_decoded_package_path(&decoded)
}

fn normalize_encryption_target(uri: &str) -> Result<String, PackageError> {
    let decoded = decode_percent_escapes(uri)?;
    if decoded.contains(['?', '#']) {
        return Err(PackageError::Malformed(format!(
            "EPUB encryption target '{uri}' is not a package part"
        )));
    }
    normalize_decoded_package_path(&decoded)
}

fn normalize_decoded_package_path(path: &str) -> Result<String, PackageError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', ':', '?', '#'])
        || path.chars().any(char::is_control)
    {
        return Err(PackageError::Malformed(format!(
            "EPUB package path '{path}' is unsafe"
        )));
    }
    let mut normalized = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                return Err(PackageError::Malformed(format!(
                    "EPUB package path '{path}' escapes its containing part"
                )));
            }
            other => normalized.push(other),
        }
    }
    if normalized.is_empty() {
        return Err(PackageError::Malformed(format!(
            "EPUB package path '{path}' has no package target"
        )));
    }
    Ok(normalized.join("/"))
}

fn decode_percent_escapes(value: &str) -> Result<String, PackageError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0_usize;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some(high) = bytes.get(index + 1).and_then(|byte| hex_value(*byte)) else {
            return Err(PackageError::Malformed(format!(
                "EPUB package path '{value}' has a malformed percent escape"
            )));
        };
        let Some(low) = bytes.get(index + 2).and_then(|byte| hex_value(*byte)) else {
            return Err(PackageError::Malformed(format!(
                "EPUB package path '{value}' has a malformed percent escape"
            )));
        };
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| {
        PackageError::Malformed(format!(
            "EPUB package path '{value}' is not valid percent-encoded UTF-8"
        ))
    })
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
