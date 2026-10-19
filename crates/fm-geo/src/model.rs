use serde_json::{Value, json};

/// The formats this crate understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoFormat {
    /// GeoJSON (RFC 7946).
    GeoJson,
}

impl GeoFormat {
    /// Stable identifier used on the wire.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::GeoJson => "geojson",
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
}

impl GeoDocument {
    /// The features as a GeoJSON `FeatureCollection`.
    #[must_use]
    pub fn feature_collection(&self) -> Value {
        json!({ "type": "FeatureCollection", "features": self.features })
    }
}
