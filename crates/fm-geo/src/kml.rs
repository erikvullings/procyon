//! KML reading (task 0280): placemarks, folders, styles, descriptions and ground overlays.
//!
//! The reader is an event stream like the GPX reader: entities are never expanded and DOCTYPE
//! declarations with entities are refused. Nothing named by the file is fetched: `NetworkLink`
//! targets and external icons are listed as left out, and only images shipped in the same KMZ
//! can be drawn.

use std::collections::{BTreeMap, HashMap};

use base64::Engine;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::message::{GeoMessage, codes};
use crate::model::{Bounds, GeoDocument, GeoFormat, GeoIssue, GroundOverlay, Limits};

const MAX_RESOURCES: usize = 64;
const MAX_RESOURCE_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_RESOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_STYLE_DEPTH: usize = 4;

/// Where images referenced by a KML file can be read from (the other entries of a KMZ).
pub trait ResourceSource {
    /// The bytes of the entry at `path` (relative to the KML file), if present and small enough.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

/// A source without any resources, used for a plain `.kml` file.
#[derive(Debug, Clone, Copy)]
pub struct NoResources;

impl ResourceSource for NoResources {
    fn read(&self, _path: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Why a KML file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KmlError {
    /// The XML is malformed.
    #[error("invalid XML at line {line}, column {column}: {message}")]
    Syntax {
        /// Parser message.
        message: String,
        /// One-based line.
        line: usize,
        /// One-based column.
        column: usize,
    },
    /// The XML is not a KML document.
    #[error("{}", .0.text)]
    NotKml(GeoMessage),
    /// The document declares entities.
    #[error("DOCTYPE declarations with entities are not allowed")]
    Entities,
}

impl KmlError {
    /// The localisable description of the failure.
    #[must_use]
    pub fn message(&self) -> GeoMessage {
        match self {
            Self::Syntax {
                message,
                line,
                column,
            } => GeoMessage::new(codes::XML_SYNTAX, self.to_string())
                .with("line", line)
                .with("column", column)
                .with("detail", message),
            Self::NotKml(message) => message.clone(),
            Self::Entities => GeoMessage::new(codes::ENTITIES_NOT_ALLOWED, self.to_string()),
        }
    }
}

fn line_column(bytes: &[u8], offset: usize) -> (usize, usize) {
    let end = offset.min(bytes.len());
    let before = &bytes[..end];
    let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
    let column = before
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(end, |newline| end - newline - 1)
        + 1;
    (line, column)
}

fn local(name: &[u8]) -> String {
    let text = String::from_utf8_lossy(name);
    text.rsplit(':').next().unwrap_or(&text).to_owned()
}

fn attribute(start: &quick_xml::events::BytesStart<'_>, wanted: &str) -> Option<String> {
    start.attributes().flatten().find_map(|attribute| {
        (local(attribute.key.as_ref()) == wanted)
            .then(|| String::from_utf8_lossy(&attribute.value).into_owned())
    })
}

/// KML colours are `aabbggrr`; returns a CSS `#rrggbb` and the alpha in 0..=1.
fn color(text: &str) -> Option<(String, f64)> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 8 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let alpha = u8::from_str_radix(&text[0..2], 16).ok()?;
    Some((
        format!("#{}{}{}", &text[6..8], &text[4..6], &text[2..4]),
        f64::from(alpha) / 255.0,
    ))
}

#[derive(Default, Clone)]
struct StyleDef {
    stroke: Option<(String, f64)>,
    width: Option<f64>,
    fill: Option<(String, f64)>,
    fill_enabled: Option<bool>,
    outline: Option<bool>,
    icon_color: Option<(String, f64)>,
    scale: Option<f64>,
    icon_href: Option<String>,
}

impl StyleDef {
    /// Later (more specific) values win.
    fn merged(&self, over: &StyleDef) -> StyleDef {
        StyleDef {
            stroke: over.stroke.clone().or_else(|| self.stroke.clone()),
            width: over.width.or(self.width),
            fill: over.fill.clone().or_else(|| self.fill.clone()),
            fill_enabled: over.fill_enabled.or(self.fill_enabled),
            outline: over.outline.or(self.outline),
            icon_color: over.icon_color.clone().or_else(|| self.icon_color.clone()),
            scale: over.scale.or(self.scale),
            icon_href: over.icon_href.clone().or_else(|| self.icon_href.clone()),
        }
    }
}

enum Geo {
    Point(Option<Vec<f64>>),
    Line(Vec<Vec<f64>>),
    Ring(Vec<Vec<f64>>),
    Polygon {
        outer: Option<Vec<Vec<f64>>>,
        inner: Vec<Vec<Vec<f64>>>,
        in_inner: bool,
    },
    Multi(Vec<Value>),
}

#[derive(Default)]
struct Placemark {
    name: Option<String>,
    description: Option<String>,
    style_ref: Option<String>,
    inline_style: StyleDef,
    geometries: Vec<Value>,
    extra: Map<String, Value>,
    folder: String,
}

#[derive(Default)]
struct Overlay {
    name: Option<String>,
    href: Option<String>,
    north: Option<f64>,
    south: Option<f64>,
    east: Option<f64>,
    west: Option<f64>,
    rotation: f64,
}

fn parse_positions(text: &str) -> (Vec<Vec<f64>>, usize) {
    let mut positions = Vec::new();
    let mut invalid = 0;
    for tuple in text.split_whitespace() {
        let numbers: Option<Vec<f64>> = tuple
            .split(',')
            .map(|part| {
                part.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
            })
            .collect();
        match numbers {
            Some(values)
                if (2..=3).contains(&values.len())
                    && (-180.0..=180.0).contains(&values[0])
                    && (-90.0..=90.0).contains(&values[1]) =>
            {
                positions.push(values);
            }
            _ => invalid += 1,
        }
    }
    (positions, invalid)
}

fn ring_value(ring: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut closed = ring.to_vec();
    if closed.first() != closed.last()
        && let Some(first) = closed.first().cloned()
    {
        closed.push(first);
    }
    closed
}

fn finish_geometry(geo: Geo) -> Option<Value> {
    match geo {
        Geo::Point(Some(position)) => Some(json!({ "type": "Point", "coordinates": position })),
        Geo::Line(positions) | Geo::Ring(positions) if positions.len() >= 2 => {
            Some(json!({ "type": "LineString", "coordinates": positions }))
        }
        Geo::Polygon {
            outer: Some(outer),
            inner,
            ..
        } if outer.len() >= 3 => {
            let mut rings = vec![ring_value(&outer)];
            rings.extend(
                inner
                    .iter()
                    .filter(|ring| ring.len() >= 3)
                    .map(|ring| ring_value(ring)),
            );
            Some(json!({ "type": "Polygon", "coordinates": rings }))
        }
        Geo::Multi(children) => match children.len() {
            0 => None,
            1 => children.into_iter().next(),
            _ => {
                let kinds: Vec<&str> = children
                    .iter()
                    .filter_map(|child| child.get("type").and_then(Value::as_str))
                    .collect();
                let same = |kind: &str| kinds.iter().all(|candidate| *candidate == kind);
                let merged = |multi: &str| {
                    json!({
                        "type": multi,
                        "coordinates": children
                            .iter()
                            .filter_map(|child| child.get("coordinates").cloned())
                            .collect::<Vec<_>>()
                    })
                };
                if same("Point") {
                    Some(merged("MultiPoint"))
                } else if same("LineString") {
                    Some(merged("MultiLineString"))
                } else if same("Polygon") {
                    Some(merged("MultiPolygon"))
                } else {
                    Some(json!({ "type": "GeometryCollection", "geometries": children }))
                }
            }
        },
        _ => None,
    }
}

fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn is_remote(href: &str) -> bool {
    let lowered = href.trim().to_ascii_lowercase();
    lowered.contains("://") || lowered.starts_with("//") || lowered.starts_with("data:")
}

/// Reads KML bytes; `resources` supplies the other files of a KMZ.
///
/// # Errors
///
/// Returns the first XML problem with its position, or why the document is not KML.
#[allow(clippy::too_many_lines)]
pub fn parse_kml(
    bytes: &[u8],
    limits: Limits,
    resources: &dyn ResourceSource,
    format: GeoFormat,
) -> Result<GeoDocument, KmlError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_end_names = true;
    let syntax = |error: &dyn std::fmt::Display, offset: usize| {
        let (line, column) = line_column(bytes, offset);
        KmlError::Syntax {
            message: error.to_string(),
            line,
            column,
        }
    };

    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut seen_root = false;
    let mut folder_names: Vec<String> = Vec::new();
    let mut styles: HashMap<String, StyleDef> = HashMap::new();
    let mut style_maps: HashMap<String, String> = HashMap::new();
    let mut current_style: Option<(Option<String>, StyleDef)> = None;
    let mut current_map: Option<String> = None;
    let mut pair_key: Option<String> = None;
    let mut pair_style: Option<String> = None;
    let mut placemark: Option<Placemark> = None;
    let mut placemarks: Vec<Placemark> = Vec::new();
    let mut total_features = 0_usize;
    let mut geometry: Vec<Geo> = Vec::new();
    let mut overlay: Option<Overlay> = None;
    let mut overlays: Vec<Overlay> = Vec::new();
    let mut data_name: Option<String> = None;
    let mut skipped: Vec<String> = Vec::new();
    let mut warnings: Vec<GeoIssue> = Vec::new();
    let mut invalid_positions = 0_usize;
    let mut link_href: Option<String> = None;
    let mut network_name: Option<String> = None;
    let mut positions_seen = 0_usize;

    loop {
        let event = reader.read_event().map_err(|error| {
            syntax(
                &error,
                usize::try_from(reader.error_position()).unwrap_or(0),
            )
        })?;
        match event {
            Event::Eof => break,
            Event::DocType(doctype) => {
                if String::from_utf8_lossy(&doctype).contains("ENTITY") {
                    return Err(KmlError::Entities);
                }
            }
            Event::Start(ref start) | Event::Empty(ref start) => {
                let is_start = matches!(event, Event::Start(_));
                let name = local(start.name().as_ref());
                if !seen_root {
                    if name != "kml" {
                        return Err(KmlError::NotKml(
                            GeoMessage::new(
                                codes::ROOT_NOT_KML,
                                format!("the root element is <{name}>, not <kml>"),
                            )
                            .with("name", &name),
                        ));
                    }
                    seen_root = true;
                }
                match name.as_str() {
                    "Folder" | "Document" => folder_names.push(String::new()),
                    "Style" => {
                        current_style = Some((attribute(start, "id"), StyleDef::default()));
                    }
                    "StyleMap" => current_map = attribute(start, "id"),
                    "Pair" => {
                        pair_key = None;
                        pair_style = None;
                    }
                    "Placemark" => {
                        placemark = Some(Placemark {
                            folder: folder_names
                                .iter()
                                .filter(|part| !part.is_empty())
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(" / "),
                            ..Placemark::default()
                        });
                        total_features += 1;
                    }
                    "GroundOverlay" => overlay = Some(Overlay::default()),
                    "NetworkLink" => {
                        network_name = None;
                        link_href = None;
                    }
                    "Point" => geometry.push(Geo::Point(None)),
                    "LineString" => geometry.push(Geo::Line(Vec::new())),
                    "LinearRing" => geometry.push(Geo::Ring(Vec::new())),
                    "Polygon" => geometry.push(Geo::Polygon {
                        outer: None,
                        inner: Vec::new(),
                        in_inner: false,
                    }),
                    "MultiGeometry" => geometry.push(Geo::Multi(Vec::new())),
                    "innerBoundaryIs" => {
                        if let Some(Geo::Polygon { in_inner, .. }) = geometry.last_mut() {
                            *in_inner = true;
                        }
                    }
                    "outerBoundaryIs" => {
                        if let Some(Geo::Polygon { in_inner, .. }) = geometry.last_mut() {
                            *in_inner = false;
                        }
                    }
                    "Data" => data_name = attribute(start, "name"),
                    "SimpleData" => data_name = attribute(start, "name"),
                    "Model" | "Track" | "MultiTrack" | "Tour" | "PhotoOverlay"
                    | "ScreenOverlay" => {
                        let label = match name.as_str() {
                            "Model" => "3D model",
                            "Track" | "MultiTrack" => "gx:Track",
                            "Tour" => "tour",
                            "PhotoOverlay" => "photo overlay",
                            _ => "screen overlay",
                        };
                        skipped.push(format!("{label} (not drawn on the 2D map)"));
                    }
                    "TimeSpan" | "TimeStamp"
                        if !skipped
                            .iter()
                            .any(|item| item.starts_with("time animation")) =>
                    {
                        skipped.push(
                            "time animation (not played; times are kept as properties)".into(),
                        );
                    }
                    _ => {}
                }
                if is_start {
                    stack.push(name);
                    text.clear();
                } else {
                    // An empty element carries no text; close it as if it ended immediately.
                    let closed = local(start.name().as_ref());
                    if matches!(closed.as_str(), "Folder" | "Document") {
                        folder_names.pop();
                    }
                }
            }
            Event::Text(content) => {
                if let Ok(decoded) = content.decode() {
                    text.push_str(&decoded);
                }
            }
            Event::CData(content) => text.push_str(&String::from_utf8_lossy(&content)),
            Event::GeneralRef(reference) => {
                if let Ok(name) = reference.decode() {
                    text.push_str(match name.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
                if let Ok(Some(character)) = reference.resolve_char_ref() {
                    text.push(character);
                }
            }
            Event::End(_) => {
                let Some(name) = stack.pop() else {
                    continue;
                };
                let value = text.trim().to_owned();
                text.clear();
                let parent = stack.last().map(String::as_str).unwrap_or_default();
                match name.as_str() {
                    "Folder" | "Document" => {
                        folder_names.pop();
                    }
                    "name" => match parent {
                        "Placemark" => {
                            if let Some(item) = placemark.as_mut() {
                                item.name = Some(value);
                            }
                        }
                        "Folder" | "Document" => {
                            if let Some(slot) = folder_names.last_mut() {
                                *slot = value;
                            }
                        }
                        "GroundOverlay" => {
                            if let Some(item) = overlay.as_mut() {
                                item.name = Some(value);
                            }
                        }
                        "NetworkLink" => network_name = Some(value),
                        _ => {}
                    },
                    "description" if parent == "Placemark" => {
                        if let Some(item) = placemark.as_mut() {
                            item.description = Some(value);
                        }
                    }
                    "styleUrl" => match parent {
                        "Placemark" => {
                            if let Some(item) = placemark.as_mut() {
                                item.style_ref = Some(value);
                            }
                        }
                        "Pair" => pair_style = Some(value),
                        _ => {}
                    },
                    "key" if parent == "Pair" => pair_key = Some(value),
                    "Pair" => {
                        if pair_key.as_deref() == Some("normal")
                            && let (Some(map), Some(target)) =
                                (current_map.clone(), pair_style.take())
                        {
                            style_maps.insert(map, target);
                        }
                    }
                    "StyleMap" => current_map = None,
                    "Style" => {
                        if let Some((id, definition)) = current_style.take() {
                            if let Some(id) = id {
                                styles.insert(id, definition);
                            } else if let Some(item) = placemark.as_mut() {
                                item.inline_style = definition;
                            }
                        }
                    }
                    "color" => {
                        if let Some((_, definition)) = current_style.as_mut() {
                            match parent {
                                "LineStyle" => definition.stroke = color(&value),
                                "PolyStyle" => definition.fill = color(&value),
                                "IconStyle" => definition.icon_color = color(&value),
                                _ => {}
                            }
                        }
                    }
                    "width" if parent == "LineStyle" => {
                        if let Some((_, definition)) = current_style.as_mut() {
                            definition.width = value.parse().ok();
                        }
                    }
                    "scale" if parent == "IconStyle" => {
                        if let Some((_, definition)) = current_style.as_mut() {
                            definition.scale = value.parse().ok();
                        }
                    }
                    "fill" if parent == "PolyStyle" => {
                        if let Some((_, definition)) = current_style.as_mut() {
                            definition.fill_enabled = Some(value != "0");
                        }
                    }
                    "outline" if parent == "PolyStyle" => {
                        if let Some((_, definition)) = current_style.as_mut() {
                            definition.outline = Some(value != "0");
                        }
                    }
                    "href" => {
                        let grand = stack
                            .len()
                            .checked_sub(2)
                            .and_then(|i| stack.get(i))
                            .map(String::as_str);
                        match (parent, grand) {
                            ("Icon", Some("IconStyle")) => {
                                if let Some((_, definition)) = current_style.as_mut() {
                                    definition.icon_href = Some(value);
                                }
                            }
                            ("Icon", Some("GroundOverlay")) => {
                                if let Some(item) = overlay.as_mut() {
                                    item.href = Some(value);
                                }
                            }
                            ("Link" | "Url", Some("NetworkLink")) => link_href = Some(value),
                            _ => {}
                        }
                    }
                    "north" | "south" | "east" | "west" | "rotation" if parent == "LatLonBox" => {
                        if let Some(item) = overlay.as_mut() {
                            let number = value.parse::<f64>().ok();
                            match name.as_str() {
                                "north" => item.north = number,
                                "south" => item.south = number,
                                "east" => item.east = number,
                                "west" => item.west = number,
                                _ => item.rotation = number.unwrap_or(0.0),
                            }
                        }
                    }
                    "coordinates" => {
                        let (positions, invalid) = parse_positions(&value);
                        invalid_positions += invalid;
                        positions_seen += positions.len();
                        match geometry.last_mut() {
                            Some(Geo::Point(slot)) => *slot = positions.into_iter().next(),
                            Some(Geo::Line(target) | Geo::Ring(target)) => *target = positions,
                            _ => {}
                        }
                    }
                    "Point" | "LineString" | "LinearRing" | "Polygon" | "MultiGeometry" => {
                        if let Some(built) = geometry.pop() {
                            let finished = match (&name[..], built, geometry.last_mut()) {
                                (
                                    "LinearRing",
                                    Geo::Ring(ring),
                                    Some(Geo::Polygon {
                                        outer,
                                        inner,
                                        in_inner,
                                    }),
                                ) => {
                                    if *in_inner {
                                        inner.push(ring);
                                    } else {
                                        *outer = Some(ring);
                                    }
                                    None
                                }
                                (_, built, _) => finish_geometry(built),
                            };
                            if let Some(finished) = finished {
                                match geometry.last_mut() {
                                    Some(Geo::Multi(children)) => children.push(finished),
                                    _ => {
                                        if let Some(item) = placemark.as_mut() {
                                            item.geometries.push(finished);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "altitudeMode" | "extrude" | "tessellate"
                        if parent != "LookAt" && parent != "Camera" =>
                    {
                        if let Some(item) = placemark.as_mut() {
                            item.extra.insert(name.clone(), Value::String(value));
                        }
                    }
                    "when" | "begin" | "end" => {
                        if let Some(item) = placemark.as_mut() {
                            let key = match name.as_str() {
                                "when" => "timestamp",
                                "begin" => "timeBegin",
                                _ => "timeEnd",
                            };
                            item.extra.insert(key.to_owned(), Value::String(value));
                        }
                    }
                    "value" if parent == "Data" => {
                        if let (Some(item), Some(key)) = (placemark.as_mut(), data_name.take()) {
                            item.extra.insert(key, Value::String(value));
                        }
                    }
                    "SimpleData" => {
                        if let (Some(item), Some(key)) = (placemark.as_mut(), data_name.take()) {
                            item.extra.insert(key, Value::String(value));
                        }
                    }
                    "NetworkLink" => {
                        let label = network_name.take().unwrap_or_else(|| "unnamed".into());
                        skipped.push(match link_href.take() {
                            Some(href) => format!("network link '{label}' to {href} (not fetched)"),
                            None => format!("network link '{label}' (not fetched)"),
                        });
                    }
                    "GroundOverlay" => {
                        if let Some(item) = overlay.take() {
                            overlays.push(item);
                        }
                    }
                    "Placemark" => {
                        if let Some(item) = placemark.take()
                            && placemarks.len() < limits.max_features
                            && positions_seen < limits.max_positions
                        {
                            placemarks.push(item);
                        }
                        geometry.clear();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if !seen_root {
        return Err(KmlError::NotKml(GeoMessage::new(
            codes::EMPTY_DOCUMENT,
            "the document is empty",
        )));
    }

    // Resolve styles, then images, now that every definition has been read.
    let resolve = |reference: &str, inline: &StyleDef| -> StyleDef {
        let mut current = reference.trim_start_matches('#').to_owned();
        let mut base = StyleDef::default();
        for _ in 0..MAX_STYLE_DEPTH {
            if let Some(target) = style_maps.get(&current) {
                current = target.trim_start_matches('#').to_owned();
                continue;
            }
            if let Some(found) = styles.get(&current) {
                base = found.clone();
            }
            break;
        }
        base.merged(inline)
    };
    let mut loaded: BTreeMap<String, String> = BTreeMap::new();
    let mut total_resource_bytes = 0_usize;
    let mut external_icons = 0_usize;
    let mut missing_images = 0_usize;
    let mut load = |href: &str, missing: &mut usize| -> Option<String> {
        if is_remote(href) {
            return None;
        }
        let key = href.trim().trim_start_matches("./").to_owned();
        if loaded.contains_key(&key) {
            return Some(key);
        }
        let data = if loaded.len() < MAX_RESOURCES {
            resources.read(&key)
        } else {
            None
        };
        let mime = data.as_deref().and_then(sniff_mime);
        match (data, mime) {
            (Some(data), Some(mime))
                if data.len() <= MAX_RESOURCE_BYTES
                    && total_resource_bytes + data.len() <= MAX_TOTAL_RESOURCE_BYTES =>
            {
                total_resource_bytes += data.len();
                loaded.insert(
                    key.clone(),
                    format!(
                        "data:{mime};base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&data)
                    ),
                );
                Some(key)
            }
            _ => {
                *missing += 1;
                None
            }
        }
    };

    let mut features: Vec<Value> = Vec::new();
    let mut bounds: Option<Bounds> = None;
    let mut include = |position: &[f64]| {
        let (longitude, latitude) = (position[0], position[1]);
        bounds = Some(match bounds {
            None => [longitude, latitude, longitude, latitude],
            Some([west, south, east, north]) => [
                west.min(longitude),
                south.min(latitude),
                east.max(longitude),
                north.max(latitude),
            ],
        });
    };
    fn visit(value: &Value, include: &mut dyn FnMut(&[f64])) {
        match value {
            Value::Array(items) => {
                if items.len() >= 2 && items.iter().all(Value::is_number) {
                    let numbers: Vec<f64> = items.iter().filter_map(Value::as_f64).collect();
                    include(&numbers);
                } else {
                    for item in items {
                        visit(item, include);
                    }
                }
            }
            Value::Object(object) => {
                for key in ["coordinates", "geometries"] {
                    if let Some(inner) = object.get(key) {
                        visit(inner, include);
                    }
                }
            }
            _ => {}
        }
    }

    for item in placemarks {
        if item.geometries.is_empty() {
            continue;
        }
        let style = resolve(item.style_ref.as_deref().unwrap_or(""), &item.inline_style);
        let mut properties = Map::new();
        if let Some(name) = &item.name {
            properties.insert("name".into(), json!(name));
        }
        if let Some(description) = &item.description {
            properties.insert("description".into(), json!(description));
        }
        if !item.folder.is_empty() {
            properties.insert("folder".into(), json!(item.folder));
        }
        if let Some((value, opacity)) = &style.stroke {
            properties.insert("stroke".into(), json!(value));
            properties.insert("stroke-opacity".into(), json!(opacity));
        }
        if let Some(width) = style.width {
            properties.insert("stroke-width".into(), json!(width));
        }
        if let Some((value, opacity)) = &style.fill {
            properties.insert("fill".into(), json!(value));
            properties.insert("fill-opacity".into(), json!(opacity));
        }
        if style.fill_enabled == Some(false) {
            properties.insert("fill-opacity".into(), json!(0));
        }
        if style.outline == Some(false) {
            properties.insert("stroke-opacity".into(), json!(0));
        }
        if let Some((value, _)) = &style.icon_color {
            properties.insert("marker-color".into(), json!(value));
        }
        if let Some(scale) = style.scale {
            let size = if scale < 0.8 {
                "small"
            } else if scale > 1.4 {
                "large"
            } else {
                "medium"
            };
            properties.insert("marker-size".into(), json!(size));
        }
        if let Some(href) = &style.icon_href {
            if is_remote(href) {
                external_icons += 1;
            } else if let Some(key) = load(href, &mut missing_images) {
                properties.insert("_icon".into(), json!(key));
            }
        }
        for (key, value) in &item.extra {
            properties
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        let geometry = if item.geometries.len() == 1 {
            item.geometries.into_iter().next().unwrap_or(Value::Null)
        } else {
            json!({ "type": "GeometryCollection", "geometries": item.geometries })
        };
        visit(&geometry, &mut include);
        features.push(json!({ "type": "Feature", "geometry": geometry, "properties": properties }));
    }

    let mut ground: Vec<GroundOverlay> = Vec::new();
    for item in overlays {
        let label = item.name.clone().unwrap_or_else(|| "unnamed".into());
        let (Some(north), Some(south), Some(east), Some(west)) =
            (item.north, item.south, item.east, item.west)
        else {
            skipped.push(format!("ground overlay '{label}' has no LatLonBox"));
            continue;
        };
        match item.href.as_deref() {
            None => skipped.push(format!("ground overlay '{label}' has no image")),
            Some(href) if is_remote(href) => {
                skipped.push(format!(
                    "ground overlay '{label}' uses an external image (not fetched)"
                ));
            }
            Some(href) => match load(href, &mut missing_images) {
                Some(image) => {
                    include(&[west, south]);
                    include(&[east, north]);
                    ground.push(GroundOverlay {
                        name: item.name,
                        image,
                        north,
                        south,
                        east,
                        west,
                        rotation: item.rotation,
                    });
                }
                None => skipped.push(format!(
                    "ground overlay '{label}': image '{href}' is not available in this file"
                )),
            },
        }
    }
    if external_icons > 0 {
        warnings.push(GeoIssue::new(
            String::new(),
            GeoMessage::new(
                codes::EXTERNAL_ICONS,
                format!("{external_icons} placemark(s) use external icons, which are not fetched; a coloured dot is drawn instead"),
            )
            .with("count", external_icons),
        ));
    }
    if missing_images > 0 {
        warnings.push(GeoIssue::new(
            String::new(),
            GeoMessage::new(
                codes::IMAGES_MISSING,
                format!("{missing_images} referenced image(s) are missing, too large or not a supported image"),
            )
            .with("count", missing_images),
        ));
    }
    if invalid_positions > 0 {
        warnings.push(GeoIssue::new(
            String::new(),
            GeoMessage::new(
                codes::INVALID_POSITIONS,
                format!("{invalid_positions} coordinate(s) were invalid and left out"),
            )
            .with("count", invalid_positions),
        ));
    }
    let truncated = total_features > limits.max_features && total_features > features.len();
    Ok(GeoDocument {
        format,
        total_features,
        truncated,
        features,
        bounds,
        warnings,
        skipped,
        tracks: Vec::new(),
        resources: loaded,
        overlays: ground,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    struct Files(Vec<(&'static str, Vec<u8>)>);

    impl ResourceSource for Files {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, data)| data.clone())
        }
    }

    fn parse(text: &str) -> GeoDocument {
        parse_kml(
            text.as_bytes(),
            Limits::default(),
            &NoResources,
            GeoFormat::Kml,
        )
        .expect("valid kml")
    }

    /// Modelled on a Google Earth export: shared styles, a StyleMap, nested folders, rich
    /// placemarks, extended data and several things the 2D map cannot show.
    const EARTH: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Trip</name>
  <Style id="road"><LineStyle><color>ff0000ff</color><width>4</width></LineStyle></Style>
  <Style id="park"><PolyStyle><color>7f00ff00</color><outline>0</outline></PolyStyle></Style>
  <Style id="pin"><IconStyle><color>ffff0000</color><scale>2</scale><Icon><href>icons/pin.png</href></Icon></IconStyle></Style>
  <Style id="web"><IconStyle><Icon><href>http://maps.example/icon.png</href></Icon></IconStyle></Style>
  <StyleMap id="roadmap"><Pair><key>normal</key><styleUrl>#road</styleUrl></Pair><Pair><key>highlight</key><styleUrl>#park</styleUrl></Pair></StyleMap>
  <Folder><name>Day 1</name>
    <Folder><name>Sights</name>
      <Placemark><name>Museum &amp; cafe</name><description><![CDATA[<b>Open</b> daily <script>alert(1)</script>]]></description>
        <styleUrl>#pin</styleUrl><Point><coordinates>4.9,52.37,3</coordinates></Point>
        <ExtendedData><Data name="rating"><value>5</value></Data><SchemaData><SimpleData name="kind">museum</SimpleData></SchemaData></ExtendedData>
        <TimeSpan><begin>2024-05-01</begin><end>2024-05-02</end></TimeSpan>
      </Placemark>
      <Placemark><name>Web icon</name><styleUrl>#web</styleUrl><Point><coordinates>5,53</coordinates></Point></Placemark>
    </Folder>
    <Placemark><name>Route</name><styleUrl>#roadmap</styleUrl><LineString><altitudeMode>clampToGround</altitudeMode><coordinates>4,52 4.5,52.5 5,53</coordinates></LineString></Placemark>
  </Folder>
  <Placemark><name>Park</name><styleUrl>#park</styleUrl>
    <Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 4,0 4,4 0,4 0,0</coordinates></LinearRing></outerBoundaryIs>
      <innerBoundaryIs><LinearRing><coordinates>1,1 2,1 2,2 1,2 1,1</coordinates></LinearRing></innerBoundaryIs></Polygon></Placemark>
  <Placemark><name>Pair</name><MultiGeometry><Point><coordinates>1,1</coordinates></Point><Point><coordinates>2,2</coordinates></Point></MultiGeometry></Placemark>
  <Placemark><name>Mixed</name><MultiGeometry><Point><coordinates>1,1</coordinates></Point><LineString><coordinates>0,0 1,1</coordinates></LineString></MultiGeometry></Placemark>
  <Placemark><name>Building</name><Model><Location><longitude>1</longitude></Location></Model></Placemark>
  <NetworkLink><name>Live traffic</name><Link><href>http://traffic.example/feed.kml</href></Link></NetworkLink>
  <GroundOverlay><name>Old map</name><Icon><href>http://old.example/map.png</href></Icon><LatLonBox><north>1</north><south>0</south><east>1</east><west>0</west></LatLonBox></GroundOverlay>
</Document>
</kml>"##;

    #[test]
    fn reads_geometries_of_every_kind() {
        let document = parse(EARTH);
        assert_eq!(document.format, GeoFormat::Kml);
        let names: Vec<&str> = document
            .features
            .iter()
            .filter_map(|f| f["properties"]["name"].as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Museum & cafe",
                "Web icon",
                "Route",
                "Park",
                "Pair",
                "Mixed"
            ]
        );
        let geometry = |index: usize| &document.features[index]["geometry"];
        assert_eq!(geometry(0)["coordinates"], json!([4.9, 52.37, 3.0]));
        assert_eq!(geometry(2)["type"], "LineString");
        assert_eq!(geometry(3)["type"], "Polygon");
        assert_eq!(
            geometry(3)["coordinates"].as_array().map(Vec::len),
            Some(2),
            "outer ring plus hole"
        );
        assert_eq!(geometry(4)["type"], "MultiPoint");
        assert_eq!(geometry(5)["type"], "GeometryCollection");
        assert_eq!(document.total_features, 7);
    }

    #[test]
    fn folders_become_layer_paths() {
        let document = parse(EARTH);
        let folder = |index: usize| document.features[index]["properties"]["folder"].as_str();
        assert_eq!(folder(0), Some("Trip / Day 1 / Sights"));
        assert_eq!(folder(2), Some("Trip / Day 1"));
        assert_eq!(folder(3), Some("Trip"));
    }

    #[test]
    fn resolves_styles_style_maps_and_converts_colours() {
        let document = parse(EARTH);
        let route = &document.features[2]["properties"];
        // StyleMap -> normal -> #road: red (aabbggrr ff0000ff), 4 px.
        assert_eq!(route["stroke"], "#ff0000");
        assert_eq!(route["stroke-width"], 4.0);
        assert_eq!(route["stroke-opacity"], 1.0);
        assert_eq!(route["altitudeMode"], "clampToGround");
        let park = &document.features[3]["properties"];
        assert_eq!(park["fill"], "#00ff00");
        assert!((park["fill-opacity"].as_f64().expect("opacity") - 127.0 / 255.0).abs() < 1e-9);
        assert_eq!(park["stroke-opacity"], 0);
        let pin = &document.features[0]["properties"];
        assert_eq!(pin["marker-color"], "#0000ff");
        assert_eq!(pin["marker-size"], "large");
    }

    #[test]
    fn keeps_descriptions_extended_data_and_times_as_properties() {
        let document = parse(EARTH);
        let properties = &document.features[0]["properties"];
        assert!(
            properties["description"]
                .as_str()
                .is_some_and(|d| d.contains("<b>Open</b>"))
        );
        assert_eq!(properties["rating"], "5");
        assert_eq!(properties["kind"], "museum");
        assert_eq!(properties["timeBegin"], "2024-05-01");
        assert_eq!(properties["timeEnd"], "2024-05-02");
    }

    #[test]
    fn lists_what_it_cannot_draw_and_never_fetches() {
        let document = parse(EARTH);
        let skipped = document.skipped.join(" | ");
        assert!(skipped.contains(
            "network link 'Live traffic' to http://traffic.example/feed.kml (not fetched)"
        ));
        assert!(skipped.contains("3D model"));
        assert!(skipped.contains("time animation"));
        assert!(skipped.contains("ground overlay 'Old map' uses an external image (not fetched)"));
        assert!(document.overlays.is_empty());
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("external icons"))
        );
        // The pin icon is relative but there is no archive, so it is reported as missing.
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("missing"))
        );
        assert!(document.features[0]["properties"].get("_icon").is_none());
    }

    #[test]
    fn loads_icons_and_overlays_shipped_in_the_archive() {
        let kml = r#"<kml><Document>
            <Style id="pin"><IconStyle><Icon><href>icons/pin.png</href></Icon></IconStyle></Style>
            <Placemark><styleUrl>#pin</styleUrl><Point><coordinates>1,2</coordinates></Point></Placemark>
            <GroundOverlay><name>Plan</name><Icon><href>plan.png</href></Icon>
              <LatLonBox><north>3</north><south>1</south><east>4</east><west>2</west><rotation>10</rotation></LatLonBox></GroundOverlay>
            </Document></kml>"#;
        let files = Files(vec![
            ("icons/pin.png", PNG.to_vec()),
            ("plan.png", PNG.to_vec()),
        ]);
        let document =
            parse_kml(kml.as_bytes(), Limits::default(), &files, GeoFormat::Kmz).expect("valid");
        assert_eq!(document.features[0]["properties"]["_icon"], "icons/pin.png");
        assert!(document.resources["icons/pin.png"].starts_with("data:image/png;base64,"));
        assert_eq!(document.overlays.len(), 1);
        assert_eq!(document.overlays[0].image, "plan.png");
        assert_eq!(document.overlays[0].rotation, 10.0);
        // The overlay extends the bounds even without placemarks nearby.
        assert_eq!(document.bounds, Some([1.0, 1.0, 4.0, 3.0]));
    }

    #[test]
    fn rejects_non_images_and_oversized_resources() {
        let kml = r#"<kml><Placemark><Style><IconStyle><Icon><href>x.png</href></Icon></IconStyle></Style>
            <Point><coordinates>1,2</coordinates></Point></Placemark></kml>"#;
        let not_an_image = Files(vec![("x.png", b"<html>".to_vec())]);
        let document = parse_kml(
            kml.as_bytes(),
            Limits::default(),
            &not_an_image,
            GeoFormat::Kmz,
        )
        .expect("valid");
        assert!(document.resources.is_empty());
        let huge = Files(vec![(
            "x.png",
            [PNG, &vec![0; MAX_RESOURCE_BYTES][..]].concat(),
        )]);
        let document =
            parse_kml(kml.as_bytes(), Limits::default(), &huge, GeoFormat::Kmz).expect("valid");
        assert!(document.resources.is_empty());
    }

    #[test]
    fn invalid_coordinates_are_dropped_with_a_warning() {
        let document = parse(
            r#"<kml><Placemark><LineString><coordinates>1,2 abc 3,4 200,5</coordinates></LineString></Placemark></kml>"#,
        );
        assert_eq!(
            document.features[0]["geometry"]["coordinates"],
            json!([[1.0, 2.0], [3.0, 4.0]])
        );
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("2 coordinate(s)"))
        );
    }

    #[test]
    fn rejects_entities_wrong_roots_and_malformed_xml() {
        let hostile = r#"<?xml version="1.0"?><!DOCTYPE kml [<!ENTITY x SYSTEM "file:///etc/passwd">]><kml/>"#;
        assert_eq!(
            parse_kml(
                hostile.as_bytes(),
                Limits::default(),
                &NoResources,
                GeoFormat::Kml
            ),
            Err(KmlError::Entities)
        );
        assert!(matches!(
            parse_kml(b"<gpx/>", Limits::default(), &NoResources, GeoFormat::Kml),
            Err(KmlError::NotKml(_))
        ));
        match parse_kml(
            b"<kml>\n<Placemark>\n</kml>",
            Limits::default(),
            &NoResources,
            GeoFormat::Kml,
        ) {
            Err(KmlError::Syntax { line, .. }) => assert!(line >= 2),
            other => panic!("expected a syntax error, got {other:?}"),
        }
    }

    #[test]
    fn placemarks_without_geometry_are_not_features() {
        let document = parse(r#"<kml><Placemark><name>Label only</name></Placemark></kml>"#);
        assert!(document.features.is_empty());
        assert_eq!(document.total_features, 1);
    }
}
