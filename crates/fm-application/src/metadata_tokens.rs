//! Metadata tokens for multi-rename patterns (task 0267).
//!
//! The frontend only supplies the token names found in a pattern; the values are resolved here so
//! the browser and desktop hosts produce identical names. Values are sanitised for use in a file
//! name with the same cross-platform rules the rename dialog validates against.

use std::collections::BTreeMap;

use fm_domain::{EntryId, Location};
use fm_metadata::{
    AudioInfo, CaptureTime, PhotoInfo, is_supported_audio_extension, read_audio_info,
    read_audio_info_from_bytes, read_photo_info,
};
use fm_transport_dto::{
    MetadataTokenValuesDto, ResolveMetadataTokensRequestDto, ResolveMetadataTokensResponseDto,
};
use fm_vfs::{EntryRef, ProviderRegistry};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;

const MAX_LOCATIONS: usize = 500;
const MAX_TOKENS: usize = 32;
const MAX_TOKEN_LENGTH: usize = 64;
/// Image files are read only up to this many bytes: EXIF sits in the first segments.
const PHOTO_HEAD_BYTES: u64 = 1024 * 1024;
/// Audio from providers without a native path is read into memory up to this size.
const AUDIO_MEMORY_BYTES: u64 = 16 * 1024 * 1024;
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "tif", "tiff", "heic", "avif",
];

/// A token the resolver understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    Artist,
    AlbumArtist,
    Album,
    Title,
    Track,
    Disc,
    Year,
    Genre,
    ExifDateTaken(String),
    ExifCamera,
    ExifWidth,
    ExifHeight,
    ExifDimensions,
}

impl Token {
    fn needs_audio(&self) -> bool {
        !self.needs_photo()
    }

    fn needs_photo(&self) -> bool {
        matches!(
            self,
            Self::ExifDateTaken(_)
                | Self::ExifCamera
                | Self::ExifWidth
                | Self::ExifHeight
                | Self::ExifDimensions
        )
    }
}

/// Parses a token name such as `artist` or `exif:date-taken:yyyy-MM-dd`.
pub(crate) fn parse_token(name: &str) -> Option<Token> {
    let lowered = name.trim();
    let (head, rest) = match lowered.split_once(':') {
        Some((head, rest)) => (head.to_ascii_lowercase(), Some(rest)),
        None => (lowered.to_ascii_lowercase(), None),
    };
    match (head.as_str(), rest) {
        ("artist", None) => Some(Token::Artist),
        ("albumartist", None) => Some(Token::AlbumArtist),
        ("album", None) => Some(Token::Album),
        ("title", None) => Some(Token::Title),
        ("track", None) => Some(Token::Track),
        ("disc", None) => Some(Token::Disc),
        ("year", None) => Some(Token::Year),
        ("genre", None) => Some(Token::Genre),
        ("exif", Some(field)) => {
            let (field_name, format) = match field.split_once(':') {
                Some((name, format)) => (name.to_ascii_lowercase(), Some(format)),
                None => (field.to_ascii_lowercase(), None),
            };
            match (field_name.as_str(), format) {
                ("date-taken", format) => Some(Token::ExifDateTaken(
                    format.unwrap_or("yyyy-MM-dd").to_owned(),
                )),
                ("camera", None) => Some(Token::ExifCamera),
                ("width", None) => Some(Token::ExifWidth),
                ("height", None) => Some(Token::ExifHeight),
                ("dimensions", None) => Some(Token::ExifDimensions),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Replaces characters that are unsafe in a file name on any supported platform.
pub(crate) fn sanitise(text: &str) -> String {
    let mapped: String = text
        .chars()
        .map(|character| match character {
            '/' | '\\' => '-',
            ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            control if control.is_control() => ' ',
            other => other,
        })
        .collect();
    let collapsed = mapped.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim_matches(|character: char| character == '.' || character.is_whitespace())
        .to_owned()
}

/// Formats a capture time with `yyyy`, `MM`, `dd`, `HH`, `mm` and `ss` placeholders.
pub(crate) fn format_capture_time(time: &CaptureTime, pattern: &str) -> String {
    let mut output = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("yyyy") {
            output.push_str(&format!("{:04}", time.year));
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("MM") {
            output.push_str(&format!("{:02}", time.month));
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("dd") {
            output.push_str(&format!("{:02}", time.day));
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("HH") {
            output.push_str(&format!("{:02}", time.hour));
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("mm") {
            output.push_str(&format!("{:02}", time.minute));
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("ss") {
            output.push_str(&format!("{:02}", time.second));
            rest = tail;
        } else if let Some(character) = rest.chars().next() {
            output.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    output
}

/// What is known about one file.
#[derive(Debug, Default)]
pub(crate) struct Sources {
    pub(crate) audio: Option<AudioInfo>,
    pub(crate) photo: Option<PhotoInfo>,
}

/// The sanitised value of `token` for a file, or `None` when the file has no such field.
pub(crate) fn resolve_token(token: &Token, sources: &Sources) -> Option<String> {
    let audio = sources.audio.as_ref();
    let photo = sources.photo.as_ref();
    let value = match token {
        Token::Artist => audio?.artist.clone(),
        Token::AlbumArtist => audio?.album_artist.clone(),
        Token::Album => audio?.album.clone(),
        Token::Title => audio?.title.clone(),
        Token::Track => audio?.track_number.map(|number| format!("{number:02}")),
        Token::Disc => audio?.disc_number.map(|number| number.to_string()),
        Token::Year => audio?.year.map(|year| year.to_string()),
        Token::Genre => audio?.genre.clone(),
        Token::ExifDateTaken(pattern) => photo?
            .date_taken
            .as_ref()
            .map(|time| format_capture_time(time, pattern)),
        Token::ExifCamera => photo?.camera(),
        Token::ExifWidth => photo?.width.map(|width| width.to_string()),
        Token::ExifHeight => photo?.height.map(|height| height.to_string()),
        Token::ExifDimensions => {
            let photo = photo?;
            photo
                .width
                .zip(photo.height)
                .map(|(w, h)| format!("{w}x{h}"))
        }
    }?;
    let cleaned = sanitise(&value);
    (!cleaned.is_empty()).then_some(cleaned)
}

fn extension_of(location: &Location) -> String {
    location
        .uri
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

async fn read_head(
    providers: &ProviderRegistry,
    location: &Location,
    limit: u64,
) -> Option<Vec<u8>> {
    let provider = providers.resolve(location).ok()?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let reader = provider
        .open_read(&entry, CancellationToken::new())
        .await
        .ok()?;
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes).await.ok()?;
    Some(bytes)
}

async fn load_sources(
    providers: &ProviderRegistry,
    location: &Location,
    need_audio: bool,
    need_photo: bool,
) -> Sources {
    let extension = extension_of(location);
    let mut sources = Sources::default();
    if need_audio && is_supported_audio_extension(&extension) {
        sources.audio = match location.to_native_path() {
            Ok(path) => tokio::task::spawn_blocking(move || read_audio_info(&path).ok())
                .await
                .ok()
                .flatten(),
            Err(_) => read_head(providers, location, AUDIO_MEMORY_BYTES)
                .await
                .and_then(|bytes| read_audio_info_from_bytes(&bytes).ok()),
        };
    }
    if need_photo && IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        sources.photo = read_head(providers, location, PHOTO_HEAD_BYTES)
            .await
            .map(|bytes| read_photo_info(&bytes));
    }
    sources
}

/// Resolves every requested token for every requested file.
pub(crate) async fn resolve_metadata_tokens(
    providers: &ProviderRegistry,
    request: ResolveMetadataTokensRequestDto,
) -> Result<ResolveMetadataTokensResponseDto, ApplicationError> {
    if request.locations.len() > MAX_LOCATIONS {
        return Err(ApplicationError::InvalidRequest(format!(
            "at most {MAX_LOCATIONS} files can be resolved at once"
        )));
    }
    if request.tokens.len() > MAX_TOKENS
        || request
            .tokens
            .iter()
            .any(|token| token.len() > MAX_TOKEN_LENGTH)
    {
        return Err(ApplicationError::InvalidRequest(
            "too many or overly long tokens".to_owned(),
        ));
    }
    let mut parsed: Vec<(String, Token)> = Vec::new();
    let mut unknown_tokens = Vec::new();
    for name in &request.tokens {
        match parse_token(name) {
            Some(token) => parsed.push((name.clone(), token)),
            None => unknown_tokens.push(name.clone()),
        }
    }
    let need_audio = parsed.iter().any(|(_, token)| token.needs_audio());
    let need_photo = parsed.iter().any(|(_, token)| token.needs_photo());
    let mut entries = Vec::with_capacity(request.locations.len());
    for location_dto in request.locations {
        let location: Location = location_dto.clone().into();
        let sources = load_sources(providers, &location, need_audio, need_photo).await;
        let values: BTreeMap<String, String> = parsed
            .iter()
            .filter_map(|(name, token)| {
                resolve_token(token, &sources).map(|value| (name.clone(), value))
            })
            .collect();
        entries.push(MetadataTokenValuesDto {
            location: location_dto,
            values,
        });
    }
    Ok(ResolveMetadataTokensResponseDto {
        entries,
        unknown_tokens,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn audio() -> AudioInfo {
        AudioInfo {
            artist: Some("AC/DC".to_owned()),
            title: Some("Who Made Who?".to_owned()),
            album: Some("  ".to_owned()),
            track_number: Some(7),
            year: Some(1986),
            duration: Some(Duration::from_secs(207)),
            ..AudioInfo::default()
        }
    }

    #[test]
    fn parses_known_tokens_and_rejects_others() {
        assert_eq!(parse_token("artist"), Some(Token::Artist));
        assert_eq!(parse_token("ARTIST"), Some(Token::Artist));
        assert_eq!(
            parse_token("exif:date-taken:yyyy-MM-dd HH.mm"),
            Some(Token::ExifDateTaken("yyyy-MM-dd HH.mm".to_owned()))
        );
        assert_eq!(
            parse_token("exif:date-taken"),
            Some(Token::ExifDateTaken("yyyy-MM-dd".to_owned()))
        );
        assert_eq!(parse_token("exif:camera"), Some(Token::ExifCamera));
        assert_eq!(parse_token("exif:nope"), None);
        assert_eq!(parse_token("bogus"), None);
        assert_eq!(parse_token("artist:extra"), None);
    }

    #[test]
    fn sanitises_for_every_platform() {
        assert_eq!(sanitise("AC/DC"), "AC-DC");
        assert_eq!(sanitise("Who Made Who?"), "Who Made Who_");
        assert_eq!(sanitise("a:b*c\"d<e>f|g"), "a_b_c_d_e_f_g");
        assert_eq!(sanitise("  trailing dot.  "), "trailing dot");
        assert_eq!(sanitise("two\n lines"), "two lines");
    }

    #[test]
    fn formats_capture_times() {
        let time = CaptureTime {
            year: 2024,
            month: 3,
            day: 5,
            hour: 9,
            minute: 7,
            second: 2,
        };
        assert_eq!(format_capture_time(&time, "yyyy-MM-dd"), "2024-03-05");
        assert_eq!(
            format_capture_time(&time, "yyyyMMdd_HHmmss"),
            "20240305_090702"
        );
        assert_eq!(format_capture_time(&time, "d=dd"), "d=05");
    }

    #[test]
    fn resolves_audio_fields_sanitised_and_pads_the_track() {
        let sources = Sources {
            audio: Some(audio()),
            photo: None,
        };
        assert_eq!(
            resolve_token(&Token::Artist, &sources).as_deref(),
            Some("AC-DC")
        );
        assert_eq!(
            resolve_token(&Token::Title, &sources).as_deref(),
            Some("Who Made Who_")
        );
        assert_eq!(
            resolve_token(&Token::Track, &sources).as_deref(),
            Some("07")
        );
        assert_eq!(
            resolve_token(&Token::Year, &sources).as_deref(),
            Some("1986")
        );
        // Blank and absent fields are reported as missing.
        assert_eq!(resolve_token(&Token::Album, &sources), None);
        assert_eq!(resolve_token(&Token::Genre, &sources), None);
        assert_eq!(resolve_token(&Token::ExifCamera, &sources), None);
    }

    #[test]
    fn resolves_photo_fields() {
        let sources = Sources {
            audio: None,
            photo: Some(PhotoInfo {
                date_taken: Some(CaptureTime {
                    year: 2020,
                    month: 12,
                    day: 31,
                    hour: 23,
                    minute: 59,
                    second: 58,
                }),
                camera_make: Some("Nikon".to_owned()),
                camera_model: Some("D850".to_owned()),
                width: Some(8256),
                height: Some(5504),
            }),
        };
        assert_eq!(
            resolve_token(&Token::ExifDateTaken("yyyyMMdd".to_owned()), &sources).as_deref(),
            Some("20201231")
        );
        assert_eq!(
            resolve_token(&Token::ExifCamera, &sources).as_deref(),
            Some("Nikon D850")
        );
        assert_eq!(
            resolve_token(&Token::ExifDimensions, &sources).as_deref(),
            Some("8256x5504")
        );
    }
}
