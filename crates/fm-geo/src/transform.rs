//! Reading a file and writing it again, optionally in another format and/or smaller (tasks
//! 0281, 0282). This is the one function the operation engine and the previews call.

use crate::GeoFormat;
use crate::convert::{Target, convert};
use crate::kmz::repack_kmz;
use crate::minify::{MinifyOptions, MinifyStats, compact_json, compact_xml, minify_document};
use crate::model::GeoDocument;
use crate::read::{conversion_limits, read_document};

/// What to do with a file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformSpec {
    /// Output format; `None` keeps the format of the source.
    pub target: Option<Target>,
    /// Size reductions to apply; `None` converts only.
    pub minify: Option<MinifyOptions>,
}

/// The result of a transformation.
#[derive(Debug, Clone, PartialEq)]
pub struct Transformed {
    /// The new file content.
    pub bytes: Vec<u8>,
    /// What the output cannot represent or what was removed.
    pub notes: Vec<String>,
    /// Effect of a model-level minification.
    pub stats: Option<MinifyStats>,
    /// The features that were written, when the model path was used (for previews).
    pub document: Option<GeoDocument>,
}

fn same_format(extension: &str, minified: bool) -> Option<Target> {
    match extension.to_ascii_lowercase().as_str() {
        "geojson" | "json" => Some(Target::GeoJson { pretty: !minified }),
        "gpx" => Some(Target::Gpx),
        "kml" => Some(Target::Kml),
        "kmz" => Some(Target::Kmz),
        _ => None,
    }
}

/// Transforms `bytes` of a file with `extension`.
///
/// # Errors
///
/// Returns a user-presentable message when the file cannot be read or written in the target format.
pub fn transform(
    extension: &str,
    bytes: &[u8],
    spec: &TransformSpec,
) -> Result<Transformed, String> {
    if let Some(options) = &spec.minify
        && options.is_whitespace_only()
        && spec.target.is_none()
    {
        // Lossless text path: nothing but whitespace (and XML comments) changes.
        let format = crate::detect_format(extension, bytes)
            .ok_or_else(|| "this is not a recognised geographic file".to_owned())?;
        let compact = match format {
            GeoFormat::GeoJson => compact_json(bytes),
            GeoFormat::Gpx | GeoFormat::Kml => compact_xml(bytes)?,
            GeoFormat::Kmz => repack_kmz(bytes).map_err(|error| error.to_string())?,
            GeoFormat::Csv => return Err("CSV files cannot be minified".into()),
        };
        return Ok(Transformed {
            bytes: compact,
            notes: Vec::new(),
            stats: None,
            document: None,
        });
    }
    let document =
        read_document(extension, bytes, conversion_limits()).map_err(|failure| failure.message)?;
    let (document, stats, mut notes) = match &spec.minify {
        Some(options) => {
            let (minified, stats, notes) = minify_document(&document, options);
            (minified, Some(stats), notes)
        }
        None => (document, None, Vec::new()),
    };
    let target = match spec.target {
        Some(target) => target,
        None => same_format(extension, spec.minify.is_some())
            .ok_or_else(|| "this file has no output format of its own".to_owned())?,
    };
    let target = match (target, spec.minify.is_some()) {
        (Target::GeoJson { .. }, true) => Target::GeoJson { pretty: false },
        (other, _) => other,
    };
    let converted = convert(&document, target).map_err(|error| error.to_string())?;
    notes.extend(converted.notes);
    Ok(Transformed {
        bytes: converted.bytes,
        notes,
        stats,
        document: Some(document),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEOJSON: &[u8] = b"{\n  \"type\": \"FeatureCollection\",\n  \"features\": [\n    {\"type\": \"Feature\", \"geometry\": {\"type\": \"Point\", \"coordinates\": [4.123456789, 52.987654321]}, \"properties\": {\"name\": \"A\"}}\n  ]\n}\n";

    fn spec(options: MinifyOptions) -> TransformSpec {
        TransformSpec {
            target: None,
            minify: Some(options),
        }
    }

    #[test]
    fn whitespace_only_takes_the_lossless_text_path() {
        let result = transform("geojson", GEOJSON, &spec(MinifyOptions::default())).expect("ok");
        assert!(result.document.is_none() && result.stats.is_none());
        let text = String::from_utf8(result.bytes).expect("utf-8");
        assert!(
            text.contains("4.123456789"),
            "numbers are untouched: {text}"
        );
        assert!(!text.contains('\n') && !text.contains(": "));
    }

    #[test]
    fn precision_takes_the_model_path_and_reports_statistics() {
        let options = MinifyOptions {
            precision: Some(3),
            ..MinifyOptions::default()
        };
        let result = transform("geojson", GEOJSON, &spec(options)).expect("ok");
        let text = String::from_utf8(result.bytes).expect("utf-8");
        assert!(text.contains("[4.123,52.988]"), "{text}");
        assert_eq!(result.stats.map(|s| s.positions_after), Some(1));
        assert!(result.document.is_some());
        assert_eq!(text.trim().lines().count(), 1, "compact output");
    }

    #[test]
    fn xml_formats_keep_their_format_and_kmz_is_repacked() {
        let kml = b"<kml>\n  <Document>\n    <Placemark><name>A</name><Point><coordinates>1,2</coordinates></Point></Placemark>\n  </Document>\n</kml>\n";
        let compact = transform("kml", kml, &spec(MinifyOptions::default())).expect("kml");
        assert!(compact.bytes.len() < kml.len());
        assert!(!String::from_utf8_lossy(&compact.bytes).contains('\n'));

        let kmz = convert(
            &read_document("kml", kml, conversion_limits()).expect("kml"),
            Target::Kmz,
        )
        .expect("kmz")
        .bytes;
        let repacked = transform("kmz", &kmz, &spec(MinifyOptions::default())).expect("kmz");
        let document = crate::parse_kmz(&repacked.bytes, conversion_limits()).expect("reads back");
        assert_eq!(document.features.len(), 1);
    }

    #[test]
    fn an_explicit_target_converts_and_minifies_in_one_step() {
        let options = MinifyOptions {
            drop_elevation: true,
            ..MinifyOptions::default()
        };
        let result = transform(
            "geojson",
            GEOJSON,
            &TransformSpec {
                target: Target::parse("gpx", false),
                minify: Some(options),
            },
        )
        .expect("ok");
        assert!(
            String::from_utf8(result.bytes)
                .expect("utf-8")
                .contains("<wpt ")
        );
    }

    #[test]
    fn unreadable_or_unknown_files_report_why() {
        assert!(
            transform("geojson", b"{", &spec(MinifyOptions::default())).is_ok(),
            "text path does not validate JSON"
        );
        let options = MinifyOptions {
            precision: Some(3),
            ..MinifyOptions::default()
        };
        assert!(transform("geojson", b"{", &spec(options)).is_err());
        assert!(transform("txt", b"x", &spec(options)).is_err());
    }
}
