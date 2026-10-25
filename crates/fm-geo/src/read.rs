//! One entry point for reading any supported format.

use crate::csv_geo::{CsvOptions, read_csv};
use crate::geojson::{GeoJsonError, parse_geojson};
use crate::gpx::{GpxError, parse_gpx};
use crate::kml::{KmlError, NoResources, parse_kml};
use crate::kmz::{KmzError, parse_kmz};
use crate::message::{GeoMessage, codes};
use crate::model::{GeoDocument, GeoFormat, Limits};

/// Why a file could not be read, in a form independent of the format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFailure {
    /// The detected format, if any.
    pub format: Option<GeoFormat>,
    /// What went wrong.
    pub message: GeoMessage,
    /// One-based line of a syntax error.
    pub line: Option<usize>,
    /// One-based column of a syntax error.
    pub column: Option<usize>,
    /// Location of a validation error inside the document.
    pub path: Option<String>,
}

impl ReadFailure {
    fn plain(format: Option<GeoFormat>, message: GeoMessage) -> Self {
        Self {
            format,
            message,
            line: None,
            column: None,
            path: None,
        }
    }

    fn syntax(format: GeoFormat, message: GeoMessage, line: usize, column: usize) -> Self {
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
        match &error {
            KmlError::Syntax { line, column, .. } => {
                Self::syntax(GeoFormat::Kml, error.message(), *line, *column)
            }
            _ => Self::plain(Some(GeoFormat::Kml), error.message()),
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
            GeoMessage::new(
                codes::UNRECOGNISED_FILE,
                "this is not a recognised geographic file",
            ),
        ));
    };
    match format {
        GeoFormat::GeoJson => parse_geojson(bytes, limits).map_err(|error| match &error {
            GeoJsonError::Syntax { line, column, .. } => {
                ReadFailure::syntax(format, error.message(), *line, *column)
            }
            GeoJsonError::Invalid(issue) => ReadFailure {
                format: Some(format),
                message: error.message(),
                line: None,
                column: None,
                path: Some(issue.path.clone()),
            },
        }),
        GeoFormat::Gpx => parse_gpx(bytes, limits).map_err(|error| match &error {
            GpxError::Syntax { line, column, .. } => {
                ReadFailure::syntax(format, error.message(), *line, *column)
            }
            _ => ReadFailure::plain(Some(format), error.message()),
        }),
        GeoFormat::Kml => {
            parse_kml(bytes, limits, &NoResources, GeoFormat::Kml).map_err(Into::into)
        }
        GeoFormat::Csv => read_csv(bytes, limits, &CsvOptions::default())
            .map(|(document, _)| document)
            .map_err(|error| ReadFailure::plain(Some(format), error.message())),
        GeoFormat::Kmz => parse_kmz(bytes, limits).map_err(|error| match error {
            KmzError::Kml(inner) => ReadFailure::from(inner),
            other => ReadFailure::plain(Some(format), other.message()),
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
