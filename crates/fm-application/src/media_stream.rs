//! Seekable byte-range access to any VFS entry for `<audio>`/`<video>` elements (task 0260).
//!
//! Hosts (Axum route, Tauri custom URI scheme) translate their transport's `Range` header into a
//! [`MediaRangeRequest`] and stream the returned reader back with `Content-Range` /
//! `Accept-Ranges` semantics. Nothing here buffers the file: memory use is bounded by the
//! reader's own buffer, and hosts that must buffer a whole response (Tauri) cap it with
//! [`MediaRangeRequest::max_response_bytes`] and answer with a shorter partial response.

use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, ProviderCapabilities, ProviderReadStream, ProviderRegistry};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::content_streaming::skip_bytes;
use crate::error::ApplicationError;
use crate::file_editor::read_stream_error;

/// An inclusive byte span of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    /// First byte offset.
    pub start: u64,
    /// Last byte offset (inclusive).
    pub end: u64,
}

impl ByteSpan {
    /// Number of bytes in the span.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    /// A span always holds at least one byte.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }
}

/// How a `Range` header resolves against a file of known size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeResolution {
    /// No usable range: serve the whole file with status 200.
    Full,
    /// Serve this span with status 206.
    Partial(ByteSpan),
    /// The range starts at or past the end of the file: status 416.
    Unsatisfiable,
}

/// Resolves an HTTP `Range` header value against `total` bytes (RFC 9110 section 14).
///
/// Absent, non-`bytes`, malformed, and multi-range headers are ignored (answered in full), which
/// the RFC permits. Open-ended (`500-`) and suffix (`-500`) forms are supported.
#[must_use]
pub fn resolve_range(header: Option<&str>, total: u64) -> RangeResolution {
    let Some(spec) = header.and_then(|value| value.trim().strip_prefix("bytes=")) else {
        return RangeResolution::Full;
    };
    if spec.contains(',') {
        return RangeResolution::Full;
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return RangeResolution::Full;
    };
    let (first, last) = (first.trim(), last.trim());
    if first.is_empty() {
        let Ok(suffix) = last.parse::<u64>() else {
            return RangeResolution::Full;
        };
        if suffix == 0 || total == 0 {
            return RangeResolution::Unsatisfiable;
        }
        return RangeResolution::Partial(ByteSpan {
            start: total.saturating_sub(suffix),
            end: total - 1,
        });
    }
    let Ok(start) = first.parse::<u64>() else {
        return RangeResolution::Full;
    };
    let end = if last.is_empty() {
        None
    } else {
        match last.parse::<u64>() {
            Ok(end) if end >= start => Some(end),
            _ => return RangeResolution::Full,
        }
    };
    if start >= total {
        return RangeResolution::Unsatisfiable;
    }
    RangeResolution::Partial(ByteSpan {
        start,
        end: end.map_or(total - 1, |end| end.min(total - 1)),
    })
}

/// One media byte-range request.
#[derive(Debug, Clone)]
pub struct MediaRangeRequest {
    /// The file to stream.
    pub location: Location,
    /// The raw `Range` header value, when the client sent one.
    pub range_header: Option<String>,
    /// Upper bound on the bytes returned; a larger answer is shortened to a partial response.
    pub max_response_bytes: Option<u64>,
}

/// A file stream ready to send.
pub struct MediaContent {
    /// Total size of the file.
    pub total_size: u64,
    /// The served span, or `None` when the whole file is served (status 200).
    pub span: Option<ByteSpan>,
    /// Number of bytes the reader yields.
    pub content_length: u64,
    /// Browser MIME type derived from the file name.
    pub content_type: &'static str,
    /// Reader limited to exactly `content_length` bytes.
    pub reader: ProviderReadStream,
    /// Cancels the provider read when dropped by the host; hold it as long as the body streams.
    pub cancellation: CancellationToken,
}

/// Result of resolving a media request.
pub enum MediaOutcome {
    /// Stream this content.
    Content(MediaContent),
    /// Status 416 with `Content-Range: bytes */total_size`.
    Unsatisfiable {
        /// Total size of the file.
        total_size: u64,
    },
}

/// Maps a file name's extension to a browser-playable MIME type.
#[must_use]
pub fn media_content_type(file_name: &str) -> &'static str {
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "weba" => "audio/webm",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

/// Opens the requested span of a file as a bounded reader.
pub(crate) async fn open_media_range(
    providers: &ProviderRegistry,
    request: MediaRangeRequest,
) -> Result<MediaOutcome, ApplicationError> {
    let provider = providers
        .resolve(&request.location)
        .map_err(ApplicationError::from)?;
    let capabilities = provider
        .capabilities_for(&request.location)
        .map_err(ApplicationError::from)?;
    capabilities
        .require(ProviderCapabilities::READ)
        .map_err(ApplicationError::from)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: request.location.clone(),
    };
    let cancellation = CancellationToken::new();
    let total_size = provider
        .file_size(&entry, cancellation.clone())
        .await
        .map_err(ApplicationError::from)?;

    let resolution = resolve_range(request.range_header.as_deref(), total_size);
    let mut span = match resolution {
        RangeResolution::Unsatisfiable => return Ok(MediaOutcome::Unsatisfiable { total_size }),
        RangeResolution::Partial(span) => Some(span),
        RangeResolution::Full => None,
    };
    if let Some(cap) = request.max_response_bytes.filter(|cap| *cap > 0) {
        let current = span.unwrap_or(ByteSpan {
            start: 0,
            end: total_size.saturating_sub(1),
        });
        if total_size > 0 && current.len() > cap {
            span = Some(ByteSpan {
                start: current.start,
                end: current.start + cap - 1,
            });
        }
    }
    let content_length = span.map_or(total_size, |span| span.len());
    let reader: ProviderReadStream = match span {
        Some(span) if capabilities.contains(ProviderCapabilities::RANDOM_ACCESS) => provider
            .read_range(&entry, span.start, Some(span.len()), cancellation.clone())
            .await
            .map_err(ApplicationError::from)?,
        Some(span) => {
            let mut sequential = provider
                .open_read(&entry, cancellation.clone())
                .await
                .map_err(ApplicationError::from)?;
            skip_bytes(&mut sequential, span.start)
                .await
                .map_err(read_stream_error)?;
            sequential
        }
        None => provider
            .open_read(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?,
    };
    let file_name = request
        .location
        .uri
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    Ok(MediaOutcome::Content(MediaContent {
        total_size,
        span,
        content_length,
        content_type: media_content_type(file_name),
        reader: Box::pin(reader.take(content_length)),
        cancellation,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partial(start: u64, end: u64) -> RangeResolution {
        RangeResolution::Partial(ByteSpan { start, end })
    }

    #[test]
    fn missing_or_foreign_headers_serve_the_whole_file() {
        assert_eq!(resolve_range(None, 100), RangeResolution::Full);
        assert_eq!(resolve_range(Some("items=0-5"), 100), RangeResolution::Full);
        assert_eq!(resolve_range(Some("bytes=abc"), 100), RangeResolution::Full);
        assert_eq!(resolve_range(Some("bytes=x-5"), 100), RangeResolution::Full);
    }

    #[test]
    fn bounded_open_ended_and_suffix_ranges_resolve() {
        assert_eq!(resolve_range(Some("bytes=10-19"), 100), partial(10, 19));
        assert_eq!(resolve_range(Some("bytes=90-"), 100), partial(90, 99));
        assert_eq!(resolve_range(Some("bytes=-10"), 100), partial(90, 99));
        assert_eq!(resolve_range(Some("bytes=0-0"), 100), partial(0, 0));
    }

    #[test]
    fn ends_past_the_file_are_clamped_and_oversized_suffixes_cover_everything() {
        assert_eq!(resolve_range(Some("bytes=95-500"), 100), partial(95, 99));
        assert_eq!(resolve_range(Some("bytes=-500"), 100), partial(0, 99));
    }

    #[test]
    fn out_of_range_requests_are_unsatisfiable() {
        assert_eq!(
            resolve_range(Some("bytes=100-"), 100),
            RangeResolution::Unsatisfiable
        );
        assert_eq!(
            resolve_range(Some("bytes=500-600"), 100),
            RangeResolution::Unsatisfiable
        );
        assert_eq!(
            resolve_range(Some("bytes=-0"), 100),
            RangeResolution::Unsatisfiable
        );
        assert_eq!(
            resolve_range(Some("bytes=-5"), 0),
            RangeResolution::Unsatisfiable
        );
    }

    #[test]
    fn inverted_and_multi_ranges_are_ignored() {
        assert_eq!(resolve_range(Some("bytes=9-3"), 100), RangeResolution::Full);
        assert_eq!(
            resolve_range(Some("bytes=0-1, 5-6"), 100),
            RangeResolution::Full
        );
    }

    #[test]
    fn content_types_follow_the_extension() {
        assert_eq!(media_content_type("Song.MP3"), "audio/mpeg");
        assert_eq!(media_content_type("clip.mp4"), "video/mp4");
        assert_eq!(media_content_type("noext"), "application/octet-stream");
    }
}
