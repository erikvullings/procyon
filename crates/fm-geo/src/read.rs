//! One entry point for reading any supported format.

use crate::geojson::{GeoJsonError, parse_geojson};
use crate::gpx::{GpxError, parse_gpx};
use crate::kml::{KmlError, NoResources, parse_kml};
use crate::kmz::{KmzError, parse_kmz};
use crate::model::{GeoDocument, GeoFormat, Limits};

/// Why a file could not be read, in a form independent of the format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFailure {
    /// The detected format, if any.
    pub format: Option<GeoFormat>,
    /// What went wrong.
    pub message: String,
    /// One-based line of a syntax error.
    pub line: Option<usize>,
    /// One-based column of a syntax error.
    pub column: Option<usize>,
    /// Location of a validation error inside the document.
    pub path: Option<String>,
}

impl ReadFailure {
    fn plain(format: Option<GeoFormat>, message: String) -> Self {
        Self {
            format,
            message,
            line: None,
            column: None,
            path: None,
        }
    }

    fn syntax(format: GeoFormat, message: String, line: usize, column: usize) -> Self {
        Self {
            format: Some(format),
            message,
            line: Some(line),
            column: Some(column),
            path: None,
        }
    }
}

impl From<KmlError> for ReadFailure {
    fn from(error: KmlError) -> Self {
        match error {
            KmlError::Syntax {
                message,
                line,
                column,
            } => Self::syntax(GeoFormat::Kml, message, line, column),
            other => Self::plain(Some(GeoFormat::Kml), other.to_string()),
        }
    }
}

/// Reads `bytes`, choosing the format from `extension` and the leading bytes.
///
/// # Errors
///
/// Returns why the file is not a readable geographic file.
pub fn read_document(
    extension: &str,
    bytes: &[u8],
    limits: Limits,
) -> Result<GeoDocument, ReadFailure> {
    let Some(format) = crate::detect_format(extension, bytes) else {
        return Err(ReadFailure::plain(
            None,
            "this is not a recognised geographic file".into(),
        ));
    };
    match format {
        GeoFormat::GeoJson => parse_geojson(bytes, limits).map_err(|error| match error {
            GeoJsonError::Syntax {
                message,
                line,
                column,
            } => ReadFailure::syntax(format, message, line, column),
            GeoJsonError::Invalid(issue) => ReadFailure {
                format: Some(format),
                message: issue.message,
                line: None,
                column: None,
                path: Some(issue.path),
            },
        }),
        GeoFormat::Gpx => parse_gpx(bytes, limits).map_err(|error| match error {
            GpxError::Syntax {
                message,
                line,
                column,
            } => ReadFailure::syntax(format, message, line, column),
            other => ReadFailure::plain(Some(format), other.to_string()),
        }),
        GeoFormat::Kml => {
            parse_kml(bytes, limits, &NoResources, GeoFormat::Kml).map_err(Into::into)
        }
        GeoFormat::Kmz => parse_kmz(bytes, limits).map_err(|error| match error {
            KmzError::Kml(inner) => ReadFailure::from(inner),
            other => ReadFailure::plain(Some(format), other.to_string()),
        }),
    }
}

/// Limits for conversion: everything is kept, including per-point data.
#[must_use]
pub fn conversion_limits() -> Limits {
    Limits {
        max_features: 1_000_000,
        max_positions: 20_000_000,
        lossless: true,
    }
}
