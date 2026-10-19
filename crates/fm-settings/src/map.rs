//! Map settings (task 0275): which basemap the shared map component shows.
//!
//! The default is OpenStreetMap's public raster tiles, whose usage policy requires visible
//! attribution, an identifying user agent and no bulk downloading. Everything is replaceable, and
//! online tiles can be switched off so only a local PMTiles file or a blank grid is used.

use fm_domain::Location;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// Default raster tile template.
pub const DEFAULT_TILE_TEMPLATE: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
/// Attribution the default tiles require.
pub const DEFAULT_ATTRIBUTION: &str = "\u{a9} OpenStreetMap contributors";
/// Highest zoom level the default tiles provide.
pub const DEFAULT_MAX_ZOOM: u8 = 19;
const MAX_ZOOM_LIMIT: u8 = 24;
const MAX_FIELD_LENGTH: usize = 2048;

/// Where basemap imagery comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MapBasemap {
    /// A raster tile URL template with `{z}`, `{x}` and `{y}` placeholders. Fetched by the
    /// backend, so the web view never contacts the tile host.
    Raster {
        /// URL template.
        url_template: String,
        /// Attribution text shown on the map; required.
        attribution: String,
        /// Highest zoom level the provider serves.
        max_zoom: u8,
    },
    /// A MapLibre style document URL. The web view fetches the style and its tiles directly.
    Style {
        /// Style URL.
        url: String,
        /// Attribution shown in addition to the style's own.
        attribution: String,
    },
    /// A local raster PMTiles archive, read through the VFS for fully offline use.
    Pmtiles {
        /// The `.pmtiles` file.
        location: Location,
        /// Attribution text shown on the map; required.
        attribution: String,
    },
}

impl Default for MapBasemap {
    fn default() -> Self {
        Self::Raster {
            url_template: DEFAULT_TILE_TEMPLATE.to_owned(),
            attribution: DEFAULT_ATTRIBUTION.to_owned(),
            max_zoom: DEFAULT_MAX_ZOOM,
        }
    }
}

/// Why map settings were rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MapSettingsError {
    /// A required value is missing or malformed.
    #[error("{0}")]
    Invalid(String),
}

fn invalid(message: &str) -> MapSettingsError {
    MapSettingsError::Invalid(message.to_owned())
}

/// Accepts https, and plain http only towards the local machine (for self-hosted tile servers).
fn check_url(text: &str, placeholders: bool) -> Result<Url, MapSettingsError> {
    if text.len() > MAX_FIELD_LENGTH {
        return Err(invalid("the URL is too long"));
    }
    let probe = if placeholders {
        text.replace("{z}", "0")
            .replace("{x}", "0")
            .replace("{y}", "0")
    } else {
        text.to_owned()
    };
    let url = Url::parse(&probe).map_err(|_| invalid("the URL is not valid"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    match url.scheme() {
        "https" => {}
        "http" if local => {}
        _ => {
            return Err(invalid(
                "use an https URL (http is only allowed for localhost)",
            ));
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("the URL must not contain credentials"));
    }
    if url.host_str().is_none() {
        return Err(invalid("the URL needs a host"));
    }
    Ok(url)
}

impl MapBasemap {
    /// Checks the basemap; the same rules apply on every host.
    ///
    /// # Errors
    ///
    /// Returns a user-presentable reason when the configuration cannot be used.
    pub fn validate(&self) -> Result<(), MapSettingsError> {
        match self {
            Self::Raster {
                url_template,
                attribution,
                max_zoom,
            } => {
                for placeholder in ["{z}", "{x}", "{y}"] {
                    if !url_template.contains(placeholder) {
                        return Err(MapSettingsError::Invalid(format!(
                            "the tile URL must contain {placeholder}"
                        )));
                    }
                }
                check_url(url_template, true)?;
                if attribution.trim().is_empty() {
                    return Err(invalid("attribution is required by tile providers"));
                }
                if *max_zoom == 0 || *max_zoom > MAX_ZOOM_LIMIT {
                    return Err(invalid("the maximum zoom must be between 1 and 24"));
                }
            }
            Self::Style { url, .. } => {
                check_url(url, false)?;
            }
            Self::Pmtiles {
                location,
                attribution,
            } => {
                if !location.uri.to_ascii_lowercase().ends_with(".pmtiles") {
                    return Err(invalid("choose a .pmtiles file"));
                }
                if attribution.trim().is_empty() {
                    return Err(invalid("attribution is required for map data"));
                }
            }
        }
        if let Self::Style { attribution, .. } | Self::Pmtiles { attribution, .. } = self
            && attribution.len() > MAX_FIELD_LENGTH
        {
            return Err(invalid("the attribution is too long"));
        }
        Ok(())
    }

    /// The remote host that receives requests for this basemap, if any.
    #[must_use]
    pub fn remote_host(&self) -> Option<String> {
        let text = match self {
            Self::Raster { url_template, .. } => url_template.as_str(),
            Self::Style { url, .. } => url.as_str(),
            Self::Pmtiles { .. } => return None,
        };
        check_url(text, true)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
    }
}

/// How the map component gets its imagery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MapSettings {
    /// Whether any request may go to a remote tile or style host; when off, only a local PMTiles
    /// file or a blank grid is shown.
    pub online_tiles: bool,
    /// The configured basemap.
    pub basemap: MapBasemap,
}

impl Default for MapSettings {
    fn default() -> Self {
        Self {
            online_tiles: true,
            basemap: MapBasemap::default(),
        }
    }
}

impl MapSettings {
    /// Checks the whole map configuration.
    ///
    /// # Errors
    ///
    /// Returns a user-presentable reason when the basemap cannot be used.
    pub fn validate(&self) -> Result<(), MapSettingsError> {
        self.basemap.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(url: &str) -> MapBasemap {
        MapBasemap::Raster {
            url_template: url.to_owned(),
            attribution: "x".into(),
            max_zoom: 18,
        }
    }

    #[test]
    fn the_default_is_valid_and_names_its_host() {
        let settings = MapSettings::default();
        assert!(settings.validate().is_ok());
        assert_eq!(
            settings.basemap.remote_host().as_deref(),
            Some("tile.openstreetmap.org")
        );
    }

    #[test]
    fn raster_templates_need_placeholders_https_and_no_credentials() {
        assert!(
            raster("https://tiles.example/{z}/{x}/{y}.png")
                .validate()
                .is_ok()
        );
        assert!(
            raster("https://tiles.example/{z}/{x}.png")
                .validate()
                .is_err()
        );
        assert!(
            raster("http://tiles.example/{z}/{x}/{y}.png")
                .validate()
                .is_err()
        );
        assert!(
            raster("http://localhost:8080/{z}/{x}/{y}.png")
                .validate()
                .is_ok()
        );
        assert!(
            raster("https://user:pw@tiles.example/{z}/{x}/{y}.png")
                .validate()
                .is_err()
        );
        assert!(raster("file:///tiles/{z}/{x}/{y}.png").validate().is_err());
    }

    #[test]
    fn attribution_and_zoom_are_required() {
        let missing = MapBasemap::Raster {
            url_template: "https://t.example/{z}/{x}/{y}.png".into(),
            attribution: "  ".into(),
            max_zoom: 18,
        };
        assert!(missing.validate().is_err());
        let too_deep = MapBasemap::Raster {
            url_template: "https://t.example/{z}/{x}/{y}.png".into(),
            attribution: "x".into(),
            max_zoom: 40,
        };
        assert!(too_deep.validate().is_err());
    }

    #[test]
    fn pmtiles_must_be_a_pmtiles_file_and_has_no_remote_host() {
        let location = Location {
            provider_id: fm_domain::ProviderId::new("local"),
            uri: "file:///maps/world.PMTiles".into(),
        };
        let ok = MapBasemap::Pmtiles {
            location: location.clone(),
            attribution: "x".into(),
        };
        assert!(ok.validate().is_ok());
        assert_eq!(ok.remote_host(), None);
        let wrong = MapBasemap::Pmtiles {
            location: Location {
                uri: "file:///maps/world.mbtiles".into(),
                ..location
            },
            attribution: "x".into(),
        };
        assert!(wrong.validate().is_err());
    }

    #[test]
    fn style_urls_follow_the_same_scheme_rules() {
        let style = |url: &str| MapBasemap::Style {
            url: url.into(),
            attribution: String::new(),
        };
        assert!(style("https://maps.example/style.json").validate().is_ok());
        assert!(style("ftp://maps.example/style.json").validate().is_err());
    }

    #[test]
    fn legacy_settings_without_a_map_section_use_the_default() {
        let parsed: MapSettings = serde_json::from_str("{}").expect("parse");
        assert_eq!(parsed, MapSettings::default());
        let roundtrip: MapSettings =
            serde_json::from_str(&serde_json::to_string(&parsed).expect("serialise"))
                .expect("parse");
        assert_eq!(roundtrip, parsed);
    }
}
