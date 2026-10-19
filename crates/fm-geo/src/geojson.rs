//! GeoJSON reading and validation (RFC 7946).

use serde_json::{Map, Value};
use thiserror::Error;

use crate::model::{Bounds, GeoDocument, GeoFormat, GeoIssue, Limits};

const MAX_GEOMETRY_DEPTH: usize = 8;

/// Why a document could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GeoJsonError {
    /// The text is not valid JSON.
    #[error("invalid JSON at line {line}, column {column}: {message}")]
    Syntax {
        /// Parser message.
        message: String,
        /// One-based line.
        line: usize,
        /// One-based column.
        column: usize,
    },
    /// The JSON is not valid GeoJSON.
    #[error("invalid GeoJSON at {}: {}", .0.path, .0.message)]
    Invalid(GeoIssue),
}

fn invalid(path: &str, message: impl Into<String>) -> GeoJsonError {
    GeoJsonError::Invalid(GeoIssue {
        path: path.to_owned(),
        message: message.into(),
    })
}

#[derive(Default)]
struct Scan {
    warnings: Vec<GeoIssue>,
    positions: usize,
    bounds: Option<Bounds>,
    out_of_range: usize,
    first_out_of_range: Option<String>,
}

impl Scan {
    fn include(&mut self, longitude: f64, latitude: f64, path: &str) {
        self.positions += 1;
        if !(-180.0..=180.0).contains(&longitude) || !(-90.0..=90.0).contains(&latitude) {
            self.out_of_range += 1;
            self.first_out_of_range
                .get_or_insert_with(|| path.to_owned());
        }
        self.bounds = Some(match self.bounds {
            None => [longitude, latitude, longitude, latitude],
            Some([west, south, east, north]) => [
                west.min(longitude),
                south.min(latitude),
                east.max(longitude),
                north.max(latitude),
            ],
        });
    }

    fn warn(&mut self, path: &str, message: impl Into<String>) {
        self.warnings.push(GeoIssue {
            path: path.to_owned(),
            message: message.into(),
        });
    }
}

fn position(value: &Value, path: &str, scan: &mut Scan) -> Result<(), GeoJsonError> {
    let parts = value
        .as_array()
        .ok_or_else(|| invalid(path, "a position must be an array of numbers"))?;
    if parts.len() < 2 {
        return Err(invalid(
            path,
            "a position needs at least longitude and latitude",
        ));
    }
    let number = |index: usize| {
        parts[index]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                invalid(
                    &format!("{path}/{index}"),
                    "coordinates must be finite numbers",
                )
            })
    };
    let (longitude, latitude) = (number(0)?, number(1)?);
    for index in 2..parts.len() {
        number(index)?;
    }
    scan.include(longitude, latitude, path);
    Ok(())
}

fn positions<'a>(value: &'a Value, path: &str) -> Result<&'a Vec<Value>, GeoJsonError> {
    value
        .as_array()
        .ok_or_else(|| invalid(path, "coordinates must be an array"))
}

fn line(value: &Value, path: &str, scan: &mut Scan, minimum: usize) -> Result<(), GeoJsonError> {
    let items = positions(value, path)?;
    if items.len() < minimum {
        return Err(invalid(
            path,
            format!("needs at least {minimum} positions, found {}", items.len()),
        ));
    }
    for (index, item) in items.iter().enumerate() {
        position(item, &format!("{path}/{index}"), scan)?;
    }
    Ok(())
}

fn polygon(value: &Value, path: &str, scan: &mut Scan) -> Result<(), GeoJsonError> {
    for (ring_index, ring) in positions(value, path)?.iter().enumerate() {
        let ring_path = format!("{path}/{ring_index}");
        line(ring, &ring_path, scan, 4)?;
        let items = positions(ring, &ring_path)?;
        if items.first() != items.last() {
            scan.warn(
                &ring_path,
                "the ring is not closed (first and last position differ)",
            );
        }
    }
    Ok(())
}

fn geometry(value: &Value, path: &str, scan: &mut Scan, depth: usize) -> Result<(), GeoJsonError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(path, "a geometry must be an object or null"))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("{path}/type"), "a geometry needs a type"))?;
    if kind == "GeometryCollection" {
        if depth >= MAX_GEOMETRY_DEPTH {
            return Err(invalid(path, "geometry collections are nested too deeply"));
        }
        let members = object
            .get("geometries")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(&format!("{path}/geometries"), "needs a geometries array"))?;
        for (index, member) in members.iter().enumerate() {
            geometry(
                member,
                &format!("{path}/geometries/{index}"),
                scan,
                depth + 1,
            )?;
        }
        return Ok(());
    }
    let coordinates_path = format!("{path}/coordinates");
    let coordinates = object
        .get("coordinates")
        .ok_or_else(|| invalid(&coordinates_path, "the geometry needs coordinates"))?;
    match kind {
        // An empty array is the RFC's way of writing an empty geometry.
        "Point" => {
            if !positions(coordinates, &coordinates_path)?.is_empty() {
                position(coordinates, &coordinates_path, scan)?;
            }
        }
        "MultiPoint" => {
            for (index, item) in positions(coordinates, &coordinates_path)?
                .iter()
                .enumerate()
            {
                position(item, &format!("{coordinates_path}/{index}"), scan)?;
            }
        }
        "LineString" => {
            if !positions(coordinates, &coordinates_path)?.is_empty() {
                line(coordinates, &coordinates_path, scan, 2)?;
            }
        }
        "MultiLineString" => {
            for (index, item) in positions(coordinates, &coordinates_path)?
                .iter()
                .enumerate()
            {
                line(item, &format!("{coordinates_path}/{index}"), scan, 2)?;
            }
        }
        "Polygon" => polygon(coordinates, &coordinates_path, scan)?,
        "MultiPolygon" => {
            for (index, item) in positions(coordinates, &coordinates_path)?
                .iter()
                .enumerate()
            {
                polygon(item, &format!("{coordinates_path}/{index}"), scan)?;
            }
        }
        other => {
            return Err(invalid(
                &format!("{path}/type"),
                format!("unknown geometry type '{other}'"),
            ));
        }
    }
    Ok(())
}

/// Checks one `Feature` and returns it with `properties` defaulted to an object.
fn feature(value: &Value, path: &str, scan: &mut Scan) -> Result<Value, GeoJsonError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(path, "a feature must be an object"))?;
    if object.get("type").and_then(Value::as_str) != Some("Feature") {
        return Err(invalid(
            &format!("{path}/type"),
            "expected type \"Feature\"",
        ));
    }
    let geometry_value = object.get("geometry").unwrap_or(&Value::Null);
    if !geometry_value.is_null() {
        geometry(geometry_value, &format!("{path}/geometry"), scan, 0)?;
    }
    let properties = match object.get("properties") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(properties @ Value::Object(_)) => properties.clone(),
        Some(_) => {
            return Err(invalid(
                &format!("{path}/properties"),
                "properties must be an object or null",
            ));
        }
    };
    let mut kept = Map::new();
    kept.insert("type".into(), Value::String("Feature".into()));
    if let Some(id) = object.get("id") {
        kept.insert("id".into(), id.clone());
    }
    kept.insert("geometry".into(), geometry_value.clone());
    kept.insert("properties".into(), properties);
    Ok(Value::Object(kept))
}

fn crs_warning(object: &Map<String, Value>, scan: &mut Scan) {
    let Some(crs) = object.get("crs") else {
        return;
    };
    let name = crs
        .pointer("/properties/name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let wgs84 = ["CRS84", "EPSG::4326", "EPSG:4326"]
        .iter()
        .any(|known| name.contains(known));
    if wgs84 {
        scan.warn(
            "/crs",
            "the legacy crs member was removed in RFC 7946; coordinates are WGS84 longitude/latitude",
        );
    } else {
        scan.warn(
            "/crs",
            format!(
                "the legacy crs member declares '{name}', which is ignored: RFC 7946 requires WGS84 \
                 longitude/latitude, so the data may be drawn in the wrong place"
            ),
        );
    }
}

/// Reads GeoJSON bytes: a `FeatureCollection`, a `Feature` or a bare geometry.
///
/// # Errors
///
/// Returns the first syntax or validation problem with its position.
pub fn parse_geojson(bytes: &[u8], limits: Limits) -> Result<GeoDocument, GeoJsonError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let root: Value = serde_json::from_slice(bytes).map_err(|error| GeoJsonError::Syntax {
        message: error.to_string(),
        line: error.line(),
        column: error.column(),
    })?;
    let object = root
        .as_object()
        .ok_or_else(|| invalid("", "a GeoJSON document must be a JSON object"))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("/type", "a GeoJSON object needs a type"))?;
    let mut scan = Scan::default();
    crs_warning(object, &mut scan);
    let mut features = Vec::new();
    let total_features;
    let mut truncated = false;
    match kind {
        "FeatureCollection" => {
            let items = object
                .get("features")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    invalid("/features", "a FeatureCollection needs a features array")
                })?;
            total_features = items.len();
            for (index, item) in items.iter().enumerate() {
                if features.len() >= limits.max_features || scan.positions >= limits.max_positions {
                    truncated = true;
                    break;
                }
                features.push(feature(item, &format!("/features/{index}"), &mut scan)?);
            }
        }
        "Feature" => {
            total_features = 1;
            features.push(feature(&root, "", &mut scan)?);
        }
        "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon"
        | "GeometryCollection" => {
            total_features = 1;
            geometry(&root, "", &mut scan, 0)?;
            let mut bare = Map::new();
            bare.insert("type".into(), Value::String("Feature".into()));
            let mut geometry_only = object.clone();
            geometry_only.remove("crs");
            geometry_only.remove("bbox");
            bare.insert("geometry".into(), Value::Object(geometry_only));
            bare.insert("properties".into(), Value::Object(Map::new()));
            features.push(Value::Object(bare));
        }
        other => {
            return Err(invalid("/type", format!("unknown GeoJSON type '{other}'")));
        }
    }
    if scan.out_of_range > 0 {
        let first = scan.first_out_of_range.clone().unwrap_or_default();
        let count = scan.out_of_range;
        scan.warn(
            &first,
            format!(
                "{count} position(s) lie outside longitude -180..180 / latitude -90..90 \
                 (the first is shown); coordinates must be longitude, latitude"
            ),
        );
    }
    Ok(GeoDocument {
        format: GeoFormat::GeoJson,
        features,
        total_features,
        truncated,
        bounds: scan.bounds,
        warnings: scan.warnings,
        skipped: Vec::new(),
        tracks: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(text: &str) -> Result<GeoDocument, GeoJsonError> {
        parse_geojson(text.as_bytes(), Limits::default())
    }

    fn issue(result: Result<GeoDocument, GeoJsonError>) -> GeoIssue {
        match result {
            Err(GeoJsonError::Invalid(issue)) => issue,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn reads_every_geometry_type_in_a_collection() {
        let document = parse(
            r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[4.9,52.3]},"properties":{"name":"A"}},
            {"type":"Feature","geometry":{"type":"MultiPoint","coordinates":[[1,2],[3,4]]},"properties":null},
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0],[1,1,10]]}},
            {"type":"Feature","geometry":{"type":"MultiLineString","coordinates":[[[0,0],[1,1]],[[2,2],[3,3]]]},"properties":{}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[4,0],[4,4],[0,4],[0,0]],[[1,1],[2,1],[2,2],[1,1]]]},"properties":{}},
            {"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]},"properties":{}},
            {"type":"Feature","geometry":null,"properties":{"note":"no geometry"}}
            ]}"#,
        )
        .expect("valid");
        assert_eq!(document.features.len(), 7);
        assert_eq!(document.total_features, 7);
        assert!(!document.truncated);
        assert_eq!(document.bounds, Some([0.0, 0.0, 4.9, 52.3]));
        assert!(document.warnings.is_empty());
        // Missing or null properties are normalised to an object.
        assert_eq!(document.features[2]["properties"], json!({}));
        assert_eq!(document.features[1]["properties"], json!({}));
    }

    #[test]
    fn wraps_bare_geometries_and_single_features() {
        let geometry = parse(r#"{"type":"Point","coordinates":[1,2]}"#).expect("valid");
        assert_eq!(geometry.features.len(), 1);
        assert_eq!(geometry.features[0]["geometry"]["type"], "Point");
        let single = parse(r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{"a":1}}"#)
            .expect("valid");
        assert_eq!(single.features[0]["properties"]["a"], 1);
    }

    #[test]
    fn empty_collections_and_empty_geometries_are_fine() {
        let empty = parse(r#"{"type":"FeatureCollection","features":[]}"#).expect("valid");
        assert!(empty.features.is_empty());
        assert_eq!(empty.bounds, None);
        let point = parse(
            r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[]},"properties":{}}"#,
        )
        .expect("valid");
        assert_eq!(point.bounds, None);
    }

    #[test]
    fn nested_geometry_collections_are_read_and_bounded() {
        let nested = parse(
            r#"{"type":"Feature","geometry":{"type":"GeometryCollection","geometries":[
            {"type":"Point","coordinates":[1,1]},
            {"type":"GeometryCollection","geometries":[{"type":"LineString","coordinates":[[2,2],[3,3]]}]}]},"properties":{}}"#,
        )
        .expect("valid");
        assert_eq!(nested.bounds, Some([1.0, 1.0, 3.0, 3.0]));

        let mut deep = r#"{"type":"Point","coordinates":[0,0]}"#.to_owned();
        for _ in 0..12 {
            deep = format!(r#"{{"type":"GeometryCollection","geometries":[{deep}]}}"#);
        }
        assert!(matches!(parse(&deep), Err(GeoJsonError::Invalid(_))));
    }

    #[test]
    fn syntax_errors_report_line_and_column() {
        match parse("{\n  \"type\": \"FeatureCollection\",\n  \"features\": [,]\n}") {
            Err(GeoJsonError::Syntax { line, column, .. }) => {
                assert_eq!(line, 3);
                assert!(column > 0);
            }
            other => panic!("expected a syntax error, got {other:?}"),
        }
    }

    #[test]
    fn validation_errors_point_at_the_offending_element() {
        let bad_position = issue(parse(
            r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0],[1,"x"]]},"properties":{}}]}"#,
        ));
        assert_eq!(bad_position.path, "/features/0/geometry/coordinates/1/1");

        let short_line = issue(parse(
            r#"{"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0]]},"properties":{}}"#,
        ));
        assert_eq!(short_line.path, "/geometry/coordinates");

        let short_ring = issue(parse(
            r#"{"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[1,1],[0,0]]]},"properties":{}}"#,
        ));
        assert_eq!(short_ring.path, "/geometry/coordinates/0");

        assert_eq!(issue(parse(r#"{"type":"Thing"}"#)).path, "/type");
        assert_eq!(
            issue(parse(r#"{"type":"FeatureCollection"}"#)).path,
            "/features"
        );
        assert_eq!(issue(parse("[1,2]")).path, "");
        assert_eq!(
            issue(parse(
                r#"{"type":"Feature","geometry":{"type":"Blob","coordinates":[]},"properties":{}}"#
            ))
            .path,
            "/geometry/type"
        );
        assert_eq!(
            issue(parse(
                r#"{"type":"Feature","geometry":null,"properties":[1]}"#
            ))
            .path,
            "/properties"
        );
    }

    #[test]
    fn out_of_range_and_unclosed_data_warn_instead_of_failing() {
        let document = parse(
            r#"{"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[10,0],[10,95],[0,0.5]]]},"properties":{}}"#,
        )
        .expect("lenient");
        let messages: Vec<_> = document
            .warnings
            .iter()
            .map(|w| w.message.as_str())
            .collect();
        assert!(messages.iter().any(|m| m.contains("not closed")));
        assert!(messages.iter().any(|m| m.contains("outside longitude")));
    }

    #[test]
    fn antimeridian_crossing_geometry_is_accepted() {
        let document = parse(
            r#"{"type":"Feature","geometry":{"type":"LineString","coordinates":[[179.5,10],[-179.5,11]]},"properties":{}}"#,
        )
        .expect("valid");
        assert!(document.warnings.is_empty());
        assert_eq!(document.bounds, Some([-179.5, 10.0, 179.5, 11.0]));
    }

    #[test]
    fn legacy_crs_members_are_flagged() {
        let named = parse(
            r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::3857"}},"features":[]}"#,
        )
        .expect("valid");
        assert!(named.warnings[0].message.contains("EPSG::3857"));
        let wgs84 = parse(
            r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:OGC:1.3:CRS84"}},"features":[]}"#,
        )
        .expect("valid");
        assert!(wgs84.warnings[0].message.contains("WGS84"));
    }

    #[test]
    fn large_collections_are_truncated_at_the_limits() {
        let features: Vec<String> = (0..10)
            .map(|index| {
                format!(r#"{{"type":"Feature","geometry":{{"type":"Point","coordinates":[{index},0]}},"properties":{{}}}}"#)
            })
            .collect();
        let text = format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        );
        let limits = Limits {
            max_features: 4,
            ..Limits::default()
        };
        let document = parse_geojson(text.as_bytes(), limits).expect("valid");
        assert_eq!(document.features.len(), 4);
        assert_eq!(document.total_features, 10);
        assert!(document.truncated);
    }

    #[test]
    fn a_byte_order_mark_is_ignored() {
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(br#"{"type":"Point","coordinates":[1,2]}"#);
        assert!(parse_geojson(&bytes, Limits::default()).is_ok());
    }
}
