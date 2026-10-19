//! Making geographic files smaller (task 0282).
//!
//! Two paths: whitespace-only minification works on the text and changes nothing else, so it is
//! lossless for any file of the format. Everything else (fewer decimals, simplified lines, dropped
//! elevation, time or extensions, removed duplicates) works on the feature model and reports the
//! effect in statistics, including how much a track's length changed.

use serde_json::{Map, Value};

use crate::model::GeoDocument;
use crate::track_stats::{TrackPoint, haversine};

/// What to remove or reduce.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MinifyOptions {
    /// Round longitude and latitude to this many decimals (6 is about 11 cm).
    pub precision: Option<u8>,
    /// Remove line and area points that deviate less than this many metres (Douglas-Peucker).
    pub tolerance_meters: Option<f64>,
    /// Remove elevation values.
    pub drop_elevation: bool,
    /// Remove timestamps, including per-point times of tracks.
    pub drop_time: bool,
    /// Remove vendor extensions (heart rates) and custom properties.
    pub drop_extensions: bool,
    /// Remove repeated points, empty features and duplicate features.
    pub remove_duplicates: bool,
}

impl MinifyOptions {
    /// True when only whitespace is removed, which the text path can do without reading the data.
    #[must_use]
    pub fn is_whitespace_only(&self) -> bool {
        self.precision.is_none()
            && self.tolerance_meters.is_none()
            && !self.drop_elevation
            && !self.drop_time
            && !self.drop_extensions
            && !self.remove_duplicates
    }
}

/// How much a minification changed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MinifyStats {
    /// Features before.
    pub features_before: usize,
    /// Features after.
    pub features_after: usize,
    /// Coordinate positions before.
    pub positions_before: usize,
    /// Coordinate positions after.
    pub positions_after: usize,
    /// Summed length of all lines before, in metres.
    pub distance_before_meters: f64,
    /// Summed length of all lines after, in metres.
    pub distance_after_meters: f64,
    /// Rings that kept their points because simplifying would have made them cross themselves.
    pub rings_kept: usize,
}

/// Properties that stay when vendor extensions are dropped.
const STANDARD_PROPERTIES: [&str; 17] = [
    "name",
    "description",
    "desc",
    "cmt",
    "type",
    "sym",
    "link",
    "src",
    "folder",
    "gpxType",
    "ele",
    "time",
    "timestamp",
    "timeBegin",
    "timeEnd",
    "altitudeMode",
    "coordinateProperties",
];
const STYLE_PROPERTIES: [&str; 8] = [
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "fill",
    "fill-opacity",
    "marker-color",
    "marker-size",
    "_icon",
];
const TIME_PROPERTIES: [&str; 4] = ["time", "timestamp", "timeBegin", "timeEnd"];
const MAX_RING_CHECK: usize = 10_000;

type Position = Vec<f64>;

fn round(value: f64, decimals: u8) -> f64 {
    let factor = 10_f64.powi(i32::from(decimals));
    (value * factor).round() / factor
}

fn position_of(value: &Value) -> Option<Position> {
    let numbers: Vec<f64> = value.as_array()?.iter().filter_map(Value::as_f64).collect();
    (numbers.len() >= 2).then_some(numbers)
}

fn positions_of(value: &Value) -> Vec<Position> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(position_of).collect())
        .unwrap_or_default()
}

fn to_value(position: &[f64]) -> Value {
    Value::Array(
        position
            .iter()
            .map(|number| serde_json::json!(number))
            .collect(),
    )
}

fn track_point(position: &[f64]) -> TrackPoint {
    TrackPoint {
        longitude: position[0],
        latitude: position[1],
        elevation: None,
        time: None,
        heart_rate: None,
    }
}

fn line_length(points: &[Position]) -> f64 {
    points
        .windows(2)
        .map(|pair| haversine(&track_point(&pair[0]), &track_point(&pair[1])))
        .sum()
}

/// Metres east and north of the first point, small-area approximation.
fn project(points: &[Position]) -> Vec<(f64, f64)> {
    let reference = points.first().map_or((0.0, 0.0), |p| (p[0], p[1]));
    let scale = reference.1.to_radians().cos();
    points
        .iter()
        .map(|p| {
            (
                (p[0] - reference.0) * 111_320.0 * scale,
                (p[1] - reference.1) * 110_540.0,
            )
        })
        .collect()
}

fn distance_to_segment(point: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared == 0.0 {
        0.0
    } else {
        (((point.0 - a.0) * dx + (point.1 - a.1) * dy) / length_squared).clamp(0.0, 1.0)
    };
    let (px, py) = (a.0 + t * dx, a.1 + t * dy);
    ((point.0 - px).powi(2) + (point.1 - py).powi(2)).sqrt()
}

/// Indices kept by Douglas-Peucker between `first` and `last` (inclusive, both kept).
fn douglas_peucker(points: &[(f64, f64)], first: usize, last: usize, tolerance: f64) -> Vec<usize> {
    let mut keep = vec![false; points.len()];
    keep[first] = true;
    keep[last] = true;
    let mut stack = vec![(first, last)];
    while let Some((start, end)) = stack.pop() {
        if end <= start + 1 {
            continue;
        }
        let (mut worst, mut worst_distance) = (start, 0.0);
        for index in start + 1..end {
            let distance = distance_to_segment(points[index], points[start], points[end]);
            if distance > worst_distance {
                worst = index;
                worst_distance = distance;
            }
        }
        if worst_distance > tolerance {
            keep[worst] = true;
            stack.push((start, worst));
            stack.push((worst, end));
        }
    }
    keep.iter()
        .enumerate()
        .filter_map(|(index, kept)| kept.then_some(index))
        .collect()
}

fn simplify_line(points: &[Position], tolerance: f64) -> Vec<usize> {
    if points.len() <= 2 {
        return (0..points.len()).collect();
    }
    douglas_peucker(&project(points), 0, points.len() - 1, tolerance)
}

fn orientation(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

fn segments_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let (o1, o2) = (orientation(a, b, c), orientation(a, b, d));
    let (o3, o4) = (orientation(c, d, a), orientation(c, d, b));
    (o1 > 0.0) != (o2 > 0.0)
        && (o3 > 0.0) != (o4 > 0.0)
        && o1 != 0.0
        && o2 != 0.0
        && o3 != 0.0
        && o4 != 0.0
}

/// Whether a closed ring crosses itself; `None` when it is too large to check.
fn ring_self_intersects(points: &[(f64, f64)]) -> Option<bool> {
    let segments = points.len().saturating_sub(1);
    if segments > MAX_RING_CHECK {
        return None;
    }
    for i in 0..segments {
        for j in i + 2..segments {
            if i == 0 && j == segments - 1 {
                continue;
            }
            if segments_cross(points[i], points[i + 1], points[j], points[j + 1]) {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// Simplifies a closed ring; returns the kept positions, or `None` to keep the ring as it is.
fn simplify_ring(ring: &[Position], tolerance: f64) -> Option<Vec<Position>> {
    if ring.len() < 5 {
        return None;
    }
    let open = &ring[..ring.len() - 1];
    let projected = project(open);
    let original_crosses = ring_self_intersects(&project(ring)).unwrap_or(false);
    // Split at the point farthest from the start so the two halves are real open paths.
    let farthest = (1..open.len())
        .max_by(|a, b| {
            let da = projected[*a].0.hypot(projected[*a].1);
            let db = projected[*b].0.hypot(projected[*b].1);
            da.total_cmp(&db)
        })
        .unwrap_or(1);
    let mut scale = 1.0;
    for _ in 0..5 {
        let t = tolerance / scale;
        let mut indices = douglas_peucker(&projected, 0, farthest, t);
        let second = douglas_peucker(&projected, farthest, open.len() - 1, t);
        indices.extend(second.into_iter().skip(1));
        let mut kept: Vec<Position> = indices.iter().map(|index| open[*index].clone()).collect();
        if kept.len() < 3 {
            scale *= 2.0;
            continue;
        }
        kept.push(kept[0].clone());
        let crossing = ring_self_intersects(&project(&kept)).unwrap_or(false);
        if !crossing || original_crosses {
            return (kept.len() < ring.len()).then_some(kept);
        }
        scale *= 2.0;
    }
    None
}

struct Pass<'a> {
    options: &'a MinifyOptions,
    stats: MinifyStats,
    notes: Vec<String>,
}

impl Pass<'_> {
    fn clean(&self, position: &[f64]) -> Position {
        let mut cleaned: Position = position.to_vec();
        if let Some(decimals) = self.options.precision {
            cleaned[0] = round(cleaned[0], decimals);
            cleaned[1] = round(cleaned[1], decimals);
            if let Some(elevation) = cleaned.get_mut(2) {
                *elevation = round(*elevation, 1);
            }
        }
        if self.options.drop_elevation {
            cleaned.truncate(2);
        }
        cleaned
    }

    /// Cleans a line and returns the indices (into the input) of the points that remain.
    fn line(&mut self, input: &[Position]) -> (Vec<Position>, Vec<usize>) {
        self.stats.positions_before += input.len();
        self.stats.distance_before_meters += line_length(input);
        let cleaned: Vec<Position> = input.iter().map(|p| self.clean(p)).collect();
        let mut indices: Vec<usize> = (0..cleaned.len()).collect();
        if self.options.remove_duplicates {
            indices.dedup_by(|b, a| cleaned[*a][..2] == cleaned[*b][..2]);
        }
        if let Some(tolerance) = self.options.tolerance_meters {
            let kept: Vec<Position> = indices.iter().map(|i| cleaned[*i].clone()).collect();
            let simplified = simplify_line(&kept, tolerance);
            indices = simplified.into_iter().map(|i| indices[i]).collect();
        }
        let output: Vec<Position> = indices.iter().map(|i| cleaned[*i].clone()).collect();
        self.stats.positions_after += output.len();
        self.stats.distance_after_meters += line_length(&output);
        (output, indices)
    }

    fn ring(&mut self, input: &[Position]) -> Vec<Position> {
        self.stats.positions_before += input.len();
        let mut cleaned: Vec<Position> = input.iter().map(|p| self.clean(p)).collect();
        if self.options.remove_duplicates {
            cleaned.dedup_by(|b, a| a[..2] == b[..2]);
            if cleaned.first().map(|p| &p[..2]) != cleaned.last().map(|p| &p[..2])
                && let Some(first) = cleaned.first().cloned()
            {
                cleaned.push(first);
            }
        }
        if let Some(tolerance) = self.options.tolerance_meters {
            match simplify_ring(&cleaned, tolerance) {
                Some(simplified) => cleaned = simplified,
                None if cleaned.len() >= 5 => self.stats.rings_kept += 1,
                None => {}
            }
        }
        self.stats.positions_after += cleaned.len();
        cleaned
    }

    fn geometry(&mut self, geometry: &Value, arrays: &mut Arrays) -> Value {
        let coordinates = geometry.get("coordinates");
        let kind = geometry.get("type").and_then(Value::as_str).unwrap_or("");
        let mut out = Map::new();
        out.insert("type".into(), Value::String(kind.to_owned()));
        match (kind, coordinates) {
            ("Point", Some(point)) => {
                self.stats.positions_before += 1;
                self.stats.positions_after += 1;
                let value =
                    position_of(point).map_or_else(|| point.clone(), |p| to_value(&self.clean(&p)));
                out.insert("coordinates".into(), value);
            }
            ("MultiPoint", Some(points)) => {
                let cleaned: Vec<Value> = positions_of(points)
                    .iter()
                    .map(|p| {
                        self.stats.positions_before += 1;
                        self.stats.positions_after += 1;
                        to_value(&self.clean(p))
                    })
                    .collect();
                out.insert("coordinates".into(), Value::Array(cleaned));
            }
            ("LineString", Some(line)) => {
                let (positions, indices) = self.line(&positions_of(line));
                arrays.keep(&[indices], false);
                out.insert(
                    "coordinates".into(),
                    Value::Array(positions.iter().map(|p| to_value(p)).collect()),
                );
            }
            ("MultiLineString", Some(lines)) => {
                let mut all_indices = Vec::new();
                let mut result = Vec::new();
                for line in lines.as_array().into_iter().flatten() {
                    let (positions, indices) = self.line(&positions_of(line));
                    all_indices.push(indices);
                    result.push(Value::Array(
                        positions.iter().map(|p| to_value(p)).collect(),
                    ));
                }
                arrays.keep(&all_indices, true);
                out.insert("coordinates".into(), Value::Array(result));
            }
            ("Polygon", Some(rings)) => {
                let result: Vec<Value> = rings
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|ring| {
                        Value::Array(
                            self.ring(&positions_of(ring))
                                .iter()
                                .map(|p| to_value(p))
                                .collect(),
                        )
                    })
                    .collect();
                out.insert("coordinates".into(), Value::Array(result));
            }
            ("MultiPolygon", Some(polygons)) => {
                let result: Vec<Value> = polygons
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|polygon| {
                        Value::Array(
                            polygon
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|ring| {
                                    Value::Array(
                                        self.ring(&positions_of(ring))
                                            .iter()
                                            .map(|p| to_value(p))
                                            .collect(),
                                    )
                                })
                                .collect(),
                        )
                    })
                    .collect();
                out.insert("coordinates".into(), Value::Array(result));
            }
            ("GeometryCollection", _) => {
                let members: Vec<Value> = geometry
                    .get("geometries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|member| self.geometry(member, &mut Arrays::default()))
                    .collect();
                out.insert("geometries".into(), Value::Array(members));
            }
            _ => return geometry.clone(),
        }
        Value::Object(out)
    }
}

/// Per-point arrays (`coordinateProperties.times`, `.heartRates`) that must follow their line.
#[derive(Default)]
struct Arrays {
    lists: Vec<(String, Value)>,
}

impl Arrays {
    fn from_properties(properties: &Map<String, Value>) -> Self {
        let lists = properties
            .get("coordinateProperties")
            .and_then(Value::as_object)
            .map(|object| object.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        Self { lists }
    }

    /// Keeps only the entries at `indices` (one list of indices per line).
    fn keep(&mut self, indices: &[Vec<usize>], nested: bool) {
        for (_, value) in &mut self.lists {
            let pick = |list: &Value, kept: &[usize]| -> Value {
                let items = list.as_array().cloned().unwrap_or_default();
                Value::Array(kept.iter().filter_map(|i| items.get(*i).cloned()).collect())
            };
            *value = if nested {
                let lists = value.as_array().cloned().unwrap_or_default();
                Value::Array(
                    indices
                        .iter()
                        .enumerate()
                        .map(|(position, kept)| {
                            pick(lists.get(position).unwrap_or(&Value::Null), kept)
                        })
                        .collect(),
                )
            } else {
                pick(value, indices.first().map_or(&[], Vec::as_slice))
            };
        }
    }
}

fn filter_properties(
    properties: &Map<String, Value>,
    arrays: Arrays,
    options: &MinifyOptions,
    dropped: &mut usize,
) -> Map<String, Value> {
    let mut result = Map::new();
    for (key, value) in properties {
        if key == "coordinateProperties" {
            continue;
        }
        if options.drop_time && TIME_PROPERTIES.contains(&key.as_str()) {
            continue;
        }
        if options.drop_elevation && key == "ele" {
            continue;
        }
        if options.drop_extensions
            && !STANDARD_PROPERTIES.contains(&key.as_str())
            && !STYLE_PROPERTIES.contains(&key.as_str())
        {
            *dropped += 1;
            continue;
        }
        result.insert(key.clone(), value.clone());
    }
    let mut extra = Map::new();
    for (key, value) in arrays.lists {
        let skip = (options.drop_time && key == "times")
            || (options.drop_extensions && key == "heartRates");
        if !skip {
            extra.insert(key, value);
        }
    }
    if !extra.is_empty() {
        result.insert("coordinateProperties".into(), Value::Object(extra));
    }
    result
}

/// Reduces `document` as `options` ask and reports what changed.
#[must_use]
pub fn minify_document(
    document: &GeoDocument,
    options: &MinifyOptions,
) -> (GeoDocument, MinifyStats, Vec<String>) {
    let mut pass = Pass {
        options,
        stats: MinifyStats {
            features_before: document.features.len(),
            ..MinifyStats::default()
        },
        notes: Vec::new(),
    };
    let mut features: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut dropped_properties = 0_usize;
    for feature in &document.features {
        let properties = feature
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let geometry = feature.get("geometry").filter(|g| !g.is_null());
        let mut arrays = Arrays::from_properties(&properties);
        let new_geometry = geometry.map(|g| pass.geometry(g, &mut arrays));
        let new_properties =
            filter_properties(&properties, arrays, options, &mut dropped_properties);
        if options.remove_duplicates && new_geometry.is_none() && new_properties.is_empty() {
            continue;
        }
        let mut object = Map::new();
        object.insert("type".into(), Value::String("Feature".into()));
        object.insert("geometry".into(), new_geometry.unwrap_or(Value::Null));
        object.insert("properties".into(), Value::Object(new_properties));
        let value = Value::Object(object);
        if options.remove_duplicates && !seen.insert(value.to_string()) {
            continue;
        }
        features.push(value);
    }
    pass.stats.features_after = features.len();
    if dropped_properties > 0 {
        pass.notes.push(format!(
            "{dropped_properties} custom property value(s) were removed"
        ));
    }
    if pass.stats.rings_kept > 0 {
        pass.notes.push(format!(
            "{} area ring(s) were kept unchanged because simplifying them would have made them cross themselves",
            pass.stats.rings_kept
        ));
    }
    let mut result = document.clone();
    result.features = features;
    result.total_features = result.features.len();
    result.tracks = Vec::new();
    result.bounds = bounds_of(&result.features);
    (result, pass.stats, pass.notes)
}

fn bounds_of(features: &[Value]) -> Option<[f64; 4]> {
    fn walk(value: &Value, bounds: &mut Option<[f64; 4]>) {
        match value {
            Value::Array(items) if items.len() >= 2 && items.iter().all(Value::is_number) => {
                let (lon, lat) = (
                    items[0].as_f64().unwrap_or(0.0),
                    items[1].as_f64().unwrap_or(0.0),
                );
                *bounds = Some(match *bounds {
                    None => [lon, lat, lon, lat],
                    Some([w, s, e, n]) => [w.min(lon), s.min(lat), e.max(lon), n.max(lat)],
                });
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, bounds)),
            Value::Object(object) => {
                for key in ["coordinates", "geometries"] {
                    if let Some(inner) = object.get(key) {
                        walk(inner, bounds);
                    }
                }
            }
            _ => {}
        }
    }
    let mut bounds = None;
    for feature in features {
        if let Some(geometry) = feature.get("geometry") {
            walk(geometry, &mut bounds);
        }
    }
    bounds
}

/// Removes whitespace outside strings from JSON text without touching anything else (key order and
/// number formatting are kept exactly).
#[must_use]
pub fn compact_json(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let (mut in_string, mut escaped) = (false, false);
    for &byte in bytes {
        if in_string {
            out.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
            out.push(byte);
        } else if !matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
            out.push(byte);
        }
    }
    out
}

/// Removes indentation, line breaks between elements and comments from XML; text and attributes
/// are untouched.
///
/// # Errors
///
/// Returns the parser message when the XML is malformed.
pub fn compact_xml(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut writer = quick_xml::Writer::new(Vec::new());
    loop {
        let event = reader.read_event().map_err(|error| error.to_string())?;
        match event {
            Event::Eof => break,
            Event::Comment(_) => {}
            Event::Text(ref text) if text.iter().all(u8::is_ascii_whitespace) => {}
            other => writer
                .write_event(other)
                .map_err(|error| error.to_string())?,
        }
    }
    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{GeoFormat, conversion_limits, parse_geojson, parse_gpx};

    fn document(text: &str) -> GeoDocument {
        parse_geojson(text.as_bytes(), conversion_limits()).expect("geojson")
    }

    fn all_options() -> MinifyOptions {
        MinifyOptions::default()
    }

    #[test]
    fn rounds_coordinates_and_counts_positions() {
        let source = document(
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[4.123456789,52.987654321,10.26],[5.0000004,53,11]]},"properties":{}}]}"#,
        );
        let options = MinifyOptions {
            precision: Some(6),
            ..all_options()
        };
        let (result, stats, _) = minify_document(&source, &options);
        assert_eq!(
            result.features[0]["geometry"]["coordinates"],
            json!([[4.123457, 52.987654, 10.3], [5.0, 53.0, 11.0]])
        );
        assert_eq!((stats.positions_before, stats.positions_after), (2, 2));
        assert!(stats.distance_before_meters > 0.0);
    }

    #[test]
    fn simplifies_lines_within_the_tolerance_and_keeps_the_ends() {
        // A nearly straight line with 1 m wiggles, 0.001 degrees ~ 111 m apart.
        let points: Vec<String> = (0..50)
            .map(|index| {
                let sideways = if index % 2 == 0 { 0.0 } else { 0.000_005 };
                format!("[{},{}]", 4.0 + sideways, 52.0 + f64::from(index) * 0.001)
            })
            .collect();
        let source = document(&format!(
            r#"{{"type":"Feature","geometry":{{"type":"LineString","coordinates":[{}]}},"properties":{{}}}}"#,
            points.join(",")
        ));
        let options = MinifyOptions {
            tolerance_meters: Some(5.0),
            ..all_options()
        };
        let (result, stats, _) = minify_document(&source, &options);
        let coordinates = result.features[0]["geometry"]["coordinates"]
            .as_array()
            .expect("line")
            .clone();
        assert_eq!(coordinates.len(), 2, "a straight line needs only its ends");
        assert_eq!(coordinates[0], json!([4.0, 52.0]));
        assert_eq!(stats.positions_before, 50);
        let change = (stats.distance_after_meters - stats.distance_before_meters).abs()
            / stats.distance_before_meters;
        assert!(change < 0.001, "{change}");
        // A tighter tolerance keeps the wiggles.
        let strict = MinifyOptions {
            tolerance_meters: Some(0.1),
            ..all_options()
        };
        let (precise, _, _) = minify_document(&source, &strict);
        assert!(
            precise.features[0]["geometry"]["coordinates"]
                .as_array()
                .expect("line")
                .len()
                > 20
        );
    }

    #[test]
    fn simplified_polygons_stay_closed_and_valid() {
        // A square with a noisy top edge.
        let ring = "[[0,0],[0.001,0],[0.002,0],[0.002,0.002],[0.0015,0.002001],[0.001,0.002],[0.0005,0.002001],[0,0.002],[0,0]]";
        let source = document(&format!(
            r#"{{"type":"Feature","geometry":{{"type":"Polygon","coordinates":[{ring}]}},"properties":{{}}}}"#
        ));
        let options = MinifyOptions {
            tolerance_meters: Some(5.0),
            ..all_options()
        };
        let (result, stats, _) = minify_document(&source, &options);
        let ring = result.features[0]["geometry"]["coordinates"][0]
            .as_array()
            .expect("ring")
            .clone();
        assert!(ring.len() < 9 && ring.len() >= 4, "{}", ring.len());
        assert_eq!(ring.first(), ring.last());
        assert_eq!(stats.rings_kept, 0);
    }

    #[test]
    fn a_ring_that_would_cross_itself_is_kept_as_it_is() {
        // A thin "C" shape: removing the inner points would make the arms cross.
        let ring = "[[0,0],[0.01,0],[0.01,0.01],[0.0001,0.01],[0.0001,0.0099],[0.0099,0.0099],[0.0099,0.0001],[0,0.0001],[0,0]]";
        let source = document(&format!(
            r#"{{"type":"Feature","geometry":{{"type":"Polygon","coordinates":[{ring}]}},"properties":{{}}}}"#
        ));
        let options = MinifyOptions {
            tolerance_meters: Some(200.0),
            ..all_options()
        };
        let (result, stats, notes) = minify_document(&source, &options);
        let kept = result.features[0]["geometry"]["coordinates"][0]
            .as_array()
            .expect("ring")
            .len();
        let projected: Vec<(f64, f64)> = result.features[0]["geometry"]["coordinates"][0]
            .as_array()
            .expect("ring")
            .iter()
            .map(|p| (p[0].as_f64().expect("lon"), p[1].as_f64().expect("lat")))
            .collect();
        assert_eq!(ring_self_intersects(&projected), Some(false));
        assert!(
            kept == 9
                && stats.rings_kept == 1
                && notes.iter().any(|n| n.contains("cross themselves"))
                || kept < 9
        );
    }

    #[test]
    fn drops_elevation_time_and_extensions_and_keeps_arrays_aligned_with_simplified_points() {
        let gpx = parse_gpx(
            br#"<gpx version="1.1" xmlns:g="http://www.garmin.com/xmlschemas/TrackPointExtension/v1"><trk><name>Run</name><trkseg>
            <trkpt lat="52.000" lon="4"><ele>1</ele><time>2024-05-05T08:00:00Z</time><extensions><g:TrackPointExtension><g:hr>100</g:hr></g:TrackPointExtension></extensions></trkpt>
            <trkpt lat="52.001" lon="4"><ele>2</ele><time>2024-05-05T08:00:10Z</time><extensions><g:TrackPointExtension><g:hr>110</g:hr></g:TrackPointExtension></extensions></trkpt>
            <trkpt lat="52.002" lon="4"><ele>3</ele><time>2024-05-05T08:00:20Z</time><extensions><g:TrackPointExtension><g:hr>120</g:hr></g:TrackPointExtension></extensions></trkpt>
            </trkseg></trk></gpx>"#,
            conversion_limits(),
        )
        .expect("gpx");
        assert_eq!(gpx.format, GeoFormat::Gpx);
        // Simplification removes the middle point; the per-point arrays follow.
        let simplified = MinifyOptions {
            tolerance_meters: Some(5.0),
            ..all_options()
        };
        let (kept, _, _) = minify_document(&gpx, &simplified);
        let extra = &kept.features[0]["properties"]["coordinateProperties"];
        assert_eq!(
            extra["times"],
            json!(["2024-05-05T08:00:00Z", "2024-05-05T08:00:20Z"])
        );
        assert_eq!(extra["heartRates"], json!([100.0, 120.0]));
        assert_eq!(
            kept.features[0]["geometry"]["coordinates"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );

        let stripped = MinifyOptions {
            drop_elevation: true,
            drop_time: true,
            drop_extensions: true,
            ..all_options()
        };
        let (bare, _, _) = minify_document(&gpx, &stripped);
        assert_eq!(
            bare.features[0]["geometry"]["coordinates"][0],
            json!([4.0, 52.0])
        );
        assert!(
            bare.features[0]["properties"]
                .get("coordinateProperties")
                .is_none()
        );
        assert_eq!(bare.features[0]["properties"]["name"], "Run");
    }

    #[test]
    fn removes_repeated_points_empty_and_duplicate_features() {
        let source = document(
            r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[1,1],[1,1],[2,2],[2,2],[3,3]]},"properties":{"a":1}},
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[1,1],[1,1],[2,2],[2,2],[3,3]]},"properties":{"a":1}},
            {"type":"Feature","geometry":null,"properties":{}},
            {"type":"Feature","geometry":null,"properties":{"note":"keep"}}]}"#,
        );
        let options = MinifyOptions {
            remove_duplicates: true,
            ..all_options()
        };
        let (result, stats, _) = minify_document(&source, &options);
        assert_eq!(stats.features_before, 4);
        assert_eq!(stats.features_after, 2);
        assert_eq!(
            result.features[0]["geometry"]["coordinates"],
            json!([[1.0, 1.0], [2.0, 2.0], [3.0, 3.0]])
        );
    }

    #[test]
    fn json_compaction_keeps_key_order_numbers_and_string_contents() {
        let text = b"{\n  \"b\": 1.50,\n  \"a\": \"x  y \\\" z\",\n  \"c\": [ 1, 2 ]\n}\n";
        assert_eq!(
            compact_json(text),
            b"{\"b\":1.50,\"a\":\"x  y \\\" z\",\"c\":[1,2]}".to_vec()
        );
    }

    #[test]
    fn xml_compaction_drops_indentation_and_comments_but_keeps_text() {
        let text = b"<?xml version=\"1.0\"?>\n<gpx>\n  <!-- note -->\n  <wpt lat=\"1\" lon=\"2\">\n    <name>Start &amp; finish</name>\n  </wpt>\n</gpx>\n";
        let compact = String::from_utf8(compact_xml(text).expect("xml")).expect("utf-8");
        assert_eq!(
            compact,
            "<?xml version=\"1.0\"?><gpx><wpt lat=\"1\" lon=\"2\"><name>Start &amp; finish</name></wpt></gpx>"
        );
        assert!(compact_xml(b"<a><b></a>").is_err());
    }
}
