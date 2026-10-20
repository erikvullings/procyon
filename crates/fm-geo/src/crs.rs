//! Coordinate reference systems: any EPSG code (or PROJ string) to WGS84 longitude/latitude.
//!
//! Definitions come from the `crs-definitions` EPSG table and are applied with `proj4rs`, a pure
//! Rust PROJ.4. Datum shifts use the Helmert parameters of the definition (for example
//! `+towgs84` for the Dutch RD New), which is accurate to about a metre: plenty for display, not
//! for surveying. Grid-based shifts (`+nadgrids`) are not available.
//!
//! Input coordinates are always read as (easting or longitude, northing or latitude), whatever
//! axis order the EPSG registry prescribes for a geographic system: files written by tools
//! almost never follow the registry's lat/lon order.

use proj4rs::proj::Proj;
use serde_json::Value;

/// Why a coordinate reference system could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CrsError {
    /// The text names no EPSG code and is not a PROJ string.
    #[error("'{0}' is not an EPSG code or a PROJ definition")]
    Unrecognised(String),
    /// The EPSG code is not in the definition table.
    #[error("EPSG:{0} is not a known coordinate reference system")]
    UnknownCode(u16),
    /// The definition cannot be inverted to WGS84 (for example a vertical or engineering system).
    #[error("{0} cannot be converted to longitude/latitude")]
    Unsupported(String),
}

/// A coordinate reference system that can convert positions to WGS84.
pub struct Crs {
    proj: Proj,
    wgs84: Proj,
    geographic: bool,
    is_wgs84: bool,
    label: String,
    name: String,
}

impl std::fmt::Debug for Crs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Crs")
            .field("label", &self.label)
            .field("name", &self.name)
            .finish()
    }
}

const WGS84_PROJ: &str = "+proj=longlat +datum=WGS84 +no_defs";

/// Extracts the EPSG code from the notations files use: `EPSG:28992`, `28992`,
/// `urn:ogc:def:crs:EPSG::28992`, `urn:ogc:def:crs:EPSG:6.9:28992`,
/// `http://www.opengis.net/def/crs/EPSG/0/28992`.
fn epsg_code(text: &str) -> Option<u16> {
    let trimmed = text.trim();
    if let Ok(code) = trimmed.parse::<u16>() {
        return Some(code);
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.contains("epsg") {
        return None;
    }
    let tail = trimmed.rsplit([':', '/']).next()?;
    tail.trim().parse().ok()
}

fn wkt_name(wkt: &str) -> Option<String> {
    let start = wkt.find('"')? + 1;
    let end = start + wkt[start..].find('"')?;
    Some(wkt[start..end].to_owned())
}

impl Crs {
    /// WGS84 longitude/latitude.
    ///
    /// # Panics
    ///
    /// Never: the definition is a constant.
    #[must_use]
    pub fn wgs84() -> Self {
        Self::from_parts(WGS84_PROJ, "EPSG:4326".into(), "WGS 84".into(), true)
            .expect("the WGS84 definition is valid")
    }

    fn from_parts(
        definition: &str,
        label: String,
        name: String,
        is_wgs84: bool,
    ) -> Result<Self, CrsError> {
        let proj =
            Proj::from_proj_string(definition).map_err(|_| CrsError::Unsupported(label.clone()))?;
        let wgs84 =
            Proj::from_proj_string(WGS84_PROJ).map_err(|_| CrsError::Unsupported(label.clone()))?;
        if !proj.has_inverse() {
            return Err(CrsError::Unsupported(label));
        }
        let geographic = proj.is_latlong();
        Ok(Self {
            proj,
            wgs84,
            geographic,
            is_wgs84,
            label,
            name,
        })
    }

    /// Reads an EPSG code in any common notation, `CRS84`, or a `+proj=...` string.
    ///
    /// # Errors
    ///
    /// Returns why the text is not a usable coordinate reference system.
    pub fn parse(text: &str) -> Result<Self, CrsError> {
        let trimmed = text.trim();
        if trimmed.eq_ignore_ascii_case("crs84")
            || trimmed.to_ascii_lowercase().ends_with("crs84")
            || trimmed.eq_ignore_ascii_case("wgs84")
        {
            return Ok(Self::wgs84());
        }
        if trimmed.starts_with('+') {
            return Self::from_parts(trimmed, trimmed.to_owned(), trimmed.to_owned(), false);
        }
        let code = epsg_code(trimmed).ok_or_else(|| CrsError::Unrecognised(trimmed.to_owned()))?;
        if code == 4326 {
            return Ok(Self::wgs84());
        }
        let definition = crs_definitions::from_code(code).ok_or(CrsError::UnknownCode(code))?;
        let name = wkt_name(definition.wkt).unwrap_or_else(|| format!("EPSG:{code}"));
        Self::from_parts(definition.proj4, format!("EPSG:{code}"), name, false)
    }

    /// `EPSG:28992`, or the PROJ string the system was created from.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Human-readable name, for example `Amersfoort / RD New`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether positions are already WGS84 longitude/latitude.
    #[must_use]
    pub fn is_wgs84(&self) -> bool {
        self.is_wgs84
    }

    /// Whether the system uses angles (degrees) rather than metres or feet.
    #[must_use]
    pub fn is_geographic(&self) -> bool {
        self.geographic
    }

    /// Converts (x, y) in this system to WGS84 (longitude, latitude) in degrees.
    #[must_use]
    pub fn to_wgs84(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        if self.is_wgs84 {
            return Some((x, y));
        }
        let mut point = if self.geographic {
            (x.to_radians(), y.to_radians())
        } else {
            (x, y)
        };
        proj4rs::transform::transform(&self.proj, &self.wgs84, &mut point).ok()?;
        let (longitude, latitude) = (point.0.to_degrees(), point.1.to_degrees());
        (longitude.is_finite() && latitude.is_finite()).then_some((longitude, latitude))
    }
}

fn reproject_position(position: &mut Value, crs: &Crs) -> bool {
    let Some(items) = position.as_array_mut() else {
        return false;
    };
    let (Some(x), Some(y)) = (
        items.first().and_then(Value::as_f64),
        items.get(1).and_then(Value::as_f64),
    ) else {
        return false;
    };
    let Some((longitude, latitude)) = crs.to_wgs84(x, y) else {
        return false;
    };
    if let (Some(first), Some(second)) = (
        serde_json::Number::from_f64(longitude),
        serde_json::Number::from_f64(latitude),
    ) {
        items[0] = Value::Number(first);
        items[1] = Value::Number(second);
        return true;
    }
    false
}

fn reproject_coordinates(value: &mut Value, crs: &Crs, failed: &mut usize) {
    let is_position = value
        .as_array()
        .is_some_and(|items| items.len() >= 2 && items.iter().all(Value::is_number));
    if is_position {
        if !reproject_position(value, crs) {
            *failed += 1;
        }
    } else if let Some(items) = value.as_array_mut() {
        for item in items {
            reproject_coordinates(item, crs, failed);
        }
    }
}

/// Converts every position of a GeoJSON value (collection, feature or geometry) to WGS84 in
/// place and returns how many positions could not be converted (they are left as they were).
pub fn reproject_geojson(value: &mut Value, crs: &Crs) -> usize {
    let mut failed = 0;
    reproject_node(value, crs, &mut failed);
    failed
}

fn reproject_node(value: &mut Value, crs: &Crs, failed: &mut usize) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if let Some(coordinates) = object.get_mut("coordinates") {
        reproject_coordinates(coordinates, crs, failed);
    }
    for key in ["geometry", "geometries", "features"] {
        match object.get_mut(key) {
            Some(Value::Array(items)) => {
                for item in items {
                    reproject_node(item, crs, failed);
                }
            }
            Some(nested @ Value::Object(_)) => reproject_node(nested, crs, failed),
            _ => {}
        }
    }
}

/// A guess at the coordinate system of a set of (x, y) samples when the file does not say.
#[must_use]
pub fn guess_crs(samples: &[(f64, f64)]) -> Option<&'static str> {
    if samples.is_empty() {
        return None;
    }
    let within = |range: (f64, f64), value: f64| value >= range.0 && value <= range.1;
    if samples
        .iter()
        .all(|&(x, y)| within((-180.0, 180.0), x) && within((-90.0, 90.0), y))
    {
        return Some("EPSG:4326");
    }
    // The Dutch RD New grid is common enough to recognise by its characteristic extent.
    if samples
        .iter()
        .all(|&(x, y)| within((-7_000.0, 300_000.0), x) && within((289_000.0, 629_000.0), y))
    {
        return Some("EPSG:28992");
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn understands_the_common_epsg_notations() {
        for text in [
            "EPSG:28992",
            "epsg:28992",
            "28992",
            "urn:ogc:def:crs:EPSG::28992",
            "urn:ogc:def:crs:EPSG:6.9:28992",
            "http://www.opengis.net/def/crs/EPSG/0/28992",
        ] {
            let crs = Crs::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"));
            assert_eq!(crs.label(), "EPSG:28992", "{text}");
            assert!(crs.name().contains("RD New"), "{}", crs.name());
        }
        assert!(
            Crs::parse("urn:ogc:def:crs:OGC:1.3:CRS84")
                .unwrap()
                .is_wgs84()
        );
        assert!(Crs::parse("EPSG:4326").unwrap().is_wgs84());
        assert!(matches!(
            Crs::parse("nonsense"),
            Err(CrsError::Unrecognised(_))
        ));
        assert!(matches!(
            Crs::parse("EPSG:1"),
            Err(CrsError::UnknownCode(1))
        ));
    }

    #[test]
    fn converts_the_amersfoort_tower_from_rd_new() {
        // The Onze Lieve Vrouwetoren in Amersfoort is the origin of RD New (155000, 463000);
        // it stands at about 52.155172 N, 5.387206 E.
        let rd = Crs::parse("EPSG:28992").unwrap();
        let (longitude, latitude) = rd.to_wgs84(155_000.0, 463_000.0).unwrap();
        assert!((longitude - 5.3872).abs() < 0.001, "{longitude}");
        assert!((latitude - 52.1552).abs() < 0.001, "{latitude}");
    }

    #[test]
    fn converts_projected_systems_around_the_world() {
        // Web Mercator, UTM 18N (New York) and a Japanese plane system.
        let mercator = Crs::parse("EPSG:3857").unwrap();
        let (longitude, latitude) = mercator.to_wgs84(1_113_194.9, 0.0).unwrap();
        assert!((longitude - 10.0).abs() < 1e-4 && latitude.abs() < 1e-4);
        let utm = Crs::parse("EPSG:32618").unwrap();
        // The Empire State Building: UTM zone 18N 585628 E, 4511322 N.
        let (longitude, latitude) = utm.to_wgs84(585_628.0, 4_511_322.0).unwrap();
        assert!((longitude + 73.9857).abs() < 0.001 && (latitude - 40.7484).abs() < 0.001);
        assert!(
            Crs::parse("EPSG:2451")
                .unwrap()
                .to_wgs84(-8_000.0, -35_000.0)
                .is_some()
        );
    }

    #[test]
    fn geographic_systems_pass_degrees_through_a_datum_shift() {
        let etrs = Crs::parse("EPSG:4258").unwrap();
        let (longitude, latitude) = etrs.to_wgs84(5.0, 52.0).unwrap();
        assert!((longitude - 5.0).abs() < 1e-3 && (latitude - 52.0).abs() < 1e-3);
    }

    #[test]
    fn reprojects_every_position_of_a_feature_collection() {
        let mut value = json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": [155000.0, 463000.0]}},
                {"type": "Feature", "properties": {}, "geometry": {"type": "LineString", "coordinates": [[155000.0, 463000.0], [156000.0, 464000.0]]}},
            ]
        });
        let failed = reproject_geojson(&mut value, &Crs::parse("EPSG:28992").unwrap());
        assert_eq!(failed, 0);
        let first = value
            .pointer("/features/0/geometry/coordinates/0")
            .and_then(Value::as_f64)
            .unwrap();
        assert!((first - 5.3872).abs() < 0.001);
        let last = value
            .pointer("/features/1/geometry/coordinates/1/1")
            .and_then(Value::as_f64)
            .unwrap();
        assert!((last - 52.164).abs() < 0.01);
    }

    #[test]
    fn guesses_wgs84_and_rd_but_not_unrelated_metres() {
        assert_eq!(guess_crs(&[(4.9, 52.3), (5.1, 52.0)]), Some("EPSG:4326"));
        assert_eq!(
            guess_crs(&[(121_000.0, 487_000.0), (155_000.0, 463_000.0)]),
            Some("EPSG:28992")
        );
        assert_eq!(guess_crs(&[(583_960.0, 4_507_523.0)]), None);
        assert_eq!(guess_crs(&[]), None);
    }
}
