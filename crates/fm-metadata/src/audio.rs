//! Audio tag and stream-property extraction (task 0265, reused by task 0267).
//!
//! Reads a local file through `lofty`. Unreadable, truncated or unsupported files yield an
//! [`AudioReadError`] instead of panicking, so batch scans can degrade gracefully.

use std::path::Path;
use std::time::Duration;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};

/// Extensions the audio reader accepts.
pub const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wav", "wma", "ape", "wv", "aiff", "aif",
];

/// Whether `extension` (without dot, any case) names a supported audio format.
#[must_use]
pub fn is_supported_audio_extension(extension: &str) -> bool {
    SUPPORTED_AUDIO_EXTENSIONS
        .iter()
        .any(|supported| supported.eq_ignore_ascii_case(extension))
}

/// Tags and stream properties of one audio file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioInfo {
    /// Track artist.
    pub artist: Option<String>,
    /// Album artist.
    pub album_artist: Option<String>,
    /// Track title.
    pub title: Option<String>,
    /// Album title.
    pub album: Option<String>,
    /// Genre.
    pub genre: Option<String>,
    /// Release year.
    pub year: Option<u32>,
    /// Track number on the disc.
    pub track_number: Option<u32>,
    /// Disc number.
    pub disc_number: Option<u32>,
    /// Playing time.
    pub duration: Option<Duration>,
    /// Overall bitrate in kilobits per second.
    pub bitrate_kbps: Option<u32>,
    /// Sampling rate in hertz.
    pub sample_rate_hz: Option<u32>,
    /// Container/codec family, for example `"mpeg"`, `"flac"` or `"mp4"`.
    pub format: String,
    /// Whether the codec is lossless.
    pub lossless: bool,
}

/// Why an audio file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum AudioReadError {
    /// The file could not be opened or its content was not recognised audio.
    #[error("could not read audio metadata: {0}")]
    Unreadable(String),
}

fn lossless(file_type: FileType) -> bool {
    matches!(
        file_type,
        FileType::Flac | FileType::Ape | FileType::WavPack | FileType::Wav | FileType::Aiff
    )
}

fn format_name(file_type: FileType) -> String {
    format!("{file_type:?}").to_ascii_lowercase()
}

/// Reads tags and properties from a local audio file.
///
/// # Errors
///
/// Returns [`AudioReadError::Unreadable`] when the file cannot be opened or parsed.
pub fn read_audio_info(path: &Path) -> Result<AudioInfo, AudioReadError> {
    let tagged = Probe::open(path)
        .map_err(|error| AudioReadError::Unreadable(error.to_string()))?
        .options(ParseOptions::new().read_cover_art(false))
        .read()
        .map_err(|error| AudioReadError::Unreadable(error.to_string()))?;
    let properties = tagged.properties();
    let file_type = tagged.file_type();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let text = |value: Option<std::borrow::Cow<'_, str>>| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let duration = properties.duration();
    Ok(AudioInfo {
        artist: tag.and_then(|tag| text(tag.artist())),
        album_artist: tag.and_then(|tag| {
            text(
                tag.get_string(ItemKey::AlbumArtist)
                    .map(std::borrow::Cow::Borrowed),
            )
        }),
        title: tag.and_then(|tag| text(tag.title())),
        album: tag.and_then(|tag| text(tag.album())),
        genre: tag.and_then(|tag| text(tag.genre())),
        year: tag
            .and_then(Accessor::date)
            .map(|date| u32::from(date.year)),
        track_number: tag.and_then(Accessor::track),
        disc_number: tag.and_then(Accessor::disk),
        duration: (!duration.is_zero()).then_some(duration),
        bitrate_kbps: properties.audio_bitrate(),
        sample_rate_hz: properties.sample_rate(),
        format: format_name(file_type),
        lossless: lossless(file_type),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_audio_extensions_case_insensitively() {
        assert!(is_supported_audio_extension("MP3"));
        assert!(is_supported_audio_extension("flac"));
        assert!(!is_supported_audio_extension("txt"));
    }

    #[test]
    fn garbage_files_are_reported_not_panicked_on() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("broken.mp3");
        std::fs::write(&path, b"this is not audio").expect("write fixture");
        assert!(read_audio_info(&path).is_err());
        assert!(read_audio_info(&directory.path().join("missing.flac")).is_err());
    }

    /// Minimal PCM WAV: 1 second of 8 kHz mono silence, which has no tags.
    fn wav_bytes() -> Vec<u8> {
        let samples = 8000_u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + samples).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8000_u32.to_le_bytes());
        bytes.extend_from_slice(&8000_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&samples.to_le_bytes());
        bytes.extend(std::iter::repeat_n(128_u8, samples as usize));
        bytes
    }

    #[test]
    fn reads_tags_written_into_a_wav() {
        use lofty::config::WriteOptions;
        use lofty::tag::{Tag, TagExt, TagType};

        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("tagged.wav");
        std::fs::write(&path, wav_bytes()).expect("write fixture");
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist("Ada".to_owned());
        tag.set_title("Analytical Engine".to_owned());
        tag.set_album("Notes".to_owned());
        tag.set_track(7);
        tag.save_to_path(&path, WriteOptions::default())
            .expect("write tag");

        let info = read_audio_info(&path).expect("tagged wav must parse");

        assert_eq!(info.artist.as_deref(), Some("Ada"));
        assert_eq!(info.title.as_deref(), Some("Analytical Engine"));
        assert_eq!(info.album.as_deref(), Some("Notes"));
        assert_eq!(info.track_number, Some(7));
    }

    #[test]
    fn reads_stream_properties_of_an_untagged_wav() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("silence.wav");
        std::fs::write(&path, wav_bytes()).expect("write fixture");

        let info = read_audio_info(&path).expect("wav must parse");

        assert_eq!(info.format, "wav");
        assert!(info.lossless);
        assert_eq!(info.sample_rate_hz, Some(8000));
        assert_eq!(info.duration.map(|d| d.as_secs()), Some(1));
        assert_eq!(info.title, None);
    }
}
