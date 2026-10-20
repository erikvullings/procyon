//! Delimited text with coordinates, read as points (or WKT geometries) in any coordinate system.
//!
//! Columns are found by name (`lat`/`lon`, `latitude`/`longitude`, `breedtegraad`/`lengtegraad`,
//! `x`/`y`, `easting`/`northing`, a `wkt`/`geometry` column ...) and can be chosen explicitly. The
//! coordinate system is taken from the options, or guessed from the values when they are
//! unmistakable (longitude/latitude, the Dutch RD grid); everything is converted to WGS84.

use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};

use crate::crs::{Crs, CrsError, guess_crs};
use crate::model::{Bounds, GeoDocument, GeoFormat, GeoIssue, Limits};

/// What the caller chose; everything left out is detected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsvOptions {
    /// Column holding the easting or longitude.
    pub x_column: Option<String>,
    /// Column holding the northing or latitude.
    pub y_column: Option<String>,
    /// Column holding WKT geometries; used instead of `x_column`/`y_column`.
    pub wkt_column: Option<String>,
    /// Coordinate system of the coordinates: `EPSG:28992`, `28992`, a `+proj=...` string ...
    pub crs: Option<String>,
}

/// What was used to read the file, and what the file offers, for a "columns and system" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvInfo {
    /// Column names of the first row.
    pub headers: Vec<String>,
    /// The field separator that was detected.
    pub delimiter: char,
    /// Easting/longitude column in use.
    pub x_column: Option<String>,
    /// Northing/latitude column in use.
    pub y_column: Option<String>,
    /// WKT column in use.
    pub wkt_column: Option<String>,
    /// Coordinate system in use, for example `EPSG:28992`.
    pub crs: String,
    /// Its name, for example `Amersfoort / RD New`.
    pub crs_name: String,
    /// Whether `crs` was guessed from the values rather than given.
    pub crs_guessed: bool,
    /// Whether the columns were found by name rather than given.
    pub columns_detected: bool,
    /// Data rows in the file.
    pub rows: usize,
    /// Rows without usable coordinates.
    pub skipped_rows: usize,
}

/// Why a file could not be read as geographic points.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CsvError {
    /// The text is not delimited text.
    #[error("{0}")]
    Syntax(String),
    /// No column pair or WKT column was found, or a chosen column does not exist.
    #[error("{message}")]
    NoCoordinates {
        /// What was missing.
        message: String,
        /// The columns the file has, so a caller can offer them.
        headers: Vec<String>,
    },
    /// The coordinate system is unknown or unusable.
    #[error(transparent)]
    Crs(#[from] CrsError),
}

/// Header names, compared case-insensitively with everything but letters and digits removed.
const LONGITUDE_NAMES: [&str; 12] = [
    "lon",
    "lng",
    "long",
    "longitude",
    "longitud",
    "lengtegraad",
    "longitudine",
    "lon_deg",
    "longitudedeg",
    "londeg",
    "wgs84lon",
    "wgs84longitude",
];
const LATITUDE_NAMES: [&str; 12] = [
    "lat",
    "latitude",
    "latitud",
    "breedtegraad",
    "latitudine",
    "lat_deg",
    "latitudedeg",
    "latdeg",
    "wgs84lat",
    "wgs84latitude",
    "ylat",
    "latit",
];
const EASTING_NAMES: [&str; 9] = [
    "x",
    "xcoord",
    "xcoordinate",
    "easting",
    "east",
    "oost",
    "xrd",
    "rdx",
    "xcoor",
];
const NORTHING_NAMES: [&str; 9] = [
    "y",
    "ycoord",
    "ycoordinate",
    "northing",
    "north",
    "noord",
    "yrd",
    "rdy",
    "ycoor",
];
const WKT_NAMES: [&str; 8] = [
    "wkt",
    "geom",
    "geometry",
    "thegeom",
    "wktgeom",
    "geometrie",
    "shape",
    "wktgeometry",
];

fn normalise(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn find(headers: &[String], names: &[&str]) -> Option<usize> {
    let wanted: Vec<String> = names.iter().map(|name| normalise(name)).collect();
    headers
        .iter()
        .position(|header| wanted.iter().any(|name| *name == normalise(header)))
}

fn sniff_delimiter(text: &str) -> char {
    let first_line = text.lines().next().unwrap_or_default();
    [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|candidate| first_line.matches(*candidate).count())
        .filter(|candidate| first_line.contains(*candidate))
        .unwrap_or(',')
}

fn parse_number(text: &str, decimal_comma: bool) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalised = if decimal_comma && !trimmed.contains('.') {
        trimmed.replace(',', ".")
    } else {
        trimmed.to_owned()
    };
    normalised
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// A cell as a JSON value: clean numbers become numbers, everything else stays text. Codes with a
/// leading zero (`007`) and long digit strings stay text so they are not corrupted.
fn cell_value(text: &str, decimal_comma: bool) -> Value {
    let trimmed = text.trim();
    let looks_like_code = trimmed.len() > 1
        && trimmed.starts_with('0')
        && !trimmed.starts_with("0.")
        && !trimmed.starts_with("0,");
    let number = if !looks_like_code && trimmed.len() <= 15 {
        parse_number(trimmed, decimal_comma)
    } else {
        None
    };
    if let Some(number) = number {
        let integral = number.fract() == 0.0 && number.abs() < 9.0e15;
        let json = if integral {
            Some(Number::from(number as i64))
        } else {
            Number::from_f64(number)
        };
        if let Some(json) = json {
            return Value::Number(json);
        }
    }
    Value::String(text.to_owned())
}

/* ------------------------------------------------------------------------------------------ */
/*  WKT                                                                                        */
/* ------------------------------------------------------------------------------------------ */

struct Wkt<'a> {
    text: &'a [u8],
    position: usize,
}

impl Wkt<'_> {
    fn skip_space(&mut self) {
        while self
            .text
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.skip_space();
        if self.text.get(self.position) == Some(&byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self) -> String {
        self.skip_space();
        let start = self.position;
        while self
            .text
            .get(self.position)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            self.position += 1;
        }
        String::from_utf8_lossy(&self.text[start..self.position]).to_ascii_uppercase()
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_space();
        let start = self.position;
        while self
            .text
            .get(self.position)
            .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E'))
        {
            self.position += 1;
        }
        std::str::from_utf8(&self.text[start..self.position])
            .ok()?
            .parse()
            .ok()
    }

    fn position_value(&mut self) -> Option<Vec<f64>> {
        let mut numbers = Vec::new();
        while let Some(number) = self.number() {
            numbers.push(number);
        }
        (numbers.len() >= 2).then_some(numbers)
    }

    /// `(x y, x y, ...)`, also tolerating the parenthesised points of `MULTIPOINT ((x y), (x y))`.
    fn positions(&mut self) -> Option<Vec<Vec<f64>>> {
        if !self.eat(b'(') {
            return None;
        }
        let mut result = Vec::new();
        loop {
            self.skip_space();
            if self.text.get(self.position) == Some(&b'(') {
                result.extend(self.positions()?);
            } else {
                result.push(self.position_value()?);
            }
            if self.eat(b',') {
                continue;
            }
            return self.eat(b')').then_some(result);
        }
    }

    fn nested<T>(&mut self, inner: impl Fn(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        if !self.eat(b'(') {
            return None;
        }
        let mut result = Vec::new();
        loop {
            result.push(inner(self)?);
            if self.eat(b',') {
                continue;
            }
            return self.eat(b')').then_some(result);
        }
    }

    fn geometry(&mut self) -> Option<Value> {
        let kind = self.word();
        // Dimension markers: Z, M, ZM (and EMPTY, which has no geometry to show).
        let mut marker = String::new();
        self.skip_space();
        let before = self.position;
        let next = self.word();
        if matches!(next.as_str(), "Z" | "M" | "ZM") {
            marker = next;
        } else if next == "EMPTY" {
            return None;
        } else {
            self.position = before;
        }
        let drop_measure = marker == "M";
        let position = |numbers: Vec<f64>| -> Value {
            let keep = if drop_measure {
                2
            } else {
                numbers.len().min(3)
            };
            Value::Array(
                numbers
                    .into_iter()
                    .take(keep)
                    .filter_map(Number::from_f64)
                    .map(Value::Number)
                    .collect(),
            )
        };
        let positions = |list: Vec<Vec<f64>>| -> Value {
            Value::Array(list.into_iter().map(position).collect())
        };
        let object = |kind: &str, coordinates: Value| -> Value {
            let mut map = Map::new();
            map.insert("type".into(), Value::String(kind.into()));
            map.insert("coordinates".into(), coordinates);
            Value::Object(map)
        };
        match kind.as_str() {
            "POINT" => {
                if !self.eat(b'(') {
                    return None;
                }
                let point = self.position_value()?;
                self.eat(b')').then(|| object("Point", position(point)))
            }
            "LINESTRING" => Some(object("LineString", positions(self.positions()?))),
            "POLYGON" => {
                let rings = self.nested(Self::positions)?;
                Some(object(
                    "Polygon",
                    Value::Array(rings.into_iter().map(positions).collect()),
                ))
            }
            "MULTIPOINT" => Some(object("MultiPoint", positions(self.positions()?))),
            "MULTILINESTRING" => {
                let lines = self.nested(Self::positions)?;
                Some(object(
                    "MultiLineString",
                    Value::Array(lines.into_iter().map(positions).collect()),
                ))
            }
            "MULTIPOLYGON" => {
                let polygons = self.nested(|parser| parser.nested(Self::positions))?;
                Some(object(
                    "MultiPolygon",
                    Value::Array(
                        polygons
                            .into_iter()
                            .map(|rings| Value::Array(rings.into_iter().map(positions).collect()))
                            .collect(),
                    ),
                ))
            }
            "GEOMETRYCOLLECTION" => {
                let members = self.nested(Self::geometry)?;
                let mut map = Map::new();
                map.insert("type".into(), Value::String("GeometryCollection".into()));
                map.insert("geometries".into(), Value::Array(members));
                Some(Value::Object(map))
            }
            _ => None,
        }
    }
}

/// Reads one WKT geometry (an `SRID=...;` prefix is ignored) as a GeoJSON geometry object.
#[must_use]
pub fn parse_wkt(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    let body = match trimmed.split_once(';') {
        Some((prefix, rest)) if prefix.to_ascii_uppercase().starts_with("SRID=") => rest,
        _ => trimmed,
    };
    let mut parser = Wkt {
        text: body.as_bytes(),
        position: 0,
    };
    parser.geometry()
}

/* ------------------------------------------------------------------------------------------ */
/*  Reading                                                                                    */
/* ------------------------------------------------------------------------------------------ */

fn extend_bounds(bounds: &mut Option<Bounds>, longitude: f64, latitude: f64) {
    let current = bounds.get_or_insert([longitude, latitude, longitude, latitude]);
    current[0] = current[0].min(longitude);
    current[1] = current[1].min(latitude);
    current[2] = current[2].max(longitude);
    current[3] = current[3].max(latitude);
}

fn visit_positions(value: &Value, visit: &mut impl FnMut(f64, f64)) {
    if let Some(items) = value.as_array() {
        if items.len() >= 2 && items.iter().all(Value::is_number) {
            if let (Some(x), Some(y)) = (items[0].as_f64(), items[1].as_f64()) {
                visit(x, y);
            }
        } else {
            for item in items {
                visit_positions(item, visit);
            }
        }
    }
}

fn visit_geometry(geometry: &Value, visit: &mut impl FnMut(f64, f64)) {
    if let Some(coordinates) = geometry.get("coordinates") {
        visit_positions(coordinates, visit);
    }
    if let Some(members) = geometry.get("geometries").and_then(Value::as_array) {
        for member in members {
            visit_geometry(member, visit);
        }
    }
}

fn reproject_geometry(geometry: &mut Value, crs: &Crs) -> bool {
    let mut wrapper = Value::Object(Map::from_iter([(
        "geometry".to_owned(),
        std::mem::take(geometry),
    )]));
    let failed = crate::crs::reproject_geojson(&mut wrapper, crs);
    *geometry = wrapper
        .as_object_mut()
        .and_then(|map| map.remove("geometry"))
        .unwrap_or(Value::Null);
    failed == 0
}

/// Reads delimited text as geographic features.
///
/// # Errors
///
/// Returns why the text cannot be read as points or geometries: malformed text, no coordinate
/// columns, or an unusable coordinate system.
pub fn read_csv(
    bytes: &[u8],
    limits: Limits,
    options: &CsvOptions,
) -> Result<(GeoDocument, CsvInfo), CsvError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes);
    let delimiter = sniff_delimiter(&text);
    let decimal_comma = delimiter != ',';
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(u8::try_from(delimiter).unwrap_or(b','))
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|error| CsvError::Syntax(error.to_string()))?
        .iter()
        .map(|header| header.trim().to_owned())
        .collect();
    let no_coordinates = |message: String| CsvError::NoCoordinates {
        message,
        headers: headers.clone(),
    };
    let explicit = |name: &Option<String>| -> Result<Option<usize>, CsvError> {
        match name {
            None => Ok(None),
            Some(name) => headers
                .iter()
                .position(|header| header == name)
                .map(Some)
                .ok_or_else(|| no_coordinates(format!("the file has no column named '{name}'"))),
        }
    };

    // Which columns hold the position.
    let chosen_x = explicit(&options.x_column)?;
    let chosen_y = explicit(&options.y_column)?;
    let chosen_wkt = explicit(&options.wkt_column)?;
    let columns_given = chosen_x.is_some() || chosen_y.is_some() || chosen_wkt.is_some();
    let (mut x_index, mut y_index, mut wkt_index) = (chosen_x, chosen_y, chosen_wkt);
    let mut strong_names = columns_given;
    if !columns_given {
        let named = find(&headers, &LONGITUDE_NAMES).zip(find(&headers, &LATITUDE_NAMES));
        if let Some((x, y)) = named {
            (x_index, y_index) = (Some(x), Some(y));
            strong_names = true;
        } else if let Some((x, y)) =
            find(&headers, &EASTING_NAMES).zip(find(&headers, &NORTHING_NAMES))
        {
            (x_index, y_index) = (Some(x), Some(y));
        } else {
            wkt_index = find(&headers, &WKT_NAMES);
            strong_names = wkt_index.is_some();
        }
    }
    if wkt_index.is_none() && (x_index.is_none() || y_index.is_none()) {
        return Err(no_coordinates(
            "no latitude/longitude, x/y or WKT geometry column was found".into(),
        ));
    }

    let records: Vec<csv::StringRecord> = reader
        .records()
        .collect::<Result<_, _>>()
        .map_err(|error| CsvError::Syntax(error.to_string()))?;

    // Which coordinate system.
    let samples: Vec<(f64, f64)> = if let (Some(x), Some(y)) = (x_index, y_index) {
        records
            .iter()
            .filter_map(|record| {
                Some((
                    parse_number(record.get(x)?, decimal_comma)?,
                    parse_number(record.get(y)?, decimal_comma)?,
                ))
            })
            .take(500)
            .collect()
    } else {
        let mut collected = Vec::new();
        if let Some(wkt) = wkt_index {
            for record in records.iter().take(200) {
                if let Some(geometry) = record.get(wkt).and_then(parse_wkt) {
                    visit_geometry(&geometry, &mut |x, y| collected.push((x, y)));
                }
            }
        }
        collected
    };
    let (system, guessed) = match &options.crs {
        Some(text) => (Crs::parse(text)?, false),
        None => match guess_crs(&samples) {
            Some(label) => (Crs::parse(label)?, true),
            None if strong_names => (Crs::wgs84(), true),
            None => {
                return Err(no_coordinates(
                    "the columns look like x/y values in an unknown coordinate system: choose one"
                        .into(),
                ));
            }
        },
    };

    // The features.
    let mut features = Vec::new();
    let mut bounds: Option<Bounds> = None;
    let (mut skipped_rows, mut unconverted) = (0_usize, 0_usize);
    let mut skipped_notes = Vec::new();
    let coordinate_columns = [x_index, y_index, wkt_index];
    let keep_coordinate_columns = !system.is_wgs84();
    for (row, record) in records.iter().enumerate() {
        if features.len() >= limits.max_features {
            break;
        }
        let geometry = if let Some(wkt) = wkt_index {
            record.get(wkt).and_then(parse_wkt)
        } else if let (Some(x), Some(y)) = (x_index, y_index) {
            record
                .get(x)
                .zip(record.get(y))
                .and_then(|(x, y)| {
                    parse_number(x, decimal_comma).zip(parse_number(y, decimal_comma))
                })
                .and_then(|(x, y)| {
                    let (x, y) = (Number::from_f64(x)?, Number::from_f64(y)?);
                    let mut map = Map::new();
                    map.insert("type".into(), Value::String("Point".into()));
                    map.insert(
                        "coordinates".into(),
                        Value::Array(vec![Value::Number(x), Value::Number(y)]),
                    );
                    Some(Value::Object(map))
                })
        } else {
            None
        };
        let Some(mut geometry) = geometry else {
            skipped_rows += 1;
            if skipped_notes.len() < 3 {
                skipped_notes.push(row + 2);
            }
            continue;
        };
        if !reproject_geometry(&mut geometry, &system) {
            unconverted += 1;
        }
        visit_geometry(&geometry, &mut |longitude, latitude| {
            extend_bounds(&mut bounds, longitude, latitude);
        });
        let mut properties = Map::new();
        for (index, header) in headers.iter().enumerate() {
            let is_coordinate = coordinate_columns.contains(&Some(index));
            if (is_coordinate && !keep_coordinate_columns) || header.is_empty() {
                continue;
            }
            if let Some(cell) = record.get(index).filter(|cell| !cell.trim().is_empty()) {
                properties.insert(header.clone(), cell_value(cell, decimal_comma));
            }
        }
        let mut feature = Map::new();
        feature.insert("type".into(), Value::String("Feature".into()));
        feature.insert("geometry".into(), geometry);
        feature.insert("properties".into(), Value::Object(properties));
        features.push(Value::Object(feature));
    }

    let mut warnings = Vec::new();
    if !system.is_wgs84() {
        warnings.push(GeoIssue {
            path: String::new(),
            message: format!(
                "coordinates were converted from {} ({}) to WGS84{}",
                system.label(),
                system.name(),
                if guessed {
                    " (guessed from the values)"
                } else {
                    ""
                }
            ),
        });
    }
    if skipped_rows > 0 {
        let rows = skipped_notes
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        warnings.push(GeoIssue {
            path: String::new(),
            message: format!(
                "{skipped_rows} row(s) without usable coordinates were left out (first: {rows})"
            ),
        });
    }
    if unconverted > 0 {
        warnings.push(GeoIssue {
            path: String::new(),
            message: format!("{unconverted} row(s) could not be converted to WGS84"),
        });
    }
    if system.is_wgs84() {
        let outside = samples
            .iter()
            .filter(|(x, y)| x.abs() > 180.0 || y.abs() > 90.0)
            .count();
        if outside > 0 {
            warnings.push(GeoIssue {
                path: String::new(),
                message: format!(
                    "{outside} position(s) lie outside longitude -180..180 / latitude -90..90: \
                     choose the coordinate system of this file, or swap the columns"
                ),
            });
        }
    }

    let info = CsvInfo {
        headers: headers.clone(),
        delimiter,
        x_column: x_index.and_then(|index| headers.get(index).cloned()),
        y_column: y_index.and_then(|index| headers.get(index).cloned()),
        wkt_column: wkt_index.and_then(|index| headers.get(index).cloned()),
        crs: system.label().to_owned(),
        crs_name: system.name().to_owned(),
        crs_guessed: guessed,
        columns_detected: !columns_given,
        rows: records.len(),
        skipped_rows,
    };
    let total_features = records.len() - skipped_rows;
    let truncated = features.len() < total_features;
    Ok((
        GeoDocument {
            format: GeoFormat::Csv,
            features,
            total_features,
            truncated,
            bounds,
            warnings,
            skipped: Vec::new(),
            tracks: Vec::new(),
            resources: BTreeMap::new(),
            overlays: Vec::new(),
        },
        info,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, options: &CsvOptions) -> Result<(GeoDocument, CsvInfo), CsvError> {
        read_csv(text.as_bytes(), Limits::default(), options)
    }

    fn coordinates(document: &GeoDocument, index: usize) -> (f64, f64) {
        let position = document.features[index]
            .pointer("/geometry/coordinates")
            .and_then(Value::as_array)
            .expect("point");
        (
            position[0].as_f64().expect("x"),
            position[1].as_f64().expect("y"),
        )
    }

    #[test]
    fn finds_latitude_and_longitude_by_name_and_keeps_other_columns() {
        let (document, info) = read(
            "name,lat,lon,rating\nDam,52.3731,4.8936,4.2\nPark,52.358,4.868,\n",
            &CsvOptions::default(),
        )
        .expect("readable");
        assert_eq!(document.features.len(), 2);
        assert_eq!(coordinates(&document, 0), (4.8936, 52.3731));
        assert_eq!(info.x_column.as_deref(), Some("lon"));
        assert_eq!(info.y_column.as_deref(), Some("lat"));
        assert_eq!(info.crs, "EPSG:4326");
        let properties = &document.features[0]["properties"];
        assert_eq!(properties["name"], "Dam");
        assert_eq!(properties["rating"], 4.2);
        assert!(properties.get("lat").is_none());
        assert!(document.features[1]["properties"].get("rating").is_none());
        assert_eq!(document.bounds, Some([4.868, 52.358, 4.8936, 52.3731]));
    }

    #[test]
    fn reads_semicolon_files_with_decimal_commas() {
        let (document, info) = read(
            "naam;breedtegraad;lengtegraad\nDam;52,3731;4,8936\n",
            &CsvOptions::default(),
        )
        .expect("readable");
        assert_eq!(info.delimiter, ';');
        assert_eq!(coordinates(&document, 0), (4.8936, 52.3731));
    }

    #[test]
    fn recognises_rd_new_from_x_y_columns_and_converts_it() {
        let (document, info) = read(
            "id,X,Y\n1,155000,463000\n2,121000,487000\n",
            &CsvOptions::default(),
        )
        .expect("readable");
        assert_eq!(info.crs, "EPSG:28992");
        assert!(info.crs_guessed && info.columns_detected);
        let (longitude, latitude) = coordinates(&document, 0);
        assert!((longitude - 5.3872).abs() < 0.001 && (latitude - 52.1552).abs() < 0.001);
        // The original grid coordinates stay available as properties.
        assert_eq!(document.features[0]["properties"]["X"], 155_000);
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("guessed"))
        );
    }

    #[test]
    fn any_epsg_code_can_be_chosen() {
        let options = CsvOptions {
            x_column: Some("easting".into()),
            y_column: Some("northing".into()),
            crs: Some("EPSG:32618".into()),
            ..CsvOptions::default()
        };
        let (document, info) =
            read("easting,northing\n585628,4511322\n", &options).expect("readable");
        let (longitude, latitude) = coordinates(&document, 0);
        assert!((longitude + 73.9857).abs() < 0.001 && (latitude - 40.7484).abs() < 0.001);
        assert!(!info.crs_guessed && !info.columns_detected);
        assert_eq!(info.crs, "EPSG:32618");
    }

    #[test]
    fn unrelated_x_y_values_are_not_taken_for_coordinates() {
        let error = read("x,y\n583960,4507523\n", &CsvOptions::default()).expect_err("ambiguous");
        assert!(matches!(error, CsvError::NoCoordinates { .. }));
        // Plain data without any coordinate-like column is not geographic either.
        assert!(matches!(
            read("name,age\nAnn,3\n", &CsvOptions::default()),
            Err(CsvError::NoCoordinates { .. })
        ));
    }

    #[test]
    fn reads_wkt_geometries_and_reprojects_them() {
        let (document, info) = read(
            "id;wkt\n1;\"POINT (155000 463000)\"\n2;\"LINESTRING (155000 463000, 156000 464000)\"\n3;\"POLYGON ((1 1, 2 1, 2 2, 1 1))\"\n",
            &CsvOptions {
                crs: Some("28992".into()),
                ..CsvOptions::default()
            },
        )
        .expect("readable");
        assert_eq!(info.wkt_column.as_deref(), Some("wkt"));
        assert_eq!(document.features.len(), 3);
        assert_eq!(document.features[1]["geometry"]["type"], "LineString");
        assert_eq!(document.features[2]["geometry"]["type"], "Polygon");
    }

    #[test]
    fn counts_rows_without_coordinates_and_warns() {
        let (document, info) =
            read("lat,lon\n52,4\n,\nabc,4\n", &CsvOptions::default()).expect("readable");
        assert_eq!(document.features.len(), 1);
        assert_eq!(info.skipped_rows, 2);
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("2 row(s)"))
        );
    }

    #[test]
    fn warns_about_out_of_range_degrees_and_unknown_columns_are_errors() {
        let (document, _) = read("lat,lon\n452,4\n", &CsvOptions::default()).expect("readable");
        assert!(
            document
                .warnings
                .iter()
                .any(|w| w.message.contains("outside longitude"))
        );
        let options = CsvOptions {
            x_column: Some("nope".into()),
            ..CsvOptions::default()
        };
        assert!(matches!(
            read("lat,lon\n1,2\n", &options),
            Err(CsvError::NoCoordinates { .. })
        ));
        assert!(matches!(
            read(
                "lat,lon\n1,2\n",
                &CsvOptions {
                    crs: Some("EPSG:1".into()),
                    ..CsvOptions::default()
                }
            ),
            Err(CsvError::Crs(_))
        ));
    }

    #[test]
    fn keeps_codes_with_leading_zeros_as_text() {
        let (document, _) =
            read("code,lat,lon\n007,1,2\n12,1,2\n", &CsvOptions::default()).expect("ok");
        assert_eq!(document.features[0]["properties"]["code"], "007");
        assert_eq!(document.features[1]["properties"]["code"], 12);
    }

    #[test]
    fn parses_wkt_variants() {
        assert_eq!(
            parse_wkt("POINT Z (1 2 3)").unwrap()["coordinates"],
            serde_json::json!([1.0, 2.0, 3.0])
        );
        assert_eq!(parse_wkt("SRID=4326;POINT(1 2)").unwrap()["type"], "Point");
        assert_eq!(
            parse_wkt("MULTIPOINT ((1 2), (3 4))").unwrap()["coordinates"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(parse_wkt("MULTIPOLYGON (((0 0, 1 0, 1 1, 0 0)), ((5 5, 6 5, 6 6, 5 5)))").unwrap()["coordinates"].as_array().unwrap().len(), 2);
        assert_eq!(parse_wkt("GEOMETRYCOLLECTION (POINT (1 2), LINESTRING (0 0, 1 1))").unwrap()["geometries"].as_array().unwrap().len(), 2);
        assert!(parse_wkt("POINT EMPTY").is_none());
        assert!(parse_wkt("CIRCLE (1 2)").is_none());
        assert!(parse_wkt("POINT (1").is_none());
    }
}
