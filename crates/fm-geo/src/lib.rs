//! Geographic data formats behind one GeoJSON-like feature model (tasks 0277-0282).
//!
//! Every reader produces a [`GeoDocument`]: a list of GeoJSON `Feature` values (RFC 7946, WGS84
//! longitude/latitude) plus warnings about what the source format could not express faithfully.
//! Parsing is bounded and never panics on malformed input.

mod convert;
mod crs;
mod csv_geo;
mod geojson;
mod gpx;
mod kml;
mod kmz;
mod minify;
mod model;
mod read;
mod time;
mod track_stats;
mod transform;

pub use convert::{ConvertError, Converted, Target, convert};
pub use crs::{Crs, CrsError, guess_crs, reproject_geojson};
pub use csv_geo::{CsvError, CsvInfo, CsvOptions, parse_wkt, read_csv};
pub use geojson::{GeoJsonError, parse_geojson};
pub use gpx::{GpxError, parse_gpx};
pub use kml::{KmlError, NoResources, ResourceSource, parse_kml};
pub use kmz::{KmzError, parse_kmz, repack_kmz};
pub use minify::{MinifyOptions, MinifyStats, compact_json, compact_xml, minify_document};
pub use model::{
    Bounds, GeoDocument, GeoFormat, GeoIssue, GroundOverlay, Limits, ProfilePoint, TrackSummary,
};
pub use read::{ReadFailure, conversion_limits, read_document};
pub use transform::{TransformSpec, Transformed, transform};

/// Guesses the format of a file from its extension and leading bytes.
///
/// `.geojson` is trusted; a plain `.json` counts as GeoJSON only when its first kilobytes name a
/// GeoJSON object type, so ordinary JSON documents keep opening as data.
#[must_use]
pub fn detect_format(extension: &str, head: &[u8]) -> Option<GeoFormat> {
    match extension.to_ascii_lowercase().as_str() {
        "geojson" => Some(GeoFormat::GeoJson),
        "gpx" => Some(GeoFormat::Gpx),
        "kml" => Some(GeoFormat::Kml),
        "kmz" => Some(GeoFormat::Kmz),
        "csv" | "tsv" => Some(GeoFormat::Csv),
        "json" => sniff_geojson(head).then_some(GeoFormat::GeoJson),
        _ => None,
    }
}

fn sniff_geojson(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&head[..head.len().min(4096)]);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with('{') {
        return false;
    }
    const TYPES: [&str; 9] = [
        "FeatureCollection",
        "Feature",
        "Point",
        "MultiPoint",
        "LineString",
        "MultiLineString",
        "Polygon",
        "MultiPolygon",
        "GeometryCollection",
    ];
    TYPES.iter().any(|name| {
        let quoted = format!("\"{name}\"");
        text.find("\"type\"").is_some_and(|at| {
            text[at..]
                .split_once(':')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with(&quoted))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusts_the_geojson_extension_and_sniffs_plain_json() {
        assert_eq!(detect_format("GeoJSON", b""), Some(GeoFormat::GeoJson));
        assert_eq!(
            detect_format(
                "json",
                br#"{ "type" : "FeatureCollection", "features": [] }"#
            ),
            Some(GeoFormat::GeoJson)
        );
        assert_eq!(
            detect_format("json", br#"{"name":"x","type":"book"}"#),
            None
        );
        assert_eq!(detect_format("json", b"[1,2,3]"), None);
        assert_eq!(detect_format("txt", br#"{"type":"Point"}"#), None);
        assert_eq!(detect_format("GPX", b"<gpx/>"), Some(GeoFormat::Gpx));
    }
}
