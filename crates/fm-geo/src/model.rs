use serde_json::{Value, json};

/// The formats this crate understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoFormat {
    /// GeoJSON (RFC 7946).
    GeoJson,
    /// GPX 1.0 and 1.1 tracks, routes and waypoints.
    Gpx,
    /// KML (Google Earth).
    Kml,
    /// KMZ: zipped KML with its icons and overlay images.
    Kmz,
}

impl GeoFormat {
    /// Stable identifier used on the wire.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::GeoJson => "geojson",
            Self::Gpx => "gpx",
            Self::Kml => "kml",
            Self::Kmz => "kmz",
        }
    }
}

/// A problem or limitation found while reading, with a JSON-pointer-like path where available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoIssue {
    /// Where in the document the issue was found, for example `/features/3/geometry`.
    pub path: String,
    /// What is wrong or was left out.
    pub message: String,
}

/// West, south, east, north in degrees.
pub type Bounds = [f64; 4];

/// Size limits that keep reading bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most features kept for display.
    pub max_features: usize,
    /// Most coordinate positions kept for display.
    pub max_positions: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_features: 50_000,
            max_positions: 2_000_000,
        }
    }
}

/// A parsed geographic document.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoDocument {
    /// Source format.
    pub format: GeoFormat,
    /// GeoJSON `Feature` values kept for display.
    pub features: Vec<Value>,
    /// Features in the source, which can exceed `features.len()` when limits applied.
    pub total_features: usize,
    /// Whether the limits cut the document short.
    pub truncated: bool,
    /// Extent of the kept features.
    pub bounds: Option<Bounds>,
    /// Non-fatal issues: legacy members, out-of-range coordinates, unclosed rings.
    pub warnings: Vec<GeoIssue>,
    /// Source features or elements that were left out because they have no 2D representation.
    pub skipped: Vec<String>,
    /// Statistics per recorded track (GPX).
    pub tracks: Vec<TrackSummary>,
    /// Images (icons, overlays) shipped inside a KMZ, as `data:` URIs by their path in the archive.
    pub resources: std::collections::BTreeMap<String, String>,
    /// Ground overlays whose image is available in `resources`.
    pub overlays: Vec<GroundOverlay>,
}

/// An image draped over a rectangle of the map (KML `GroundOverlay`).
#[derive(Debug, Clone, PartialEq)]
pub struct GroundOverlay {
    /// Overlay name.
    pub name: Option<String>,
    /// Path of the image in `GeoDocument::resources`.
    pub image: String,
    /// Northern edge in degrees.
    pub north: f64,
    /// Southern edge in degrees.
    pub south: f64,
    /// Eastern edge in degrees.
    pub east: f64,
    /// Western edge in degrees.
    pub west: f64,
    /// Counter-clockwise rotation in degrees around the centre.
    pub rotation: f64,
}

/// One sample of a track's elevation and speed profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfilePoint {
    /// Distance from the start of the track in metres.
    pub distance_meters: f64,
    /// Position of the sample.
    pub longitude: f64,
    /// Position of the sample.
    pub latitude: f64,
    /// Elevation in metres, when recorded.
    pub elevation: Option<f64>,
    /// Speed over the last leg, when timestamps allow it.
    pub speed_meters_per_second: Option<f64>,
    /// Heart rate in beats per minute, when recorded.
    pub heart_rate: Option<f64>,
}

/// Statistics of one recorded track. Values the file cannot support are `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSummary {
    /// Track name.
    pub name: Option<String>,
    /// Index of the track's feature in [`GeoDocument::features`].
    pub feature_index: usize,
    /// Recorded fixes.
    pub points: usize,
    /// Length in metres over the whole recording (segments are not connected).
    pub distance_meters: f64,
    /// Last minus first timestamp.
    pub elapsed_seconds: Option<f64>,
    /// Time spent faster than walking pace.
    pub moving_seconds: Option<f64>,
    /// Climbed metres, ignoring changes below the noise floor.
    pub ascent_meters: Option<f64>,
    /// Descended metres, ignoring changes below the noise floor.
    pub descent_meters: Option<f64>,
    /// Lowest recorded elevation.
    pub min_elevation: Option<f64>,
    /// Highest recorded elevation.
    pub max_elevation: Option<f64>,
    /// Distance covered while moving divided by the moving time.
    pub average_speed: Option<f64>,
    /// Fastest speed sustained for at least ten seconds.
    pub max_speed: Option<f64>,
    /// First timestamp, UTC.
    pub start_time: Option<String>,
    /// Last timestamp, UTC.
    pub end_time: Option<String>,
    /// At most about 500 samples for charts.
    pub profile: Vec<ProfilePoint>,
}

impl GeoDocument {
    /// The features as a GeoJSON `FeatureCollection`.
    #[must_use]
    pub fn feature_collection(&self) -> Value {
        json!({ "type": "FeatureCollection", "features": self.features })
    }
}
