//! Writing a [`GeoDocument`] in another format (task 0281).
//!
//! Every writer reports what the target format cannot represent instead of dropping it silently,
//! so the caller can show the list and let the user continue or cancel before anything is written.
//! The notes are plain English sentences with counts; they are deduplicated and bounded.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{Cursor, Write};

use serde_json::{Map, Value};
use thiserror::Error;
use zip::write::SimpleFileOptions;

use crate::model::GeoDocument;
use crate::time::parse_timestamp;

/// The formats a document can be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// GeoJSON, pretty-printed or compact.
    GeoJson {
        /// Indent with two spaces instead of writing one line.
        pretty: bool,
    },
    /// GPX 1.1.
    Gpx,
    /// KML 2.2.
    Kml,
    /// KMZ holding one `doc.kml`.
    Kmz,
    /// Comma separated values with latitude and longitude columns.
    Csv {
        /// Add a `wkt` column with every geometry, so lines and areas survive.
        wkt: bool,
    },
}

impl Target {
    /// File extension of the format, without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::GeoJson { .. } => "geojson",
            Self::Gpx => "gpx",
            Self::Kml => "kml",
            Self::Kmz => "kmz",
            Self::Csv { .. } => "csv",
        }
    }

    /// Reads a wire identifier (`geojson`, `gpx`, `kml`, `kmz`, `csv`).
    #[must_use]
    pub fn parse(id: &str, csv_wkt: bool) -> Option<Self> {
        match id {
            "geojson" => Some(Self::GeoJson { pretty: true }),
            "gpx" => Some(Self::Gpx),
            "kml" => Some(Self::Kml),
            "kmz" => Some(Self::Kmz),
            "csv" => Some(Self::Csv { wkt: csv_wkt }),
            _ => None,
        }
    }
}

/// A converted file and what was lost on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Converted {
    /// The file content.
    pub bytes: Vec<u8>,
    /// What the target format could not represent.
    pub notes: Vec<String>,
}

/// Why a document could not be written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConvertError {
    /// Nothing in the document fits the target format.
    #[error("{0}")]
    NothingToWrite(String),
    /// Building the output failed.
    #[error("writing the output failed: {0}")]
    Output(String),
}

/// Keys that exist only to carry reader state or style, never user data.
const INTERNAL_KEYS: [&str; 2] = ["_icon", "_fmIndex"];
const STYLE_KEYS: [&str; 9] = [
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "fill",
    "fill-opacity",
    "marker-color",
    "marker-size",
    "icon-href",
    "_icon",
];
const MAX_NOTES: usize = 20;

#[derive(Default)]
struct Notes {
    items: BTreeSet<String>,
}

impl Notes {
    fn add(&mut self, note: impl Into<String>) {
        if self.items.len() < MAX_NOTES {
            self.items.insert(note.into());
        }
    }

    fn finish(self) -> Vec<String> {
        self.items.into_iter().collect()
    }
}

fn properties_of(feature: &Value) -> Map<String, Value> {
    feature
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn text_of(properties: &Map<String, Value>, key: &str) -> Option<String> {
    match properties.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            control if control.is_control() && !matches!(control, '\n' | '\r' | '\t') => {}
            other => escaped.push(other),
        }
    }
    escaped
}

fn position(value: &Value) -> Option<Vec<f64>> {
    let numbers: Vec<f64> = value.as_array()?.iter().filter_map(Value::as_f64).collect();
    (numbers.len() >= 2).then_some(numbers)
}

fn positions(value: &Value) -> Vec<Vec<f64>> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(position).collect())
        .unwrap_or_default()
}

fn geometry_of(feature: &Value) -> Option<&Value> {
    feature
        .get("geometry")
        .filter(|geometry| !geometry.is_null())
}

fn kind_of(geometry: &Value) -> &str {
    geometry.get("type").and_then(Value::as_str).unwrap_or("")
}

fn has_style(properties: &Map<String, Value>) -> bool {
    STYLE_KEYS.iter().any(|key| properties.contains_key(*key))
}

/// Converts `document` into `target`.
///
/// # Errors
///
/// Fails when nothing in the document can be stored in the target format.
pub fn convert(document: &GeoDocument, target: Target) -> Result<Converted, ConvertError> {
    let mut notes = Notes::default();
    if !document.resources.is_empty() {
        notes.add(format!(
            "{} embedded image(s) of the KMZ are not carried over",
            document.resources.len()
        ));
    }
    if !document.overlays.is_empty() {
        notes.add(format!(
            "{} ground overlay(s) are not carried over",
            document.overlays.len()
        ));
    }
    if !document.skipped.is_empty() {
        notes.add(format!(
            "{} element(s) of the source were never read (network links, 3D models, tours)",
            document.skipped.len()
        ));
    }
    let bytes = match target {
        Target::GeoJson { pretty } => write_geojson(document, pretty, &mut notes),
        Target::Gpx => write_gpx(document, &mut notes)?,
        Target::Kml => write_kml(document, &mut notes).into_bytes(),
        Target::Kmz => write_kmz(&write_kml(document, &mut notes))?,
        Target::Csv { wkt } => write_csv(document, wkt, &mut notes)?,
    };
    Ok(Converted {
        bytes,
        notes: notes.finish(),
    })
}

/* --------------------------------- GeoJSON --------------------------------- */

fn write_geojson(document: &GeoDocument, pretty: bool, notes: &mut Notes) -> Vec<u8> {
    let features: Vec<Value> = document
        .features
        .iter()
        .map(|feature| {
            let mut properties = properties_of(feature);
            for key in INTERNAL_KEYS {
                properties.remove(key);
            }
            let mut copy = feature.clone();
            if let Some(object) = copy.as_object_mut() {
                object.insert("properties".into(), Value::Object(properties));
            }
            copy
        })
        .collect();
    if document
        .features
        .iter()
        .any(|f| properties_of(f).contains_key("icon-href"))
    {
        notes.add("icon URLs are kept only as the icon-href property");
    }
    let collection = serde_json::json!({ "type": "FeatureCollection", "features": features });
    let text = if pretty {
        serde_json::to_string_pretty(&collection)
    } else {
        serde_json::to_string(&collection)
    };
    let mut output = text.unwrap_or_default();
    output.push('\n');
    output.into_bytes()
}

/* ----------------------------------- GPX ----------------------------------- */

struct GpxOut {
    waypoints: String,
    routes: String,
    tracks: String,
    written: usize,
}

fn gpx_point_elements(properties: &Map<String, Value>, elevation: Option<f64>) -> String {
    let mut out = String::new();
    if let Some(elevation) = elevation.or_else(|| properties.get("ele").and_then(Value::as_f64)) {
        let _ = write!(out, "<ele>{elevation}</ele>");
    }
    if let Some(time) = text_of(properties, "time").or_else(|| text_of(properties, "timestamp"))
        && parse_timestamp(&time).is_some()
    {
        let _ = write!(out, "<time>{}</time>", escape_xml(&time));
    }
    for (key, element) in [
        ("name", "name"),
        ("cmt", "cmt"),
        ("description", "desc"),
        ("desc", "desc"),
        ("link", ""),
        ("sym", "sym"),
        ("type", "type"),
    ] {
        if element.is_empty() {
            continue;
        }
        if let Some(text) = text_of(properties, key) {
            let _ = write!(out, "<{element}>{}</{element}>", escape_xml(&text));
        }
    }
    out
}

fn gpx_line_points(
    coordinates: &[Vec<f64>],
    times: Option<&Vec<Value>>,
    rates: Option<&Vec<Value>>,
    tag: &str,
) -> String {
    let mut out = String::new();
    for (index, point) in coordinates.iter().enumerate() {
        let _ = write!(out, "<{tag} lat=\"{}\" lon=\"{}\">", point[1], point[0]);
        if let Some(elevation) = point.get(2) {
            let _ = write!(out, "<ele>{elevation}</ele>");
        }
        if let Some(Value::String(time)) = times.and_then(|list| list.get(index))
            && parse_timestamp(time).is_some()
        {
            let _ = write!(out, "<time>{}</time>", escape_xml(time));
        }
        if let Some(rate) = rates
            .and_then(|list| list.get(index))
            .and_then(Value::as_f64)
        {
            let _ = write!(
                out,
                "<extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>{rate}</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions>"
            );
        }
        let _ = write!(out, "</{tag}>");
    }
    out
}

fn array_at(value: Option<&Value>, index: Option<usize>) -> Option<&Vec<Value>> {
    let value = value?;
    match index {
        Some(position) => value.as_array()?.get(position)?.as_array(),
        None => value.as_array(),
    }
}

fn gpx_geometry(
    geometry: &Value,
    properties: &Map<String, Value>,
    out: &mut GpxOut,
    notes: &mut Notes,
) {
    match kind_of(geometry) {
        "Point" => {
            if let Some(point) = geometry.get("coordinates").and_then(position) {
                let _ = write!(
                    out.waypoints,
                    "<wpt lat=\"{}\" lon=\"{}\">{}</wpt>",
                    point[1],
                    point[0],
                    gpx_point_elements(properties, point.get(2).copied())
                );
                out.written += 1;
            }
        }
        "MultiPoint" => {
            for point in geometry
                .get("coordinates")
                .map(positions)
                .unwrap_or_default()
            {
                let _ = write!(
                    out.waypoints,
                    "<wpt lat=\"{}\" lon=\"{}\">{}</wpt>",
                    point[1],
                    point[0],
                    gpx_point_elements(properties, point.get(2).copied())
                );
                out.written += 1;
            }
        }
        kind @ ("LineString" | "MultiLineString") => {
            let lines: Vec<Vec<Vec<f64>>> = if kind == "LineString" {
                vec![
                    geometry
                        .get("coordinates")
                        .map(positions)
                        .unwrap_or_default(),
                ]
            } else {
                geometry
                    .get("coordinates")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().map(positions).collect())
                    .unwrap_or_default()
            };
            let extra = properties.get("coordinateProperties");
            let is_route = properties.get("gpxType").and_then(Value::as_str) == Some("route");
            let header = {
                let mut text = String::new();
                for (key, element) in [
                    ("name", "name"),
                    ("description", "desc"),
                    ("desc", "desc"),
                    ("cmt", "cmt"),
                    ("src", "src"),
                    ("type", "type"),
                ] {
                    if let Some(value) = text_of(properties, key) {
                        let _ = write!(text, "<{element}>{}</{element}>", escape_xml(&value));
                    }
                }
                text
            };
            if is_route && lines.len() == 1 {
                let _ = write!(
                    out.routes,
                    "<rte>{header}{}</rte>",
                    gpx_line_points(&lines[0], None, None, "rtept")
                );
            } else {
                let _ = write!(out.tracks, "<trk>{header}");
                let multi = kind == "MultiLineString";
                for (index, line) in lines.iter().enumerate() {
                    let at = multi.then_some(index);
                    let times = array_at(extra.and_then(|e| e.get("times")), at);
                    let rates = array_at(extra.and_then(|e| e.get("heartRates")), at);
                    let _ = write!(
                        out.tracks,
                        "<trkseg>{}</trkseg>",
                        gpx_line_points(line, times, rates, "trkpt")
                    );
                }
                out.tracks.push_str("</trk>");
            }
            out.written += 1;
        }
        "GeometryCollection" => {
            for member in geometry
                .get("geometries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                gpx_geometry(member, properties, out, notes);
            }
        }
        _ => notes.add("polygons cannot be stored in GPX (it has no areas) and were left out"),
    }
}

fn write_gpx(document: &GeoDocument, notes: &mut Notes) -> Result<Vec<u8>, ConvertError> {
    let mut out = GpxOut {
        waypoints: String::new(),
        routes: String::new(),
        tracks: String::new(),
        written: 0,
    };
    let mut dropped_properties = 0_usize;
    let mut styled = 0_usize;
    let mut descriptions_with_markup = false;
    for feature in &document.features {
        let Some(geometry) = geometry_of(feature) else {
            continue;
        };
        let properties = properties_of(feature);
        if has_style(&properties) {
            styled += 1;
        }
        const KEPT: [&str; 15] = [
            "name",
            "description",
            "desc",
            "cmt",
            "src",
            "type",
            "sym",
            "link",
            "ele",
            "time",
            "timestamp",
            "gpxType",
            "coordinateProperties",
            "folder",
            "altitudeMode",
        ];
        dropped_properties += properties
            .keys()
            .filter(|key| !KEPT.contains(&key.as_str()) && !STYLE_KEYS.contains(&key.as_str()))
            .count();
        descriptions_with_markup |=
            text_of(&properties, "description").is_some_and(|d| d.contains('<'));
        gpx_geometry(geometry, &properties, &mut out, notes);
    }
    if styled > 0 {
        notes.add(format!(
            "colours, line widths and icons of {styled} feature(s) are not stored in GPX"
        ));
    }
    if dropped_properties > 0 {
        notes.add(format!(
            "{dropped_properties} custom property value(s) are not stored in GPX (only name, description, type, symbol, link, elevation and time are)"
        ));
    }
    if descriptions_with_markup {
        notes.add("HTML descriptions are written as text with their markup");
    }
    if document
        .features
        .iter()
        .any(|f| properties_of(f).contains_key("folder"))
    {
        notes.add("folder structure is not stored in GPX");
    }
    if out.written == 0 {
        return Err(ConvertError::NothingToWrite(
            "GPX can only hold points and lines, and this document has none".into(),
        ));
    }
    let mut gpx = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"Procyon\" xmlns=\"http://www.topografix.com/GPX/1/1\" xmlns:gpxtpx=\"http://www.garmin.com/xmlschemas/TrackPointExtension/v1\">\n",
    );
    for section in [&out.waypoints, &out.routes, &out.tracks] {
        gpx.push_str(section);
    }
    gpx.push_str("\n</gpx>\n");
    Ok(gpx.into_bytes())
}

/* ----------------------------------- KML ----------------------------------- */

fn kml_color(css: &str, opacity: f64) -> Option<String> {
    let hex = css.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!(
        "{alpha:02x}{}{}{}",
        hex[4..6].to_ascii_lowercase(),
        hex[2..4].to_ascii_lowercase(),
        hex[0..2].to_ascii_lowercase()
    ))
}

fn kml_coordinates(points: &[Vec<f64>]) -> String {
    points
        .iter()
        .map(|point| {
            point
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn kml_geometry(geometry: &Value, properties: &Map<String, Value>) -> String {
    let mode = text_of(properties, "altitudeMode")
        .map(|mode| format!("<altitudeMode>{}</altitudeMode>", escape_xml(&mode)))
        .unwrap_or_default();
    let coordinates = geometry.get("coordinates");
    let ring = |points: &[Vec<f64>]| {
        format!(
            "<LinearRing><coordinates>{}</coordinates></LinearRing>",
            kml_coordinates(points)
        )
    };
    let polygon = |rings: &Value| {
        let rings: Vec<Vec<Vec<f64>>> = rings
            .as_array()
            .map(|items| items.iter().map(positions).collect())
            .unwrap_or_default();
        let mut text = String::from("<Polygon>");
        if let Some(outer) = rings.first() {
            let _ = write!(text, "<outerBoundaryIs>{}</outerBoundaryIs>", ring(outer));
        }
        for hole in rings.iter().skip(1) {
            let _ = write!(text, "<innerBoundaryIs>{}</innerBoundaryIs>", ring(hole));
        }
        text.push_str("</Polygon>");
        text
    };
    match kind_of(geometry) {
        "Point" => coordinates
            .and_then(position)
            .map_or_else(String::new, |point| {
                format!(
                    "<Point>{mode}<coordinates>{}</coordinates></Point>",
                    kml_coordinates(&[point])
                )
            }),
        "LineString" => format!(
            "<LineString>{mode}<coordinates>{}</coordinates></LineString>",
            kml_coordinates(&coordinates.map(positions).unwrap_or_default())
        ),
        "Polygon" => coordinates.map_or_else(String::new, polygon),
        "MultiPoint" => format!(
            "<MultiGeometry>{}</MultiGeometry>",
            coordinates
                .map(positions)
                .unwrap_or_default()
                .iter()
                .map(|point| format!(
                    "<Point><coordinates>{}</coordinates></Point>",
                    kml_coordinates(std::slice::from_ref(point))
                ))
                .collect::<String>()
        ),
        "MultiLineString" => format!(
            "<MultiGeometry>{}</MultiGeometry>",
            coordinates
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|line| format!(
                    "<LineString><coordinates>{}</coordinates></LineString>",
                    kml_coordinates(&positions(line))
                ))
                .collect::<String>()
        ),
        "MultiPolygon" => format!(
            "<MultiGeometry>{}</MultiGeometry>",
            coordinates
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(polygon)
                .collect::<String>()
        ),
        "GeometryCollection" => format!(
            "<MultiGeometry>{}</MultiGeometry>",
            geometry
                .get("geometries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|member| kml_geometry(member, properties))
                .collect::<String>()
        ),
        _ => String::new(),
    }
}

fn kml_style(properties: &Map<String, Value>) -> Option<String> {
    let number = |key: &str| properties.get(key).and_then(Value::as_f64);
    let mut style = String::new();
    if let Some(color) = text_of(properties, "stroke")
        && let Some(value) = kml_color(&color, number("stroke-opacity").unwrap_or(1.0))
    {
        let width = number("stroke-width").unwrap_or(1.0);
        let _ = write!(
            style,
            "<LineStyle><color>{value}</color><width>{width}</width></LineStyle>"
        );
    }
    if let Some(color) = text_of(properties, "fill")
        && let Some(value) = kml_color(&color, number("fill-opacity").unwrap_or(0.5))
    {
        let _ = write!(style, "<PolyStyle><color>{value}</color></PolyStyle>");
    }
    if let Some(color) = text_of(properties, "marker-color")
        && let Some(value) = kml_color(&color, 1.0)
    {
        let scale = match text_of(properties, "marker-size").as_deref() {
            Some("small") => 0.7,
            Some("large") => 1.6,
            _ => 1.0,
        };
        let _ = write!(
            style,
            "<IconStyle><color>{value}</color><scale>{scale}</scale></IconStyle>"
        );
    }
    (!style.is_empty()).then_some(style)
}

enum Item {
    Placemark(String),
    Child(String, Folder),
}

#[derive(Default)]
struct Folder {
    items: Vec<Item>,
}

impl Folder {
    /// The nested folder for `path`, created in order of first use.
    fn descend(&mut self, path: &[&str]) -> &mut Folder {
        let Some((first, rest)) = path.split_first() else {
            return self;
        };
        let position = match self
            .items
            .iter()
            .position(|item| matches!(item, Item::Child(name, _) if name == first))
        {
            Some(position) => position,
            None => {
                self.items
                    .push(Item::Child((*first).to_owned(), Folder::default()));
                self.items.len() - 1
            }
        };
        if let Item::Child(_, child) = &mut self.items[position] {
            child.descend(rest)
        } else {
            unreachable!("the position was found as a folder")
        }
    }

    fn render(&self, out: &mut String) {
        for item in &self.items {
            match item {
                Item::Placemark(text) => out.push_str(text),
                Item::Child(name, child) => {
                    let _ = write!(out, "<Folder><name>{}</name>", escape_xml(name));
                    child.render(out);
                    out.push_str("</Folder>");
                }
            }
        }
    }
}

fn cdata(text: &str) -> String {
    format!("<![CDATA[{}]]>", text.replace("]]>", "]]]]><![CDATA[>"))
}

fn write_kml(document: &GeoDocument, notes: &mut Notes) -> String {
    let mut styles: Vec<String> = Vec::new();
    let mut root = Folder::default();
    let mut timed_tracks = 0_usize;
    let mut skipped_polygon_free = 0_usize;
    for feature in &document.features {
        let Some(geometry) = geometry_of(feature) else {
            skipped_polygon_free += 1;
            continue;
        };
        let properties = properties_of(feature);
        let mut placemark = String::from("<Placemark>");
        if let Some(name) = text_of(&properties, "name") {
            let _ = write!(placemark, "<name>{}</name>", escape_xml(&name));
        }
        if let Some(description) =
            text_of(&properties, "description").or_else(|| text_of(&properties, "desc"))
        {
            let _ = write!(
                placemark,
                "<description>{}</description>",
                cdata(&description)
            );
        }
        if let Some(style) = kml_style(&properties) {
            let index = styles
                .iter()
                .position(|known| *known == style)
                .unwrap_or_else(|| {
                    styles.push(style);
                    styles.len() - 1
                });
            let _ = write!(placemark, "<styleUrl>#s{index}</styleUrl>");
        }
        if let Some(when) = text_of(&properties, "timestamp") {
            let _ = write!(
                placemark,
                "<TimeStamp><when>{}</when></TimeStamp>",
                escape_xml(&when)
            );
        } else if let (Some(begin), end) = (
            text_of(&properties, "timeBegin"),
            text_of(&properties, "timeEnd"),
        ) {
            let _ = write!(
                placemark,
                "<TimeSpan><begin>{}</begin>{}</TimeSpan>",
                escape_xml(&begin),
                end.map(|end| format!("<end>{}</end>", escape_xml(&end)))
                    .unwrap_or_default()
            );
        }
        let mut extended = String::new();
        const HANDLED: [&str; 12] = [
            "name",
            "description",
            "desc",
            "folder",
            "altitudeMode",
            "timestamp",
            "timeBegin",
            "timeEnd",
            "coordinateProperties",
            "gpxType",
            "_icon",
            "link",
        ];
        for (key, value) in &properties {
            if HANDLED.contains(&key.as_str()) || STYLE_KEYS.contains(&key.as_str()) {
                continue;
            }
            if let Some(text) = text_of(&properties, key) {
                let _ = write!(
                    extended,
                    "<Data name=\"{}\"><value>{}</value></Data>",
                    escape_xml(key),
                    escape_xml(&text)
                );
            } else if !value.is_null() {
                notes.add("structured property values (objects, lists) are not stored in KML");
            }
        }
        if !extended.is_empty() {
            let _ = write!(placemark, "<ExtendedData>{extended}</ExtendedData>");
        }
        placemark.push_str(&kml_geometry(geometry, &properties));
        placemark.push_str("</Placemark>");
        if properties
            .get("coordinateProperties")
            .and_then(|extra| extra.get("times"))
            .is_some()
        {
            timed_tracks += 1;
        }
        let folder = text_of(&properties, "folder").unwrap_or_default();
        let path: Vec<&str> = folder
            .split(" / ")
            .filter(|part| !part.is_empty())
            .collect();
        root.descend(&path).items.push(Item::Placemark(placemark));
    }
    if timed_tracks > 0 {
        notes.add(format!(
            "per-point timestamps and heart rates of {timed_tracks} track(s) are not stored in plain KML"
        ));
    }
    if skipped_polygon_free > 0 {
        notes.add(format!(
            "{skipped_polygon_free} feature(s) without geometry were left out"
        ));
    }
    let mut kml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\"><Document>",
    );
    for (index, style) in styles.iter().enumerate() {
        let _ = write!(kml, "<Style id=\"s{index}\">{style}</Style>");
    }
    root.render(&mut kml);
    kml.push_str("</Document></kml>\n");
    kml
}

fn write_kmz(kml: &str) -> Result<Vec<u8>, ConvertError> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        writer
            .start_file(
                "doc.kml",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|error| ConvertError::Output(error.to_string()))?;
        writer
            .write_all(kml.as_bytes())
            .map_err(|error| ConvertError::Output(error.to_string()))?;
        writer
            .finish()
            .map_err(|error| ConvertError::Output(error.to_string()))?;
    }
    Ok(buffer.into_inner())
}

/* ------------------------------------ CSV ----------------------------------- */

fn csv_cell(text: &str, formulas: &mut usize) -> String {
    let mut value = text.to_owned();
    if value.starts_with(['=', '+', '@'])
        || (value.starts_with('-') && value.parse::<f64>().is_err())
    {
        value.insert(0, '\'');
        *formulas += 1;
    }
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

fn wkt_positions(points: &[Vec<f64>]) -> String {
    points
        .iter()
        .map(|point| {
            point
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn wkt(geometry: &Value) -> String {
    let coordinates = geometry.get("coordinates");
    let rings = |value: &Value| -> String {
        value
            .as_array()
            .into_iter()
            .flatten()
            .map(|ring| format!("({})", wkt_positions(&positions(ring))))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match kind_of(geometry) {
        "Point" => coordinates.and_then(position).map_or_else(
            || "POINT EMPTY".into(),
            |point| format!("POINT ({})", wkt_positions(&[point])),
        ),
        "MultiPoint" => format!(
            "MULTIPOINT ({})",
            coordinates
                .map(positions)
                .unwrap_or_default()
                .iter()
                .map(|point| format!("({})", wkt_positions(std::slice::from_ref(point))))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "LineString" => format!(
            "LINESTRING ({})",
            wkt_positions(&coordinates.map(positions).unwrap_or_default())
        ),
        "MultiLineString" => format!(
            "MULTILINESTRING ({})",
            coordinates.map_or_else(String::new, rings)
        ),
        "Polygon" => format!("POLYGON ({})", coordinates.map_or_else(String::new, rings)),
        "MultiPolygon" => format!(
            "MULTIPOLYGON ({})",
            coordinates
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|polygon| format!("({})", rings(polygon)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "GeometryCollection" => format!(
            "GEOMETRYCOLLECTION ({})",
            geometry
                .get("geometries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(wkt)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "GEOMETRYCOLLECTION EMPTY".into(),
    }
}

fn write_csv(
    document: &GeoDocument,
    with_wkt: bool,
    notes: &mut Notes,
) -> Result<Vec<u8>, ConvertError> {
    let mut columns: BTreeSet<String> = BTreeSet::new();
    for feature in &document.features {
        for (key, value) in properties_of(feature) {
            if matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_))
                && !INTERNAL_KEYS.contains(&key.as_str())
                && !STYLE_KEYS.contains(&key.as_str())
                && key != "name"
                && key != "description"
            {
                columns.insert(key);
            }
        }
    }
    let columns: Vec<String> = columns.into_iter().take(100).collect();
    let mut header = vec!["name".to_owned(), "description".to_owned()];
    header.extend(columns.iter().cloned());
    header.extend(["latitude".into(), "longitude".into(), "altitude".into()]);
    if with_wkt {
        header.push("wkt".into());
    }
    let mut formulas = 0_usize;
    let mut csv = header
        .iter()
        .map(|cell| csv_cell(cell, &mut formulas))
        .collect::<Vec<_>>()
        .join(",");
    csv.push_str("\r\n");
    let (mut rows, mut non_points, mut structured) = (0_usize, 0_usize, 0_usize);
    for feature in &document.features {
        let Some(geometry) = geometry_of(feature) else {
            continue;
        };
        let properties = properties_of(feature);
        structured += properties
            .values()
            .filter(|value| matches!(value, Value::Object(_) | Value::Array(_)))
            .count();
        let points: Vec<Vec<f64>> = match kind_of(geometry) {
            "Point" => geometry
                .get("coordinates")
                .and_then(position)
                .into_iter()
                .collect(),
            "MultiPoint" => geometry
                .get("coordinates")
                .map(positions)
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if points.is_empty() && !with_wkt {
            non_points += 1;
            continue;
        }
        let one_row = |point: Option<&Vec<f64>>, formulas: &mut usize| -> String {
            let mut cells = vec![
                csv_cell(&text_of(&properties, "name").unwrap_or_default(), formulas),
                csv_cell(
                    &text_of(&properties, "description").unwrap_or_default(),
                    formulas,
                ),
            ];
            for column in &columns {
                cells.push(csv_cell(
                    &text_of(&properties, column).unwrap_or_default(),
                    formulas,
                ));
            }
            let coordinate = |index: usize| {
                point
                    .and_then(|p| p.get(index))
                    .map(f64::to_string)
                    .unwrap_or_default()
            };
            cells.push(coordinate(1));
            cells.push(coordinate(0));
            cells.push(coordinate(2));
            if with_wkt {
                cells.push(csv_cell(&wkt(geometry), formulas));
            }
            cells.join(",")
        };
        if points.is_empty() || with_wkt {
            csv.push_str(&one_row(points.first(), &mut formulas));
            csv.push_str("\r\n");
            rows += 1;
        } else {
            for point in &points {
                csv.push_str(&one_row(Some(point), &mut formulas));
                csv.push_str("\r\n");
                rows += 1;
            }
        }
    }
    if non_points > 0 {
        notes.add(format!(
            "{non_points} line or area feature(s) were left out: CSV rows hold points (choose the WKT option to keep every geometry)"
        ));
    }
    if structured > 0 {
        notes.add("structured property values (objects, lists) are not stored in CSV");
    }
    if formulas > 0 {
        notes.add(format!(
            "{formulas} text cell(s) starting with = + - or @ were prefixed with an apostrophe so spreadsheets do not run them as formulas"
        ));
    }
    if document
        .features
        .iter()
        .any(|f| properties_of(f).contains_key("coordinateProperties"))
    {
        notes.add("per-point timestamps and heart rates are not stored in CSV");
    }
    if rows == 0 {
        return Err(ConvertError::NothingToWrite(
            "this document has no points to write; choose the WKT option to keep lines and areas"
                .into(),
        ));
    }
    Ok(csv.into_bytes())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::kml::NoResources;
    use crate::{
        GeoFormat, Limits, conversion_limits, parse_geojson, parse_gpx, parse_kml, parse_kmz,
    };

    const GPX: &str = r#"<?xml version="1.0"?>
<gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1" xmlns:ns3="http://www.garmin.com/xmlschemas/TrackPointExtension/v1">
 <wpt lat="52.0" lon="4.0"><ele>1.5</ele><name>Start &amp; finish</name><sym>Flag</sym><time>2024-05-05T07:59:00Z</time></wpt>
 <rte><name>Plan</name><rtept lat="51" lon="3"><ele>2</ele></rtept><rtept lat="51.5" lon="3.5"><ele>3</ele></rtept></rte>
 <trk><name>Run</name><type>running</type>
  <trkseg>
   <trkpt lat="52.000" lon="4.000"><ele>10</ele><time>2024-05-05T08:00:00Z</time><extensions><ns3:TrackPointExtension><ns3:hr>120</ns3:hr></ns3:TrackPointExtension></extensions></trkpt>
   <trkpt lat="52.001" lon="4.000"><ele>15</ele><time>2024-05-05T08:00:10Z</time><extensions><ns3:TrackPointExtension><ns3:hr>130</ns3:hr></ns3:TrackPointExtension></extensions></trkpt>
  </trkseg>
  <trkseg>
   <trkpt lat="52.003" lon="4.000"><ele>22</ele><time>2024-05-05T08:00:30Z</time></trkpt>
   <trkpt lat="52.004" lon="4.000"><ele>20</ele><time>2024-05-05T08:00:40Z</time></trkpt>
  </trkseg>
 </trk>
</gpx>"#;

    fn gpx() -> GeoDocument {
        parse_gpx(GPX.as_bytes(), conversion_limits()).expect("gpx")
    }

    fn geometries(document: &GeoDocument) -> Vec<Value> {
        document
            .features
            .iter()
            .map(|f| f["geometry"].clone())
            .collect()
    }

    fn names(document: &GeoDocument) -> Vec<Option<String>> {
        document
            .features
            .iter()
            .map(|f| f["properties"]["name"].as_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn gpx_survives_a_trip_through_geojson_with_times_and_heart_rates() {
        let original = gpx();
        let geojson = convert(&original, Target::GeoJson { pretty: false }).expect("geojson");
        let via = parse_geojson(&geojson.bytes, conversion_limits()).expect("parsed");
        let back = convert(&via, Target::Gpx).expect("gpx");
        let round = parse_gpx(&back.bytes, conversion_limits()).expect("parsed again");
        assert_eq!(geometries(&round), geometries(&original));
        assert_eq!(names(&round), names(&original));
        let times = |document: &GeoDocument| {
            document.features[2]["properties"]["coordinateProperties"].clone()
        };
        assert_eq!(times(&round), times(&original));
        assert_eq!(round.features[0]["properties"]["sym"], "Flag");
        assert_eq!(
            round.features[0]["properties"]["time"],
            "2024-05-05T07:59:00Z"
        );
        assert_eq!(round.features[1]["properties"]["gpxType"], "route");
        assert_eq!(
            round.tracks[0].distance_meters,
            original.tracks[0].distance_meters
        );
    }

    #[test]
    fn gpx_to_kml_keeps_geometry_and_names_and_reports_the_lost_per_point_data() {
        let original = gpx();
        let kml = convert(&original, Target::Kml).expect("kml");
        let text = String::from_utf8(kml.bytes.clone()).expect("utf-8");
        let parsed = parse_kml(
            text.as_bytes(),
            Limits::default(),
            &NoResources,
            GeoFormat::Kml,
        )
        .expect("kml parses");
        assert_eq!(names(&parsed), names(&original));
        assert_eq!(
            parsed.features[0]["geometry"],
            original.features[0]["geometry"]
        );
        assert_eq!(
            parsed.features[1]["geometry"],
            original.features[1]["geometry"]
        );
        assert_eq!(
            parsed.features[2]["geometry"],
            original.features[2]["geometry"]
        );
        assert!(
            kml.notes
                .iter()
                .any(|note| note.contains("per-point timestamps")),
            "{:?}",
            kml.notes
        );
    }

    const KML: &str = r##"<kml><Document><name>Trip</name>
        <Style id="road"><LineStyle><color>ff0000ff</color><width>4</width></LineStyle></Style>
        <Style id="park"><PolyStyle><color>7f00ff00</color></PolyStyle></Style>
        <Folder><name>Day 1</name>
          <Placemark><name>Museum</name><description><![CDATA[<b>Open</b> & free]]></description><Point><coordinates>4.9,52.37,3</coordinates></Point>
            <ExtendedData><Data name="rating"><value>5</value></Data></ExtendedData></Placemark>
          <Placemark><name>Route</name><styleUrl>#road</styleUrl><LineString><coordinates>4,52 5,53</coordinates></LineString></Placemark>
        </Folder>
        <Placemark><name>Park</name><styleUrl>#park</styleUrl>
          <Polygon><outerBoundaryIs><LinearRing><coordinates>0,0 4,0 4,4 0,4 0,0</coordinates></LinearRing></outerBoundaryIs>
          <innerBoundaryIs><LinearRing><coordinates>1,1 2,1 2,2 1,2 1,1</coordinates></LinearRing></innerBoundaryIs></Polygon></Placemark>
        </Document></kml>"##;

    fn kml() -> GeoDocument {
        parse_kml(
            KML.as_bytes(),
            conversion_limits(),
            &NoResources,
            GeoFormat::Kml,
        )
        .expect("kml")
    }

    #[test]
    fn kml_survives_a_trip_through_geojson_and_back() {
        let original = kml();
        let geojson = convert(&original, Target::GeoJson { pretty: true }).expect("geojson");
        let via = parse_geojson(&geojson.bytes, conversion_limits()).expect("parsed");
        let back = convert(&via, Target::Kml).expect("kml");
        let round = parse_kml(
            &back.bytes,
            conversion_limits(),
            &NoResources,
            GeoFormat::Kml,
        )
        .expect("kml parses");
        assert_eq!(geometries(&round), geometries(&original));
        assert_eq!(names(&round), names(&original));
        let folder = |document: &GeoDocument, index: usize| {
            document.features[index]["properties"]
                .get("folder")
                .cloned()
        };
        assert_eq!(folder(&round, 0), folder(&original, 0));
        assert_eq!(folder(&round, 2), folder(&original, 2));
        let property = |document: &GeoDocument, index: usize, key: &str| {
            document.features[index]["properties"][key].clone()
        };
        assert_eq!(property(&round, 1, "stroke"), json!("#ff0000"));
        assert_eq!(property(&round, 1, "stroke-width"), json!(4.0));
        assert_eq!(property(&round, 2, "fill"), property(&original, 2, "fill"));
        assert_eq!(
            property(&round, 0, "description"),
            property(&original, 0, "description")
        );
        assert_eq!(property(&round, 0, "rating"), json!("5"));
    }

    #[test]
    fn kml_to_gpx_drops_areas_and_styles_but_says_so() {
        let converted = convert(&kml(), Target::Gpx).expect("gpx");
        let text = String::from_utf8(converted.bytes).expect("utf-8");
        let parsed = parse_gpx(text.as_bytes(), conversion_limits()).expect("parses");
        assert_eq!(parsed.features.len(), 2, "the polygon is not written");
        let notes = converted.notes.join(" | ");
        assert!(
            notes.contains("polygons cannot be stored in GPX"),
            "{notes}"
        );
        assert!(
            notes.contains("colours, line widths and icons of"),
            "{notes}"
        );
        assert!(notes.contains("HTML descriptions"), "{notes}");
        assert!(notes.contains("folder structure"), "{notes}");
    }

    #[test]
    fn a_document_without_points_or_lines_cannot_become_gpx() {
        let polygons = parse_geojson(
            br#"{"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]},"properties":{}}"#,
            Limits::default(),
        )
        .expect("valid");
        assert!(matches!(
            convert(&polygons, Target::Gpx),
            Err(ConvertError::NothingToWrite(_))
        ));
    }

    #[test]
    fn kmz_output_is_a_zip_with_doc_kml_that_reads_back() {
        let converted = convert(&kml(), Target::Kmz).expect("kmz");
        let round = parse_kmz(&converted.bytes, conversion_limits()).expect("kmz reads");
        assert_eq!(names(&round), names(&kml()));
    }

    #[test]
    fn csv_writes_points_with_columns_and_reports_what_it_left_out() {
        let converted = convert(&kml(), Target::Csv { wkt: false }).expect("csv");
        let text = String::from_utf8(converted.bytes).expect("utf-8");
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some("name,description,folder,rating,latitude,longitude,altitude")
        );
        assert_eq!(
            lines.next(),
            Some("Museum,<b>Open</b> & free,Trip / Day 1,5,52.37,4.9,3")
        );
        assert_eq!(lines.next(), None);
        assert!(
            converted
                .notes
                .iter()
                .any(|n| n.contains("2 line or area feature(s)")),
            "{:?}",
            converted.notes
        );
    }

    #[test]
    fn csv_with_wkt_keeps_every_geometry() {
        let converted = convert(&kml(), Target::Csv { wkt: true }).expect("csv");
        let text = String::from_utf8(converted.bytes).expect("utf-8");
        assert!(
            text.contains("\"POLYGON ((0 0, 4 0, 4 4, 0 4, 0 0), (1 1, 2 1, 2 2, 1 2, 1 1))\"")
        );
        assert!(text.contains("\"LINESTRING (4 52, 5 53)\""));
        assert!(text.contains("POINT (4.9 52.37 3)"));
        assert_eq!(text.lines().count(), 4);
    }

    #[test]
    fn csv_escapes_cells_and_neutralises_spreadsheet_formulas() {
        let document = parse_geojson(
            br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{"name":"=SUM(A1)","description":"a \"quoted\", text","score":-3}}]}"#,
            Limits::default(),
        )
        .expect("valid");
        let converted = convert(&document, Target::Csv { wkt: false }).expect("csv");
        let text = String::from_utf8(converted.bytes).expect("utf-8");
        let row = text.lines().nth(1).expect("row");
        assert_eq!(row, "'=SUM(A1),\"a \"\"quoted\"\", text\",-3,2,1,");
        assert!(converted.notes.iter().any(|n| n.contains("apostrophe")));
    }

    #[test]
    fn csv_without_any_point_is_refused() {
        let line = parse_geojson(
            br#"{"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0],[1,1]]},"properties":{}}"#,
            Limits::default(),
        )
        .expect("valid");
        assert!(matches!(
            convert(&line, Target::Csv { wkt: false }),
            Err(ConvertError::NothingToWrite(_))
        ));
        assert!(convert(&line, Target::Csv { wkt: true }).is_ok());
    }

    #[test]
    fn output_is_escaped_so_hostile_names_cannot_inject_xml() {
        let document = parse_geojson(
            br#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{"name":"</name><evil/>","description":"]]><x/>"}}"#,
            Limits::default(),
        )
        .expect("valid");
        for target in [Target::Gpx, Target::Kml] {
            let converted = convert(&document, target).expect("converts");
            let text = String::from_utf8(converted.bytes).expect("utf-8");
            assert!(!text.contains("<evil/>"), "{text}");
            quick_xml::Reader::from_str(&text)
                .config_mut()
                .check_end_names = true;
            let mut reader = quick_xml::Reader::from_str(&text);
            while reader.read_event().expect("well-formed output") != quick_xml::events::Event::Eof
            {
            }
        }
    }

    #[test]
    fn geojson_output_keeps_the_gpx_kind_drops_reader_internals_and_notes_embedded_images() {
        let mut document = gpx();
        document
            .resources
            .insert("a.png".into(), "data:image/png;base64,AA==".into());
        let converted = convert(&document, Target::GeoJson { pretty: true }).expect("geojson");
        let text = String::from_utf8(converted.bytes).expect("utf-8");
        assert!(text.contains("gpxType"));
        assert!(!text.contains("_fmIndex"));
        assert!(converted.notes.iter().any(|n| n.contains("embedded image")));
        assert!(text.contains('\n') && text.contains("  "));
        let compact = convert(&gpx(), Target::GeoJson { pretty: false }).expect("geojson");
        assert_eq!(
            String::from_utf8(compact.bytes)
                .expect("utf-8")
                .trim()
                .lines()
                .count(),
            1
        );
    }
}
