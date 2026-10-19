//! KMZ reading (task 0280): a zip holding a KML file and the images it uses.
//!
//! The archive is read from memory and nothing is extracted to disk. Entry counts and sizes are
//! bounded, so a zip bomb cannot exhaust memory.

use std::cell::RefCell;
use std::io::{Cursor, Read};

use thiserror::Error;
use zip::ZipArchive;

use crate::kml::{KmlError, ResourceSource, parse_kml};
use crate::model::{GeoDocument, GeoFormat, Limits};

const MAX_ENTRIES: usize = 10_000;
const MAX_KML_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RESOURCE_BYTES: u64 = 1024 * 1024;

/// Why a KMZ file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KmzError {
    /// The file is not a readable zip archive.
    #[error("the KMZ file is not a valid zip archive: {0}")]
    Archive(String),
    /// The archive contains no `.kml` file.
    #[error("the KMZ archive contains no .kml file")]
    NoKml,
    /// The archive has too many entries or the KML is too large.
    #[error("{0}")]
    TooLarge(String),
    /// The KML inside is invalid.
    #[error(transparent)]
    Kml(#[from] KmlError),
}

struct Archive<'a> {
    zip: RefCell<ZipArchive<Cursor<&'a [u8]>>>,
}

impl ResourceSource for Archive<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let mut zip = self.zip.borrow_mut();
        let mut entry = zip.by_name(path).ok()?;
        if entry.is_dir() || entry.size() > MAX_RESOURCE_BYTES {
            return None;
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(MAX_RESOURCE_BYTES + 1)
            .read_to_end(&mut data)
            .ok()?;
        (data.len() as u64 <= MAX_RESOURCE_BYTES).then_some(data)
    }
}

/// Picks the main KML: `doc.kml`, else the shallowest `.kml`.
fn main_kml(names: &[String]) -> Option<&String> {
    names
        .iter()
        .find(|name| name.eq_ignore_ascii_case("doc.kml"))
        .or_else(|| {
            names
                .iter()
                .filter(|name| name.to_ascii_lowercase().ends_with(".kml"))
                .min_by_key(|name| (name.matches('/').count(), name.len()))
        })
}

/// Reads KMZ bytes.
///
/// # Errors
///
/// Fails when the archive is unreadable, holds no KML, exceeds the bounds, or the KML is invalid.
pub fn parse_kmz(bytes: &[u8], limits: Limits) -> Result<GeoDocument, KmzError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| KmzError::Archive(error.to_string()))?;
    if zip.len() > MAX_ENTRIES {
        return Err(KmzError::TooLarge(format!(
            "the KMZ archive has more than {MAX_ENTRIES} entries"
        )));
    }
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    let kml_name = main_kml(&names).ok_or(KmzError::NoKml)?.clone();
    let mut kml = Vec::new();
    {
        let entry = zip
            .by_name(&kml_name)
            .map_err(|error| KmzError::Archive(error.to_string()))?;
        if entry.size() > MAX_KML_BYTES {
            return Err(KmzError::TooLarge(format!(
                "the KML inside the KMZ is larger than {MAX_KML_BYTES} bytes"
            )));
        }
        let mut limited = entry.take(MAX_KML_BYTES + 1);
        limited
            .read_to_end(&mut kml)
            .map_err(|error| KmzError::Archive(error.to_string()))?;
        if kml.len() as u64 > MAX_KML_BYTES {
            return Err(KmzError::TooLarge(
                "the KML inside the KMZ expands beyond the size limit".into(),
            ));
        }
    }
    let archive = Archive {
        zip: RefCell::new(zip),
    };
    Ok(parse_kml(&kml, limits, &archive, GeoFormat::Kmz)?)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    fn kmz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            for (name, data) in entries {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .expect("start file");
                writer.write_all(data).expect("write");
            }
            writer.finish().expect("finish");
        }
        buffer.into_inner()
    }

    const KML: &[u8] = br#"<kml><Document><Style id="s"><IconStyle><Icon><href>files/pin.png</href></Icon></IconStyle></Style>
        <Placemark><name>A</name><styleUrl>#s</styleUrl><Point><coordinates>4,52</coordinates></Point></Placemark></Document></kml>"#;

    #[test]
    fn reads_doc_kml_and_its_icons_from_memory() {
        let bytes = kmz(&[("doc.kml", KML), ("files/pin.png", PNG)]);
        let document = parse_kmz(&bytes, Limits::default()).expect("valid kmz");
        assert_eq!(document.format, GeoFormat::Kmz);
        assert_eq!(document.features.len(), 1);
        assert_eq!(document.features[0]["properties"]["_icon"], "files/pin.png");
        assert!(document.resources.contains_key("files/pin.png"));
    }

    #[test]
    fn falls_back_to_the_shallowest_kml_when_there_is_no_doc_kml() {
        let bytes = kmz(&[("deep/er/other.kml", b"<kml/>"), ("main.kml", KML)]);
        let document = parse_kmz(&bytes, Limits::default()).expect("valid kmz");
        assert_eq!(document.features.len(), 1);
    }

    #[test]
    fn reports_missing_kml_and_broken_archives() {
        assert_eq!(
            parse_kmz(&kmz(&[("readme.txt", b"hi")]), Limits::default()),
            Err(KmzError::NoKml)
        );
        assert!(matches!(
            parse_kmz(b"definitely not a zip", Limits::default()),
            Err(KmzError::Archive(_))
        ));
        assert!(matches!(
            parse_kmz(&kmz(&[("doc.kml", b"<gpx/>")]), Limits::default()),
            Err(KmzError::Kml(KmlError::NotKml(_)))
        ));
    }
}
