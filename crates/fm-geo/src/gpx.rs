//! GPX 1.0 / 1.1 reading (task 0279).
//!
//! Waypoints become Point features, routes LineString features and tracks LineString or
//! MultiLineString features (one segment each). XML is read as a stream of events; entities are
//! never expanded and DOCTYPE declarations that declare entities are refused, so a hostile file
//! cannot read local files or explode in memory.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::model::{Bounds, GeoDocument, GeoFormat, GeoIssue, Limits};
use crate::time::parse_timestamp;
use crate::track_stats::{TrackPoint, summarize};

/// Most points per track kept for drawing; statistics always use every point.
const DISPLAY_POINTS_PER_TRACK: usize = 50_000;

/// Why a GPX file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GpxError {
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
    /// The XML is not a GPX document.
    #[error("{0}")]
    NotGpx(String),
    /// The document declares entities.
    #[error("DOCTYPE declarations with entities are not allowed")]
    Entities,
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

fn attribute(start: &BytesStart<'_>, wanted: &str) -> Option<String> {
    start.attributes().flatten().find_map(|attribute| {
        (local(attribute.key.as_ref()) == wanted)
            .then(|| String::from_utf8_lossy(&attribute.value).into_owned())
    })
}

#[derive(Default, Clone)]
struct Point {
    longitude: f64,
    latitude: f64,
    elevation: Option<f64>,
    time_text: Option<String>,
    heart_rate: Option<f64>,
    properties: Map<String, Value>,
}

#[derive(Default)]
struct Container {
    properties: Map<String, Value>,
    /// Segments of a track; a route uses the first one.
    segments: Vec<Vec<Point>>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Scope {
    Waypoint,
    Route,
    Track,
    Segment,
    Point,
}

fn number(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn set_text(properties: &mut Map<String, Value>, key: &str, text: &str) {
    if !text.is_empty() {
        properties.insert(key.to_owned(), Value::String(text.to_owned()));
    }
}

/// Reads GPX bytes.
///
/// # Errors
///
/// Returns the first XML problem with its position, or why the document is not GPX.
#[allow(clippy::too_many_lines)]
pub fn parse_gpx(bytes: &[u8], limits: Limits) -> Result<GeoDocument, GpxError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_end_names = true;

    let mut stack: Vec<String> = Vec::new();
    let mut scopes: Vec<Scope> = Vec::new();
    let mut text = String::new();
    let mut seen_root = false;
    let mut waypoints: Vec<Point> = Vec::new();
    let mut routes: Vec<Container> = Vec::new();
    let mut tracks: Vec<Container> = Vec::new();
    let mut point = Point::default();
    let mut dropped_points = 0_usize;

    loop {
        let event = reader.read_event().map_err(|error| {
            let (line, column) =
                line_column(bytes, usize::try_from(reader.error_position()).unwrap_or(0));
            GpxError::Syntax {
                message: error.to_string(),
                line,
                column,
            }
        })?;
        match event {
            Event::Eof => break,
            Event::DocType(doctype) => {
                if String::from_utf8_lossy(&doctype).contains("ENTITY") {
                    return Err(GpxError::Entities);
                }
            }
            Event::Start(ref start) | Event::Empty(ref start) => {
                let is_start = matches!(event, Event::Start(_));
                let name = local(start.name().as_ref());
                if !seen_root {
                    if name != "gpx" {
                        return Err(GpxError::NotGpx(format!(
                            "the root element is <{name}>, not <gpx>"
                        )));
                    }
                    seen_root = true;
                }
                let parent = scopes.last().copied();
                let scope = match (name.as_str(), parent) {
                    ("wpt", None) => Some(Scope::Waypoint),
                    ("rte", None) => Some(Scope::Route),
                    ("trk", None) => Some(Scope::Track),
                    ("trkseg", Some(Scope::Track)) => Some(Scope::Segment),
                    ("rtept", Some(Scope::Route)) | ("trkpt", Some(Scope::Segment)) => {
                        Some(Scope::Point)
                    }
                    _ => None,
                };
                if let Some(scope) = scope {
                    match scope {
                        Scope::Route => routes.push(Container::default()),
                        Scope::Track => tracks.push(Container::default()),
                        Scope::Segment => {
                            if let Some(track) = tracks.last_mut() {
                                track.segments.push(Vec::new());
                            }
                        }
                        Scope::Waypoint | Scope::Point => {
                            point = Point::default();
                            let latitude = attribute(start, "lat").and_then(|v| number(&v));
                            let longitude = attribute(start, "lon").and_then(|v| number(&v));
                            match (latitude, longitude) {
                                (Some(lat), Some(lon))
                                    if (-90.0..=90.0).contains(&lat)
                                        && (-180.0..=180.0).contains(&lon) =>
                                {
                                    point.latitude = lat;
                                    point.longitude = lon;
                                }
                                _ => {
                                    point.latitude = f64::NAN;
                                    dropped_points += 1;
                                }
                            }
                        }
                    }
                    if is_start {
                        scopes.push(scope);
                    } else {
                        finish_scope(
                            scope,
                            parent,
                            &mut point,
                            &mut waypoints,
                            &mut routes,
                            &mut tracks,
                        );
                    }
                } else if name == "link"
                    && let Some(href) = attribute(start, "href")
                {
                    let target = match scopes.last() {
                        Some(Scope::Waypoint | Scope::Point) => Some(&mut point.properties),
                        Some(Scope::Route) => routes.last_mut().map(|c| &mut c.properties),
                        Some(Scope::Track) => tracks.last_mut().map(|c| &mut c.properties),
                        _ => None,
                    };
                    if let Some(properties) = target {
                        set_text(properties, "link", &href);
                    }
                }
                if is_start {
                    stack.push(name);
                    text.clear();
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
                let closing_scope = matches!(
                    (name.as_str(), scopes.last()),
                    ("wpt", Some(Scope::Waypoint))
                        | ("rte", Some(Scope::Route))
                        | ("trk", Some(Scope::Track))
                        | ("trkseg", Some(Scope::Segment))
                        | ("rtept" | "trkpt", Some(Scope::Point))
                );
                if closing_scope {
                    if let Some(scope) = scopes.pop() {
                        finish_scope(
                            scope,
                            scopes.last().copied(),
                            &mut point,
                            &mut waypoints,
                            &mut routes,
                            &mut tracks,
                        );
                    }
                    continue;
                }
                if value.is_empty() {
                    continue;
                }
                match scopes.last() {
                    Some(Scope::Waypoint | Scope::Point) => match name.as_str() {
                        "ele" => point.elevation = number(&value),
                        "time" => point.time_text = Some(value),
                        "hr" => point.heart_rate = number(&value),
                        "name" | "desc" | "cmt" | "sym" | "type" => {
                            set_text(&mut point.properties, &name, &value);
                        }
                        _ => {}
                    },
                    Some(Scope::Route) => {
                        if matches!(name.as_str(), "name" | "desc" | "cmt" | "type" | "src")
                            && let Some(route) = routes.last_mut()
                        {
                            set_text(&mut route.properties, &name, &value);
                        }
                    }
                    Some(Scope::Track) => {
                        if matches!(name.as_str(), "name" | "desc" | "cmt" | "type" | "src")
                            && let Some(track) = tracks.last_mut()
                        {
                            set_text(&mut track.properties, &name, &value);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if !seen_root {
        return Err(GpxError::NotGpx("the document is empty".into()));
    }
    Ok(build_document(
        &waypoints,
        &routes,
        &tracks,
        dropped_points,
        limits,
    ))
}

fn finish_scope(
    scope: Scope,
    parent: Option<Scope>,
    point: &mut Point,
    waypoints: &mut Vec<Point>,
    routes: &mut [Container],
    tracks: &mut [Container],
) {
    match scope {
        Scope::Waypoint => {
            if point.latitude.is_finite() {
                waypoints.push(std::mem::take(point));
            }
        }
        Scope::Point => {
            if point.latitude.is_finite() {
                let finished = std::mem::take(point);
                // Route points go into the route's single segment, track points into the
                // current track segment.
                if parent == Some(Scope::Segment) {
                    if let Some(segment) = tracks
                        .last_mut()
                        .and_then(|track| track.segments.last_mut())
                    {
                        segment.push(finished);
                    }
                } else if let Some(route) = routes.last_mut() {
                    if route.segments.is_empty() {
                        route.segments.push(Vec::new());
                    }
                    if let Some(segment) = route.segments.first_mut() {
                        segment.push(finished);
                    }
                }
            }
        }
        Scope::Route | Scope::Track | Scope::Segment => {}
    }
}

fn position(point: &Point, with_elevation: bool) -> Value {
    match (with_elevation, point.elevation) {
        (true, Some(elevation)) => json!([point.longitude, point.latitude, elevation]),
        _ => json!([point.longitude, point.latitude]),
    }
}

fn waypoint_properties(point: &Point) -> Map<String, Value> {
    let mut properties = point.properties.clone();
    if let Some(elevation) = point.elevation {
        properties.insert("ele".into(), json!(elevation));
    }
    if let Some(time) = &point.time_text {
        properties.insert("time".into(), json!(time));
    }
    properties.insert("gpxType".into(), json!("waypoint"));
    properties
}

fn thin(points: &[Point], target: usize) -> Vec<&Point> {
    if points.len() <= target || target < 2 {
        return points.iter().collect();
    }
    let last = points.len() - 1;
    (0..target)
        .map(|index| &points[index * last / (target - 1)])
        .collect()
}

fn feature(geometry: Value, properties: Map<String, Value>) -> Value {
    json!({ "type": "Feature", "geometry": geometry, "properties": properties })
}

#[allow(clippy::too_many_lines)]
fn build_document(
    waypoints: &[Point],
    routes: &[Container],
    tracks: &[Container],
    dropped_points: usize,
    limits: Limits,
) -> GeoDocument {
    let mut features: Vec<Value> = Vec::new();
    let mut warnings: Vec<GeoIssue> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut summaries = Vec::new();
    let mut bounds: Option<Bounds> = None;
    let mut positions = 0_usize;
    let mut total_features = 0_usize;
    let mut truncated = false;
    let mut include = |longitude: f64, latitude: f64| {
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

    for point in waypoints {
        total_features += 1;
        if features.len() >= limits.max_features {
            truncated = true;
            continue;
        }
        include(point.longitude, point.latitude);
        positions += 1;
        features.push(feature(
            json!({ "type": "Point", "coordinates": position(point, true) }),
            waypoint_properties(point),
        ));
    }
    for (index, route) in routes.iter().enumerate() {
        total_features += 1;
        let points: &[Point] = route.segments.first().map_or(&[], Vec::as_slice);
        if points.len() < 2 {
            skipped.push(format!("route {} has fewer than two points", index + 1));
            continue;
        }
        if features.len() >= limits.max_features {
            truncated = true;
            continue;
        }
        let elevation_everywhere = points.iter().all(|p| p.elevation.is_some());
        let shown = thin(points, DISPLAY_POINTS_PER_TRACK);
        for point in &shown {
            include(point.longitude, point.latitude);
        }
        positions += shown.len();
        let mut properties = route.properties.clone();
        properties.insert("gpxType".into(), json!("route"));
        features.push(feature(
            json!({
                "type": "LineString",
                "coordinates": shown.iter().map(|p| position(p, elevation_everywhere)).collect::<Vec<_>>()
            }),
            properties,
        ));
    }
    for (index, track) in tracks.iter().enumerate() {
        total_features += 1;
        let segments: Vec<&Vec<Point>> = track
            .segments
            .iter()
            .filter(|segment| segment.len() >= 2)
            .collect();
        let short_segments = track.segments.len() - segments.len();
        if segments.is_empty() {
            skipped.push(format!(
                "track {} has no segment with two points",
                index + 1
            ));
            continue;
        }
        if short_segments > 0 {
            warnings.push(GeoIssue {
                path: format!("/trk/{index}"),
                message: format!(
                    "{short_segments} segment(s) with fewer than two points were left out"
                ),
            });
        }
        if features.len() >= limits.max_features || positions >= limits.max_positions {
            truncated = true;
            continue;
        }
        let elevation_everywhere = segments
            .iter()
            .all(|s| s.iter().all(|p| p.elevation.is_some()));
        let per_segment = if limits.lossless {
            usize::MAX
        } else {
            (DISPLAY_POINTS_PER_TRACK / segments.len()).max(2)
        };
        let mut decimated = false;
        let lines: Vec<Vec<Value>> = segments
            .iter()
            .map(|segment| {
                let shown = thin(segment, per_segment);
                decimated |= shown.len() < segment.len();
                for point in &shown {
                    include(point.longitude, point.latitude);
                }
                positions += shown.len();
                shown
                    .iter()
                    .map(|p| position(p, elevation_everywhere))
                    .collect()
            })
            .collect();
        if decimated {
            warnings.push(GeoIssue {
                path: format!("/trk/{index}"),
                message: "the track has more points than are drawn; the line is thinned for display and the statistics use every point".into(),
            });
        }
        let geometry = if lines.len() == 1 {
            json!({ "type": "LineString", "coordinates": lines[0] })
        } else {
            json!({ "type": "MultiLineString", "coordinates": lines })
        };
        let mut properties = track.properties.clone();
        properties.insert("gpxType".into(), json!("track"));
        if limits.lossless {
            // Per-point data in the GeoJSON convention used by common converters.
            let times: Vec<Vec<Value>> = segments
                .iter()
                .map(|segment| {
                    segment
                        .iter()
                        .map(|p| p.time_text.as_ref().map_or(Value::Null, |t| json!(t)))
                        .collect()
                })
                .collect();
            let rates: Vec<Vec<Value>> = segments
                .iter()
                .map(|segment| {
                    segment
                        .iter()
                        .map(|p| p.heart_rate.map_or(Value::Null, |r| json!(r)))
                        .collect()
                })
                .collect();
            let mut extra = Map::new();
            let unwrap = |lists: Vec<Vec<Value>>| -> Value {
                if lists.len() == 1 {
                    Value::Array(lists.into_iter().next().unwrap_or_default())
                } else {
                    Value::Array(lists.into_iter().map(Value::Array).collect())
                }
            };
            if times.iter().flatten().any(|v| !v.is_null()) {
                extra.insert("times".into(), unwrap(times));
            }
            if rates.iter().flatten().any(|v| !v.is_null()) {
                extra.insert("heartRates".into(), unwrap(rates));
            }
            if !extra.is_empty() {
                properties.insert("coordinateProperties".into(), Value::Object(extra));
            }
        }
        let feature_index = features.len();
        features.push(feature(geometry, properties));

        let track_points: Vec<Vec<TrackPoint>> = segments
            .iter()
            .map(|segment| {
                segment
                    .iter()
                    .map(|p| TrackPoint {
                        longitude: p.longitude,
                        latitude: p.latitude,
                        elevation: p.elevation,
                        time: p.time_text.as_deref().and_then(parse_timestamp),
                        heart_rate: p.heart_rate,
                    })
                    .collect()
            })
            .collect();
        let name = track
            .properties
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let has_text_times = segments
            .iter()
            .flat_map(|s| s.iter())
            .any(|p| p.time_text.is_some());
        let has_usable_times = track_points.iter().flatten().all(|p| p.time.is_some());
        if has_text_times && !has_usable_times {
            warnings.push(GeoIssue {
                path: format!("/trk/{index}"),
                message: "some timestamps are missing or have no time zone, so durations and speeds are not shown".into(),
            });
        }
        summaries.push(summarize(name, feature_index, &track_points));
    }
    if dropped_points > 0 {
        warnings.push(GeoIssue {
            path: String::new(),
            message: format!(
                "{dropped_points} point(s) with missing or invalid coordinates were left out"
            ),
        });
    }
    GeoDocument {
        format: GeoFormat::Gpx,
        features,
        total_features,
        truncated,
        bounds,
        warnings,
        skipped,
        tracks: summaries,
        resources: std::collections::BTreeMap::new(),
        overlays: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> GeoDocument {
        parse_gpx(text.as_bytes(), Limits::default()).expect("valid gpx")
    }

    /// A Garmin-style GPX 1.1 file: metadata, a waypoint, one track with two segments, hr in the
    /// namespaced TrackPointExtension.
    const GARMIN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx creator="Garmin Connect" version="1.1" xmlns="http://www.topografix.com/GPX/1/1"
 xmlns:ns3="http://www.garmin.com/xmlschemas/TrackPointExtension/v1">
 <metadata><name>Morning &amp; run</name><link href="https://example.com"><text>Example</text></link></metadata>
 <wpt lat="52.0" lon="4.0"><ele>1.5</ele><name>Start &amp; finish</name><sym>Flag</sym><link href="https://example.com/wpt"/></wpt>
 <trk><name>Run</name><type>running</type>
  <trkseg>
   <trkpt lat="52.000" lon="4.000"><ele>10</ele><time>2024-05-05T08:00:00Z</time><extensions><ns3:TrackPointExtension><ns3:hr>120</ns3:hr></ns3:TrackPointExtension></extensions></trkpt>
   <trkpt lat="52.001" lon="4.000"><ele>15</ele><time>2024-05-05T08:00:10Z</time><extensions><ns3:TrackPointExtension><ns3:hr>130</ns3:hr></ns3:TrackPointExtension></extensions></trkpt>
   <trkpt lat="52.002" lon="4.000"><ele>20</ele><time>2024-05-05T08:00:20Z</time></trkpt>
  </trkseg>
  <trkseg>
   <trkpt lat="52.003" lon="4.000"><ele>22</ele><time>2024-05-05T08:00:30Z</time></trkpt>
   <trkpt lat="52.004" lon="4.000"><ele>20</ele><time>2024-05-05T08:00:40Z</time></trkpt>
  </trkseg>
 </trk>
</gpx>"#;

    #[test]
    fn reads_a_garmin_file_with_waypoints_tracks_and_statistics() {
        let document = parse(GARMIN);
        assert_eq!(document.format, GeoFormat::Gpx);
        assert_eq!(document.features.len(), 2);
        let waypoint = &document.features[0];
        assert_eq!(waypoint["geometry"]["coordinates"], json!([4.0, 52.0, 1.5]));
        assert_eq!(waypoint["properties"]["name"], "Start & finish");
        assert_eq!(waypoint["properties"]["sym"], "Flag");
        assert_eq!(waypoint["properties"]["link"], "https://example.com/wpt");
        let track = &document.features[1];
        assert_eq!(track["geometry"]["type"], "MultiLineString");
        assert_eq!(track["properties"]["name"], "Run");
        assert_eq!(track["properties"]["type"], "running");
        assert_eq!(document.bounds, Some([4.0, 52.0, 4.0, 52.004]));

        assert_eq!(document.tracks.len(), 1);
        let summary = &document.tracks[0];
        assert_eq!(summary.name.as_deref(), Some("Run"));
        assert_eq!(summary.feature_index, 1);
        assert_eq!(summary.points, 5);
        // Two 2-point-gap segments: (52.000-52.002) and (52.003-52.004): about 3 x 111 m.
        assert!(
            (summary.distance_meters - 3.0 * 111.2).abs() < 3.0,
            "{}",
            summary.distance_meters
        );
        assert_eq!(summary.elapsed_seconds, Some(40.0));
        // 10 -> 15 -> 20 climbs 10 m; the later +2 m and -2 m are below the noise floor.
        assert_eq!(summary.ascent_meters, Some(10.0));
        assert_eq!(summary.max_elevation, Some(22.0));
        assert_eq!(summary.start_time.as_deref(), Some("2024-05-05T08:00:00Z"));
        assert_eq!(summary.profile[0].heart_rate, Some(120.0));
        assert!(document.warnings.is_empty(), "{:?}", document.warnings);
    }

    #[test]
    fn reads_gpx_1_0_routes_with_a_single_segment() {
        let document = parse(
            r#"<gpx version="1.0" creator="old" xmlns="http://www.topografix.com/GPX/1/0">
            <rte><name>Route A</name><rtept lat="1" lon="2"><ele>3</ele></rtept><rtept lat="1.5" lon="2.5"><ele>4</ele></rtept></rte>
            <trk><trkseg><trkpt lat="1" lon="1"><time>2024-01-01T00:00:00Z</time></trkpt><trkpt lat="1.1" lon="1"><time>2024-01-01T00:10:00Z</time></trkpt></trkseg></trk>
            </gpx>"#,
        );
        assert_eq!(document.features.len(), 2);
        let route = &document.features[0];
        assert_eq!(route["geometry"]["type"], "LineString");
        assert_eq!(route["geometry"]["coordinates"][0], json!([2.0, 1.0, 3.0]));
        assert_eq!(route["properties"]["gpxType"], "route");
        // The track keeps its own points; the route's did not leak into it.
        assert_eq!(
            document.features[1]["geometry"]["coordinates"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert!(document.tracks[0].average_speed.is_some());
    }

    #[test]
    fn omits_time_statistics_when_timestamps_have_no_zone_or_are_missing() {
        let document = parse(
            r#"<gpx version="1.1"><trk><trkseg>
            <trkpt lat="1" lon="1"><time>2024-01-01T00:00:00</time></trkpt>
            <trkpt lat="1.01" lon="1"><time>2024-01-01T00:10:00</time></trkpt></trkseg></trk></gpx>"#,
        );
        let summary = &document.tracks[0];
        assert_eq!(summary.elapsed_seconds, None);
        assert_eq!(summary.average_speed, None);
        assert!(summary.distance_meters > 1000.0);
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("time zone"))
        );
    }

    #[test]
    fn skips_degenerate_tracks_and_invalid_points_with_a_note() {
        let document = parse(
            r#"<gpx version="1.1">
            <wpt lat="abc" lon="1"/><wpt lat="95" lon="1"/><wpt lat="10" lon="10"><name>ok</name></wpt>
            <trk><trkseg><trkpt lat="1" lon="1"/></trkseg></trk>
            <rte><rtept lat="1" lon="1"/></rte>
            </gpx>"#,
        );
        assert_eq!(document.features.len(), 1);
        assert_eq!(document.skipped.len(), 2);
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("2 point(s)"))
        );
    }

    #[test]
    fn long_tracks_are_thinned_for_display_but_not_for_statistics() {
        let mut body = String::from(r#"<gpx version="1.1"><trk><trkseg>"#);
        for index in 0..120_000 {
            body.push_str(&format!(
                r#"<trkpt lat="{}" lon="0"/>"#,
                f64::from(index) * 0.000_001
            ));
        }
        body.push_str("</trkseg></trk></gpx>");
        let document = parse(&body);
        let drawn = document.features[0]["geometry"]["coordinates"]
            .as_array()
            .map(Vec::len);
        assert_eq!(drawn, Some(DISPLAY_POINTS_PER_TRACK));
        assert_eq!(document.tracks[0].points, 120_000);
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("thinned"))
        );
        let first = &document.features[0]["geometry"]["coordinates"][0];
        let last = document.features[0]["geometry"]["coordinates"]
            .as_array()
            .and_then(|c| c.last())
            .cloned();
        assert_eq!(first, &json!([0.0, 0.0]));
        assert_eq!(last, Some(json!([0.0, 119_999.0 * 0.000_001])));
    }

    #[test]
    fn rejects_entities_wrong_roots_and_malformed_xml_with_a_position() {
        let hostile = r#"<?xml version="1.0"?>
<!DOCTYPE gpx [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<gpx version="1.1"><wpt lat="1" lon="1"><name>&xxe;</name></wpt></gpx>"#;
        assert_eq!(
            parse_gpx(hostile.as_bytes(), Limits::default()),
            Err(GpxError::Entities)
        );
        assert!(matches!(
            parse_gpx(b"<kml/>", Limits::default()),
            Err(GpxError::NotGpx(_))
        ));
        match parse_gpx(
            b"<gpx>\n  <wpt lat=\"1\" lon=\"1\">\n </gpx>",
            Limits::default(),
        ) {
            Err(GpxError::Syntax { line, .. }) => assert!(line >= 2),
            other => panic!("expected a syntax error, got {other:?}"),
        }
        assert!(matches!(
            parse_gpx(b"", Limits::default()),
            Err(GpxError::NotGpx(_))
        ));
    }

    #[test]
    fn handles_prefixed_elements_and_an_empty_file() {
        let document = parse(
            r#"<g:gpx xmlns:g="http://www.topografix.com/GPX/1/1" version="1.1"><g:wpt lat="1" lon="2"><g:name>x</g:name></g:wpt></g:gpx>"#,
        );
        assert_eq!(document.features[0]["properties"]["name"], "x");
        let empty = parse(r#"<gpx version="1.1"></gpx>"#);
        assert!(empty.features.is_empty());
        assert_eq!(empty.bounds, None);
    }
}
