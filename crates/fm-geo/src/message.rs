//! Localisable messages: a stable code, the English text as fallback, and the values the
//! localised sentence interpolates.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;

/// A user-facing message that clients translate by `code`; `text` is the English fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoMessage {
    /// Stable machine-readable code from [`codes`].
    pub code: &'static str,
    /// English text for clients that do not know the code.
    pub text: String,
    /// Values the localised sentence interpolates (counts, names).
    pub params: BTreeMap<String, String>,
}

impl GeoMessage {
    /// A message without parameters.
    #[must_use]
    pub fn new(code: &'static str, text: impl Into<String>) -> Self {
        Self {
            code,
            text: text.into(),
            params: BTreeMap::new(),
        }
    }

    /// Adds one interpolation value.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_owned(), value.to_string());
        self
    }
}

impl Deref for GeoMessage {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for GeoMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// Codes of [`GeoMessage`]. Every value must have a translation in each frontend locale; a
/// frontend test reads this file to enforce that, so declare each code as a plain `pub const`.
pub mod codes {
    /// KMZ images that are not carried over (`count`).
    pub const KMZ_IMAGES_DROPPED: &str = "kmzImagesDropped";
    /// Ground overlays that are not carried over (`count`).
    pub const OVERLAYS_DROPPED: &str = "overlaysDropped";
    /// Source elements that were never read (`count`).
    pub const ELEMENTS_NEVER_READ: &str = "elementsNeverRead";
    /// Icon URLs are kept only as a property.
    pub const ICON_URLS_KEPT: &str = "iconUrlsKept";
    /// Polygons cannot be stored in GPX.
    pub const POLYGONS_NOT_IN_GPX: &str = "polygonsNotInGpx";
    /// Styling of features is not stored in GPX (`count`).
    pub const STYLES_NOT_IN_GPX: &str = "stylesNotInGpx";
    /// Custom properties are not stored in GPX (`count`).
    pub const PROPERTIES_NOT_IN_GPX: &str = "propertiesNotInGpx";
    /// HTML descriptions are written as text.
    pub const HTML_AS_TEXT: &str = "htmlAsText";
    /// Folder structure is not stored in GPX.
    pub const FOLDERS_NOT_IN_GPX: &str = "foldersNotInGpx";
    /// Structured values are not stored in KML.
    pub const STRUCTURED_NOT_IN_KML: &str = "structuredNotInKml";
    /// Per-point timestamps are not stored in plain KML (`count`).
    pub const TIMESTAMPS_NOT_IN_KML: &str = "timestampsNotInKml";
    /// Features without geometry were left out (`count`).
    pub const FEATURES_WITHOUT_GEOMETRY: &str = "featuresWithoutGeometry";
    /// Lines and areas were left out of CSV (`count`).
    pub const NON_POINTS_LEFT_OUT: &str = "nonPointsLeftOut";
    /// Structured values are not stored in CSV.
    pub const STRUCTURED_NOT_IN_CSV: &str = "structuredNotInCsv";
    /// Text cells were prefixed against formula injection (`count`).
    pub const FORMULA_CELLS_PREFIXED: &str = "formulaCellsPrefixed";
    /// Per-point timestamps are not stored in CSV.
    pub const TIMESTAMPS_NOT_IN_CSV: &str = "timestampsNotInCsv";
    /// Custom property values were removed (`count`).
    pub const PROPERTIES_REMOVED: &str = "propertiesRemoved";
    /// Rings were kept to avoid self-crossing (`count`).
    pub const RINGS_KEPT: &str = "ringsKept";
    /// The result is not smaller than the original (`after`, `before` bytes).
    pub const NOT_SMALLER: &str = "notSmaller";
    /// The file is not a recognised geographic file.
    pub const UNRECOGNISED_FILE: &str = "unrecognisedFile";
    /// The document is empty.
    pub const EMPTY_DOCUMENT: &str = "emptyDocument";
    /// The XML root is not `<kml>` (`name`).
    pub const ROOT_NOT_KML: &str = "rootNotKml";
    /// The XML root is not `<gpx>` (`name`).
    pub const ROOT_NOT_GPX: &str = "rootNotGpx";
    /// DOCTYPE entities are not allowed.
    pub const ENTITIES_NOT_ALLOWED: &str = "entitiesNotAllowed";
    /// JSON syntax error (`line`, `column`, `detail`).
    pub const JSON_SYNTAX: &str = "jsonSyntax";
    /// XML syntax error (`line`, `column`, `detail`).
    pub const XML_SYNTAX: &str = "xmlSyntax";
    /// A position is not an array of numbers.
    pub const POSITION_NOT_ARRAY: &str = "positionNotArray";
    /// A position has fewer than two numbers.
    pub const POSITION_TOO_SHORT: &str = "positionTooShort";
    /// A coordinate is not a finite number.
    pub const COORDINATES_NOT_FINITE: &str = "coordinatesNotFinite";
    /// A line or ring has too few positions (`minimum`, `found`).
    pub const TOO_FEW_POSITIONS: &str = "tooFewPositions";
    /// Unknown geometry type (`type`).
    pub const UNKNOWN_GEOMETRY_TYPE: &str = "unknownGeometryType";
    /// A feature must have type Feature.
    pub const EXPECTED_FEATURE: &str = "expectedFeature";
    /// Feature properties must be an object or null.
    pub const PROPERTIES_NOT_OBJECT: &str = "propertiesNotObject";
    /// Coordinates are not an array.
    pub const COORDINATES_NOT_ARRAY: &str = "coordinatesNotArray";
    /// A geometry is not an object.
    pub const GEOMETRY_NOT_OBJECT: &str = "geometryNotObject";
    /// A geometry has no type.
    pub const GEOMETRY_NEEDS_TYPE: &str = "geometryNeedsType";
    /// Geometry collections are nested too deeply.
    pub const GEOMETRY_TOO_DEEP: &str = "geometryTooDeep";
    /// A geometry collection lacks `geometries`.
    pub const GEOMETRIES_MISSING: &str = "geometriesMissing";
    /// A geometry lacks `coordinates`.
    pub const COORDINATES_MISSING: &str = "coordinatesMissing";
    /// A feature is not an object.
    pub const FEATURE_NOT_OBJECT: &str = "featureNotObject";
    /// The document is not a JSON object.
    pub const NOT_JSON_OBJECT: &str = "notJsonObject";
    /// The GeoJSON object lacks `type`.
    pub const TYPE_MISSING: &str = "typeMissing";
    /// A FeatureCollection lacks `features`.
    pub const FEATURES_MISSING: &str = "featuresMissing";
    /// Unknown GeoJSON type (`type`).
    pub const UNKNOWN_TYPE: &str = "unknownType";
    /// A polygon ring is not closed.
    pub const RING_NOT_CLOSED: &str = "ringNotClosed";
    /// Positions lie outside the WGS84 range (`count`).
    pub const POSITIONS_OUT_OF_RANGE: &str = "positionsOutOfRange";
    /// The legacy `crs` member names an unusable system (`name`).
    pub const CRS_UNUSABLE: &str = "crsUnusable";
    /// The legacy `crs` member was removed in RFC 7946.
    pub const CRS_LEGACY: &str = "crsLegacy";
    /// Coordinates were converted to WGS84 (`label`, `name`, `failed`).
    pub const CRS_CONVERTED: &str = "crsConverted";
    /// Coordinates were converted to a guessed WGS84 source system (`label`, `name`).
    pub const CRS_GUESSED: &str = "crsGuessed";
    /// CSV rows without coordinates were left out (`count`, `first`).
    pub const ROWS_SKIPPED: &str = "rowsSkipped";
    /// CSV rows could not be converted to WGS84 (`count`).
    pub const ROWS_UNCONVERTED: &str = "rowsUnconverted";
    /// Positions lie outside the WGS84 range in a CSV (`count`).
    pub const CSV_OUT_OF_RANGE: &str = "csvOutOfRange";
    /// Placemarks use external icons (`count`).
    pub const EXTERNAL_ICONS: &str = "externalIcons";
    /// Referenced images are missing (`count`).
    pub const IMAGES_MISSING: &str = "imagesMissing";
    /// Invalid coordinates were left out (`count`).
    pub const INVALID_POSITIONS: &str = "invalidPositions";
    /// GPX segments with fewer than two points were left out (`count`).
    pub const SHORT_SEGMENTS: &str = "shortSegments";
    /// A GPX track is thinned for display.
    pub const TRACK_THINNED: &str = "trackThinned";
    /// Some timestamps are missing or lack a time zone.
    pub const TIMESTAMPS_MISSING: &str = "timestampsMissing";
    /// GPX points with invalid coordinates were left out (`count`).
    pub const POINTS_DROPPED: &str = "pointsDropped";
    /// The CSV is not delimited text (`detail`).
    pub const CSV_SYNTAX: &str = "csvSyntax";
    /// A chosen CSV column does not exist (`name`).
    pub const CSV_NO_COLUMN: &str = "csvNoColumn";
    /// No coordinate column was found.
    pub const CSV_NO_COORDINATES: &str = "csvNoCoordinates";
    /// The CSV columns look like projected values in an unknown system.
    pub const CSV_UNKNOWN_SYSTEM: &str = "csvUnknownSystem";
    /// The text names no EPSG code and is no PROJ definition (`text`).
    pub const CRS_UNRECOGNISED: &str = "crsUnrecognised";
    /// The EPSG code is unknown (`code`).
    pub const CRS_UNKNOWN_CODE: &str = "crsUnknownCode";
    /// The system cannot be converted to longitude/latitude (`name`).
    pub const CRS_UNSUPPORTED: &str = "crsUnsupported";
    /// The KMZ is not a valid zip archive (`detail`).
    pub const KMZ_ARCHIVE: &str = "kmzArchive";
    /// The KMZ holds no KML file.
    pub const KMZ_NO_KML: &str = "kmzNoKml";
    /// The KMZ has too many entries (`max`).
    pub const KMZ_TOO_MANY_ENTRIES: &str = "kmzTooManyEntries";
    /// The KML inside the KMZ is too large (`max`).
    pub const KMZ_KML_TOO_LARGE: &str = "kmzKmlTooLarge";
    /// The KML inside the KMZ expands beyond the limit.
    pub const KMZ_KML_EXPANDS: &str = "kmzKmlExpands";
    /// The KMZ is too large to repack.
    pub const KMZ_TOO_LARGE_TO_REPACK: &str = "kmzTooLargeToRepack";
    /// GPX can hold only points and lines.
    pub const GPX_NOTHING_TO_WRITE: &str = "gpxNothingToWrite";
    /// The CSV has no points to write.
    pub const CSV_NOTHING_TO_WRITE: &str = "csvNothingToWrite";
    /// Building the output failed (`detail`).
    pub const WRITE_FAILED: &str = "writeFailed";
    /// CSV files cannot be minified.
    pub const CSV_CANNOT_MINIFY: &str = "csvCannotMinify";
    /// The file has no output format of its own.
    pub const NO_OUTPUT_FORMAT: &str = "noOutputFormat";
    /// The file is too large to convert (`size`, `max`).
    pub const FILE_TOO_LARGE: &str = "fileTooLarge";
    /// The file could not be read from its location (`detail`).
    pub const READ_FAILED: &str = "readFailed";
    /// Minifying XML failed (`detail`).
    pub const MINIFY_FAILED: &str = "minifyFailed";
}
