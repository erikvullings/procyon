//! Thumbnail generation for images, CBZ/CBR comic-archive first pages,
//! H.264/MP4 video first frames, and PDF first-page embedded images
//! (task 0134).
//!
//! Provider-agnostic: decodes already-read bytes with `fm-metadata`'s
//! pure-Rust pipelines rather than an OS-native API or shelling out to
//! `ffmpeg`/a PDF renderer, so it works for any [`FileSystemProvider`] that
//! supports [`ProviderCapabilities::READ`] (local today; archive entries
//! reuse the same path; in principle a future remote provider too), not
//! just local files. CBZ/CBR are not decoded directly here - a `.cbz`/`.cbr`
//! entry's first page is fetched by resolving the existing `archive://`
//! provider (already used for browsing ZIP/7z/RAR archives, task
//! 0104/fm-archive) and reading its first image entry, exactly as
//! [`fm_domain::Location`]'s own archive-URI convention already lets the
//! frontend browse into these files
//! (`frontend/src/features/navigation/archive-location.ts`).

use std::path::Path;

use fm_domain::{EntryId, EntryKind, Location};
use fm_metadata::{
    GeneratedThumbnail, MAX_SOURCE_BYTES, ThumbnailCache, ThumbnailError, ThumbnailSize,
    generate_image_thumbnail, generate_pdf_thumbnail, generate_video_thumbnail,
    is_supported_audio_extension, is_supported_image_extension, is_supported_pdf_extension,
    is_supported_video_extension, read_embedded_picture, thumbnail_content_type,
};
use fm_vfs::{EntryRef, ListOptions, ProviderCapabilities, ProviderRegistry};
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::file_editor::read_stream_error;

/// Maximum thumbnail generations allowed to run concurrently, so scrolling a
/// directory of thousands of images doesn't spawn unbounded CPU-bound decode
/// work at once (task 0134 acceptance criteria: "...doesn't stall the UI").
const MAX_CONCURRENT_GENERATIONS: usize = 4;

/// Entries read per listing page, and in total, when looking for a folder's cover, so a huge
/// directory is never walked just to draw one tile.
const FOLDER_PROBE_PAGE_SIZE: usize = 512;
const FOLDER_PROBE_MAX_ENTRIES: usize = 2048;

/// Longest a folder or audio probe may take; a slow remote listing gives up (icon fallback)
/// instead of holding one of the few generation slots.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Leading bytes read from an audio file when looking for its embedded picture.
const AUDIO_PICTURE_PROBE_BYTES: usize = 4 * 1024 * 1024;

/// Second, larger read tried only when the first found no picture and was cut short, because a
/// tag with big artwork can run past the first read.
const AUDIO_PICTURE_RETRY_BYTES: usize = 16 * 1024 * 1024;

/// Tracks of one folder tried for an embedded picture before giving up.
const MAX_TRACKS_PROBED: usize = 3;

/// Maximum total bytes the on-disk thumbnail cache may occupy.
const MAX_CACHE_BYTES: u64 = 200 * 1024 * 1024;

/// Owns the on-disk thumbnail cache and the generation concurrency budget.
pub(crate) struct ThumbnailService {
    cache: ThumbnailCache,
    semaphore: Semaphore,
}

impl ThumbnailService {
    pub(crate) fn new(cache_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            cache: ThumbnailCache::new(cache_root, MAX_CACHE_BYTES),
            semaphore: Semaphore::new(MAX_CONCURRENT_GENERATIONS),
        }
    }

    /// Generates (or reuses a cached) thumbnail for `location` at `size`.
    pub(crate) async fn thumbnail(
        &self,
        providers: &ProviderRegistry,
        location: &Location,
        size: ThumbnailSize,
    ) -> Result<GeneratedThumbnail, ApplicationError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| ApplicationError::Internal)?;

        let (source_bytes, kind) = read_source_bytes(providers, location).await?;

        let key = ThumbnailCache::cache_key(&source_bytes, size);
        // Image thumbnails keep transparency (as PNG) since the cache was keyed only by content and
        // size: entries written earlier are JPEGs with a black background where the image was
        // transparent, so image keys carry a marker that retires them. Video and PDF are unchanged.
        let key = if matches!(kind, SourceKind::Image) {
            format!("{key}-alpha")
        } else {
            key
        };
        if let Some(cached) = self.cache.get(&key) {
            return Ok(GeneratedThumbnail {
                content_type: thumbnail_content_type(&cached),
                bytes: cached,
            });
        }

        let generated = tokio::task::spawn_blocking(move || match kind {
            SourceKind::Image => generate_image_thumbnail(&source_bytes, size),
            SourceKind::Video => generate_video_thumbnail(&source_bytes, size),
            SourceKind::Pdf => generate_pdf_thumbnail(&source_bytes, size),
        })
        .await
        .map_err(|_| ApplicationError::Internal)?
        .map_err(map_thumbnail_error)?;

        // Best-effort: a cache-write failure (e.g. a full disk) must not
        // fail the request, since the caller already has the bytes it asked
        // for. The next request for the same key just regenerates.
        let _ = self.cache.put(&key, &generated.bytes);

        Ok(generated)
    }
}

/// Which `fm-metadata` generator [`read_source_bytes`] resolved for a
/// location, so the caller knows which decoder to run on a background
/// thread once the bytes (and any cache lookup) are in hand.
enum SourceKind {
    Image,
    Video,
    Pdf,
}

/// Reads the raw source bytes to thumbnail: direct bytes for a supported
/// image, video or PDF extension, or the first page of a `.cbz`/`.cbr`
/// comic archive (itself an image once extracted).
async fn read_source_bytes(
    providers: &ProviderRegistry,
    location: &Location,
) -> Result<(Vec<u8>, SourceKind), ApplicationError> {
    let name = location
        .name()
        .map_err(|_| map_thumbnail_error(ThumbnailError::UnsupportedFormat))?;
    let extension = extension_of(&name);
    let known_file = extension.as_deref().is_some_and(is_known_file_extension);

    // A known file extension is taken at its word, which saves one `inspect` round trip per tile
    // on remote providers; anything else might be a folder (`Vol. 1`), so ask.
    if !known_file && is_directory(providers, location).await {
        let cover = tokio::time::timeout(PROBE_TIMEOUT, read_folder_cover(providers, location))
            .await
            .map_err(|_| map_thumbnail_error(ThumbnailError::UnsupportedFormat))??;
        return Ok((cover, SourceKind::Image));
    }
    let extension =
        extension.ok_or_else(|| map_thumbnail_error(ThumbnailError::UnsupportedFormat))?;

    if is_supported_image_extension(&extension) {
        Ok((
            read_whole_file(providers, location).await?,
            SourceKind::Image,
        ))
    } else if extension.eq_ignore_ascii_case("cbz") || extension.eq_ignore_ascii_case("cbr") {
        Ok((
            read_first_comic_page(providers, location).await?,
            SourceKind::Image,
        ))
    } else if is_supported_video_extension(&extension) {
        Ok((
            read_whole_file(providers, location).await?,
            SourceKind::Video,
        ))
    } else if is_supported_pdf_extension(&extension) {
        Ok((read_whole_file(providers, location).await?, SourceKind::Pdf))
    } else if is_supported_audio_extension(&extension) {
        let picture = tokio::time::timeout(PROBE_TIMEOUT, read_audio_picture(providers, location))
            .await
            .map_err(|_| map_thumbnail_error(ThumbnailError::UnsupportedFormat))??
            .ok_or_else(|| map_thumbnail_error(ThumbnailError::UnsupportedFormat))?;
        Ok((picture, SourceKind::Image))
    } else {
        Err(map_thumbnail_error(ThumbnailError::UnsupportedFormat))
    }
}

/// Whether `extension` is one `read_source_bytes` handles as a file.
fn is_known_file_extension(extension: &str) -> bool {
    is_supported_image_extension(extension)
        || extension.eq_ignore_ascii_case("cbz")
        || extension.eq_ignore_ascii_case("cbr")
        || is_supported_video_extension(extension)
        || is_supported_pdf_extension(extension)
        || is_supported_audio_extension(extension)
}

/// Whether `location` is a real directory. Symlinks are not followed (`inspect` reports the link
/// itself), and a provider that cannot inspect simply is not treated as a folder source.
async fn is_directory(providers: &ProviderRegistry, location: &Location) -> bool {
    let Ok(provider) = providers.resolve(location) else {
        return false;
    };
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    provider
        .inspect(&entry, CancellationToken::new())
        .await
        .is_ok_and(|summary| summary.kind == EntryKind::Directory)
}

/// Cover-art file names in priority order; the last one matches by prefix (`AlbumArtSmall.jpg`).
const COVER_STEMS: [&str; 4] = ["cover", "folder", "front", "album"];
const COVER_PREFIX: &str = "albumart";

/// Priority of `name` as a cover file: lower is better, `None` when it is not one.
fn cover_rank(name: &str) -> Option<usize> {
    let path = Path::new(name);
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    if !is_supported_image_extension(&extension) {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    COVER_STEMS
        .iter()
        .position(|candidate| *candidate == stem)
        .or_else(|| stem.starts_with(COVER_PREFIX).then_some(COVER_STEMS.len()))
}

/// Lists one bounded page of the folder and returns the best cover image's bytes.
async fn read_folder_cover(
    providers: &ProviderRegistry,
    folder: &Location,
) -> Result<Vec<u8>, ApplicationError> {
    let provider = providers.resolve(folder).map_err(ApplicationError::from)?;
    let mut entries = Vec::new();
    let mut continuation_token = None;
    loop {
        let page = provider
            .list(
                folder,
                ListOptions {
                    page_size: FOLDER_PROBE_PAGE_SIZE,
                    continuation_token: continuation_token.take(),
                },
                CancellationToken::new(),
            )
            .await
            .map_err(ApplicationError::from)?;
        entries.extend(page.entries);
        match page.continuation_token {
            Some(token) if page.has_more && entries.len() < FOLDER_PROBE_MAX_ENTRIES => {
                continuation_token = Some(token);
            }
            _ => break,
        }
    }
    let files: Vec<_> = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .collect();
    if let Some(cover) = files
        .iter()
        .filter_map(|entry| cover_rank(&entry.name).map(|rank| (rank, entry)))
        .min_by_key(|(rank, entry)| (*rank, entry.name.to_lowercase()))
        .map(|(_, entry)| entry)
    {
        return read_whole_file(providers, &cover.location).await;
    }

    // No cover file: the first track (by name) that carries a picture of its own. Only the head
    // of each file is read, where tags live, and only a few tracks are tried.
    let mut tracks: Vec<_> = files
        .iter()
        .filter(|entry| {
            extension_of(&entry.name)
                .is_some_and(|extension| is_supported_audio_extension(&extension))
        })
        .collect();
    tracks.sort_by_key(|entry| entry.name.to_lowercase());
    for track in tracks.into_iter().take(MAX_TRACKS_PROBED) {
        if let Some(picture) = read_audio_picture(providers, &track.location).await? {
            return Ok(picture);
        }
    }
    Err(map_thumbnail_error(ThumbnailError::UnsupportedFormat))
}

/// The picture embedded in an audio file, reading only the head of the file where tags live.
async fn read_audio_picture(
    providers: &ProviderRegistry,
    location: &Location,
) -> Result<Option<Vec<u8>>, ApplicationError> {
    for limit in [AUDIO_PICTURE_PROBE_BYTES, AUDIO_PICTURE_RETRY_BYTES] {
        let head = read_file_prefix(providers, location, limit).await?;
        let cut_short = head.len() >= limit;
        let picture = tokio::task::spawn_blocking(move || read_embedded_picture(&head))
            .await
            .map_err(|_| ApplicationError::Internal)?;
        if picture.is_some() || !cut_short {
            return Ok(picture);
        }
    }
    Ok(None)
}

/// Reads at most `limit` leading bytes of a file; a longer file is cut, not rejected.
async fn read_file_prefix(
    providers: &ProviderRegistry,
    location: &Location,
    limit: usize,
) -> Result<Vec<u8>, ApplicationError> {
    let provider = providers
        .resolve(location)
        .map_err(ApplicationError::from)?;
    provider
        .capabilities_for(location)
        .map_err(ApplicationError::from)?
        .require(ProviderCapabilities::READ)
        .map_err(ApplicationError::from)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let mut reader = provider
        .open_read(&entry, CancellationToken::new())
        .await
        .map_err(ApplicationError::from)?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    while bytes.len() < limit {
        let read = reader.read(&mut buffer).await.map_err(read_stream_error)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read.min(limit - bytes.len())]);
    }
    Ok(bytes)
}

fn extension_of(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
}

/// Builds the `archive://` root location for a local `.cbz`/`.cbr` entry,
/// mirroring `archiveRootForEntry` in
/// `frontend/src/features/navigation/archive-location.ts` exactly (same
/// URI shape, same local-provider-only restriction - a non-local comic
/// archive isn't browsable as an archive today either).
fn archive_root_for(location: &Location) -> Option<Location> {
    if location.provider_id.as_str() != "local" {
        return None;
    }
    let stripped = location.uri.strip_prefix("file://")?;
    Location::parse(&format!("archive://{stripped}!/")).ok()
}

async fn read_first_comic_page(
    providers: &ProviderRegistry,
    location: &Location,
) -> Result<Vec<u8>, ApplicationError> {
    let archive_root = archive_root_for(location)
        .ok_or_else(|| map_thumbnail_error(ThumbnailError::UnsupportedFormat))?;
    let provider = providers
        .resolve(&archive_root)
        .map_err(ApplicationError::from)?;
    let cancellation = CancellationToken::new();
    let page = provider
        .list(
            &archive_root,
            ListOptions {
                page_size: 8192,
                continuation_token: None,
            },
            cancellation,
        )
        .await
        .map_err(ApplicationError::from)?;

    let mut pages: Vec<_> = page
        .entries
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .filter(|entry| {
            entry
                .extension
                .as_deref()
                .is_some_and(is_supported_image_extension)
        })
        .collect();
    pages.sort_by(|left, right| left.name.cmp(&right.name));
    let first_page = pages
        .into_iter()
        .next()
        .ok_or_else(|| map_thumbnail_error(ThumbnailError::UnsupportedFormat))?;

    read_whole_file(providers, &first_page.location).await
}

async fn read_whole_file(
    providers: &ProviderRegistry,
    location: &Location,
) -> Result<Vec<u8>, ApplicationError> {
    let provider = providers
        .resolve(location)
        .map_err(ApplicationError::from)?;
    provider
        .capabilities_for(location)
        .map_err(ApplicationError::from)?
        .require(ProviderCapabilities::READ)
        .map_err(ApplicationError::from)?;

    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let cancellation = CancellationToken::new();

    if let Ok(size) = provider.file_size(&entry, cancellation.clone()).await
        && size > MAX_SOURCE_BYTES
    {
        return Err(map_thumbnail_error(ThumbnailError::SourceTooLarge {
            size,
            limit: MAX_SOURCE_BYTES,
        }));
    }

    let mut reader = provider
        .open_read(&entry, cancellation)
        .await
        .map_err(ApplicationError::from)?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).await.map_err(read_stream_error)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() as u64 > MAX_SOURCE_BYTES {
            return Err(map_thumbnail_error(ThumbnailError::SourceTooLarge {
                size: bytes.len() as u64,
                limit: MAX_SOURCE_BYTES,
            }));
        }
    }
    Ok(bytes)
}

/// Every [`ThumbnailError`] is a "no thumbnail available" outcome from the
/// caller's point of view, exactly like [`crate::platform_mapping::map_file_icon_error`]
/// treats [`fm_platform::PlatformError::Unsupported`] - reported as
/// [`ApplicationError::NotFound`] so hosts return 404 and the frontend
/// falls back to the generic type icon, not as a hard failure.
fn map_thumbnail_error(_error: ThumbnailError) -> ApplicationError {
    ApplicationError::NotFound
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_archive::ArchiveFileSystemProvider;
    use fm_vfs_local::LocalFileSystemProvider;
    use std::io::Cursor;
    use std::sync::Arc;

    fn providers() -> ProviderRegistry {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        providers.register(Arc::new(ArchiveFileSystemProvider::new()));
        providers
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        // Opaque on purpose: a plain photo, not a transparent logo (which is kept as PNG).
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([10, 200, 30, 255]),
        ));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode fixture png");
        bytes
    }

    fn location_for(path: &std::path::Path) -> Location {
        Location::from_native_path(path).expect("native path to location")
    }

    #[tokio::test]
    async fn generates_a_thumbnail_for_a_plain_image_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("photo.png");
        std::fs::write(&target, png_bytes(200, 100)).expect("write fixture");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&target), ThumbnailSize::Small)
            .await
            .expect("thumbnail must succeed");

        assert_eq!(thumbnail.content_type, "image/jpeg");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert!(decoded.width() <= ThumbnailSize::Small.max_dimension());
    }

    #[tokio::test]
    async fn a_stale_jpeg_cached_before_transparency_support_is_not_served() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("logo.png");
        let mut logo = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 0]));
        for x in 20..44 {
            for y in 20..44 {
                logo.put_pixel(x, y, image::Rgba([250, 0, 0, 255]));
            }
        }
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(logo)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode fixture png");
        std::fs::write(&target, &bytes).expect("write fixture");

        // What an older build left behind: a JPEG under the content-and-size-only key.
        let cache_directory = dir.path().join("cache");
        std::fs::create_dir_all(&cache_directory).expect("cache dir");
        let old_key = ThumbnailCache::cache_key(&bytes, ThumbnailSize::Small);
        std::fs::write(
            cache_directory.join(format!("{old_key}.jpg")),
            b"stale jpeg",
        )
        .expect("write stale entry");

        let service = ThumbnailService::new(cache_directory);
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&target), ThumbnailSize::Small)
            .await
            .expect("thumbnail");

        assert_eq!(thumbnail.content_type, "image/png");
        assert_ne!(thumbnail.bytes, b"stale jpeg");
        // And a repeat request serves the new PNG from the cache with the right content type.
        let again = service
            .thumbnail(&providers(), &location_for(&target), ThumbnailSize::Small)
            .await
            .expect("cached thumbnail");
        assert_eq!(again.content_type, "image/png");
        assert_eq!(again.bytes, thumbnail.bytes);
    }

    #[tokio::test]
    async fn a_second_request_for_the_same_file_hits_the_disk_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("photo.png");
        std::fs::write(&target, png_bytes(64, 64)).expect("write fixture");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let location = location_for(&target);
        let first = service
            .thumbnail(&providers(), &location, ThumbnailSize::Medium)
            .await
            .expect("first generation");
        let second = service
            .thumbnail(&providers(), &location, ThumbnailSize::Medium)
            .await
            .expect("second, cached, generation");

        assert_eq!(first.bytes, second.bytes);
    }

    #[tokio::test]
    async fn a_changed_file_produces_a_different_thumbnail_not_the_stale_cached_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("photo.png");
        std::fs::write(&target, png_bytes(64, 64)).expect("write fixture");
        let service = ThumbnailService::new(dir.path().join("cache"));
        let location = location_for(&target);
        let before = service
            .thumbnail(&providers(), &location, ThumbnailSize::Small)
            .await
            .expect("thumbnail before change");

        std::fs::write(&target, png_bytes(64, 32))
            .expect("overwrite fixture with different content");
        let after = service
            .thumbnail(&providers(), &location, ThumbnailSize::Small)
            .await
            .expect("thumbnail after change");

        let decoded_after = image::load_from_memory(&after.bytes).expect("decode after");
        assert_ne!(before.bytes, after.bytes);
        assert_eq!(decoded_after.width(), decoded_after.height() * 2);
    }

    #[tokio::test]
    async fn a_folder_with_a_cover_file_is_thumbnailed_from_that_cover() {
        let dir = tempfile::tempdir().expect("temp dir");
        let album = dir.path().join("Abattoir Blues");
        std::fs::create_dir(&album).expect("album dir");
        std::fs::write(album.join("01 track.txt"), b"not art").expect("write decoy");
        std::fs::write(album.join("Cover.PNG"), png_bytes(80, 40)).expect("write cover");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&album), ThumbnailSize::Small)
            .await
            .expect("folder thumbnail must succeed");

        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert_eq!(decoded.width(), decoded.height() * 2);
    }

    /// A silent 8 kHz WAV with `picture` embedded in an ID3v2 tag.
    fn wav_with_cover(picture: Vec<u8>) -> Vec<u8> {
        use lofty::config::WriteOptions;
        use lofty::picture::{MimeType, Picture, PictureType};
        use lofty::tag::{Tag, TagExt, TagType};

        let samples = 8000_u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for field in [16_u32, 0x0001_0001, 8000, 8000, 0x0008_0001] {
            wav.extend_from_slice(&field.to_le_bytes());
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&samples.to_le_bytes());
        wav.extend(std::iter::repeat_n(128_u8, samples as usize));

        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("fixture.wav");
        std::fs::write(&path, wav).expect("write wav");
        let mut tag = Tag::new(TagType::Id3v2);
        tag.push_picture(
            Picture::unchecked(picture)
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Png)
                .build(),
        );
        tag.save_to_path(&path, WriteOptions::default())
            .expect("write tag");
        std::fs::read(&path).expect("read back fixture")
    }

    #[tokio::test]
    async fn a_folder_without_a_cover_file_uses_the_picture_embedded_in_its_first_track() {
        let dir = tempfile::tempdir().expect("temp dir");
        let album = dir.path().join("Murder Ballads");
        std::fs::create_dir(&album).expect("album dir");
        std::fs::write(
            album.join("01 intro.wav"),
            wav_with_cover(png_bytes(80, 40)),
        )
        .expect("write track");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&album), ThumbnailSize::Small)
            .await
            .expect("embedded cover must be used");

        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert_eq!(decoded.width(), decoded.height() * 2);
    }

    #[tokio::test]
    async fn an_audio_file_is_thumbnailed_from_its_embedded_picture() {
        let dir = tempfile::tempdir().expect("temp dir");
        let track = dir.path().join("song.wav");
        std::fs::write(&track, wav_with_cover(png_bytes(40, 80))).expect("write track");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&track), ThumbnailSize::Small)
            .await
            .expect("embedded cover must be used");

        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert_eq!(decoded.height(), decoded.width() * 2);
    }

    #[tokio::test]
    async fn a_picture_larger_than_the_first_read_is_still_found() {
        // Incompressible noise keeps the PNG above the 4 MiB first read.
        let mut noise = image::RgbImage::new(1300, 1100);
        let mut state = 0x2545_f491_u32;
        for pixel in noise.pixels_mut() {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *pixel = image::Rgb([state as u8, (state >> 8) as u8, (state >> 16) as u8]);
        }
        let mut picture = Vec::new();
        image::DynamicImage::ImageRgb8(noise)
            .write_to(&mut Cursor::new(&mut picture), image::ImageFormat::Png)
            .expect("encode noise png");
        assert!(picture.len() > AUDIO_PICTURE_PROBE_BYTES);

        let dir = tempfile::tempdir().expect("temp dir");
        let track = dir.path().join("big.wav");
        std::fs::write(&track, wav_with_cover(picture)).expect("write track");

        let service = ThumbnailService::new(dir.path().join("cache"));
        service
            .thumbnail(&providers(), &location_for(&track), ThumbnailSize::Small)
            .await
            .expect("a large embedded picture must be found");
    }

    #[tokio::test]
    async fn a_cover_file_beats_a_picture_embedded_in_a_track_and_names_follow_priority() {
        let dir = tempfile::tempdir().expect("temp dir");
        let album = dir.path().join("Album");
        std::fs::create_dir(&album).expect("album dir");
        std::fs::write(album.join("01.wav"), wav_with_cover(png_bytes(20, 20))).expect("track");
        std::fs::write(album.join("folder.png"), png_bytes(20, 40)).expect("folder art");
        std::fs::write(album.join("cover.png"), png_bytes(80, 20)).expect("cover art");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&album), ThumbnailSize::Small)
            .await
            .expect("folder thumbnail");

        // cover.png (4:1) wins over folder.png (1:2) and the embedded square picture.
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert_eq!(decoded.width(), decoded.height() * 4);
    }

    #[tokio::test]
    async fn a_folder_without_any_artwork_is_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("Documents");
        std::fs::create_dir(&folder).expect("dir");
        std::fs::write(folder.join("notes.txt"), b"text").expect("file");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let error = service
            .thumbnail(&providers(), &location_for(&folder), ThumbnailSize::Small)
            .await
            .unwrap_err();

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlinked_folder_is_not_probed_for_a_cover() {
        let dir = tempfile::tempdir().expect("temp dir");
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("real dir");
        std::fs::write(real.join("cover.png"), png_bytes(40, 40)).expect("cover");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let error = service
            .thumbnail(&providers(), &location_for(&link), ThumbnailSize::Small)
            .await
            .unwrap_err();

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[tokio::test]
    async fn rejects_an_unsupported_extension_as_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("notes.txt");
        std::fs::write(&target, b"just text").expect("write fixture");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let error = service
            .thumbnail(&providers(), &location_for(&target), ThumbnailSize::Small)
            .await
            .unwrap_err();

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[tokio::test]
    async fn generates_a_thumbnail_from_the_first_page_of_a_cbz_archive() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive_path = dir.path().join("issue.cbz");
        let file = std::fs::File::create(&archive_path).expect("create cbz");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("002.png", zip::write::SimpleFileOptions::default())
            .expect("start second page");
        std::io::Write::write_all(&mut writer, &png_bytes(40, 40)).expect("write second page");
        writer
            .start_file("001.png", zip::write::SimpleFileOptions::default())
            .expect("start first page");
        std::io::Write::write_all(&mut writer, &png_bytes(80, 40)).expect("write first page");
        writer.finish().expect("finish cbz");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(
                &providers(),
                &location_for(&archive_path),
                ThumbnailSize::Small,
            )
            .await
            .expect("cbz thumbnail must succeed");

        // "001.png" sorts before "002.png": the thumbnail must be the 80x40
        // first page, not the 40x40 second page.
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert_eq!(decoded.width(), decoded.height() * 2);
    }

    #[tokio::test]
    async fn rejects_an_empty_cbz_archive_as_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let archive_path = dir.path().join("empty.cbz");
        let file = std::fs::File::create(&archive_path).expect("create cbz");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("readme.txt", zip::write::SimpleFileOptions::default())
            .expect("start non-image entry");
        std::io::Write::write_all(&mut writer, b"no pages here").expect("write entry");
        writer.finish().expect("finish cbz");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let error = service
            .thumbnail(
                &providers(),
                &location_for(&archive_path),
                ThumbnailSize::Small,
            )
            .await
            .unwrap_err();

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[tokio::test]
    async fn rejects_a_source_file_over_the_size_budget_without_decoding_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("huge.png");
        std::fs::File::create(&target)
            .expect("create fixture")
            .set_len(MAX_SOURCE_BYTES + 1)
            .expect("size fixture");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let error = service
            .thumbnail(&providers(), &location_for(&target), ThumbnailSize::Small)
            .await
            .unwrap_err();

        assert_eq!(error, ApplicationError::NotFound);
    }

    /// Strips a NAL unit's Annex-B start code (openh264's own `nal_unit()`
    /// includes it as part of the slice) - same helper `fm-metadata`'s own
    /// video fixture builder uses.
    fn strip_start_code(nal: &[u8]) -> &[u8] {
        if let Some(stripped) = nal.strip_prefix(&[0, 0, 0, 1]) {
            stripped
        } else if let Some(stripped) = nal.strip_prefix(&[0, 0, 1]) {
            stripped
        } else {
            nal
        }
    }

    /// Encodes and muxes a minimal-but-real single-keyframe MP4, exercising
    /// the extension-dispatch path (not just `fm-metadata`'s decoder
    /// directly) end to end.
    fn mp4_bytes(width: u32, height: u32) -> Vec<u8> {
        use openh264::encoder::Encoder;
        use openh264::formats::YUVBuffer;

        let mut encoder = Encoder::new().expect("create encoder");
        let yuv = YUVBuffer::new(width as usize, height as usize);
        let bitstream = encoder.encode(&yuv).expect("encode frame");

        let mut sps = Vec::new();
        let mut pps = Vec::new();
        let mut slice_nals: Vec<Vec<u8>> = Vec::new();
        for layer_index in 0..bitstream.num_layers() {
            let layer = bitstream.layer(layer_index).expect("layer must exist");
            for nal_index in 0..layer.nal_count() {
                let nal = strip_start_code(layer.nal_unit(nal_index).expect("nal must exist"));
                match nal[0] & 0x1F {
                    7 => sps = nal.to_vec(),
                    8 => pps = nal.to_vec(),
                    _ => slice_nals.push(nal.to_vec()),
                }
            }
        }

        let avc_config = mp4::AvcConfig {
            width: width as u16,
            height: height as u16,
            seq_param_set: sps,
            pic_param_set: pps,
        };
        let mut sample_bytes = Vec::new();
        for nal in &slice_nals {
            sample_bytes.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            sample_bytes.extend_from_slice(nal);
        }

        let mut out = Cursor::new(Vec::new());
        let config = mp4::Mp4Config {
            major_brand: str::parse("isom").expect("valid brand"),
            minor_version: 0,
            compatible_brands: vec![str::parse("isom").expect("valid brand")],
            timescale: 1000,
        };
        let mut writer = mp4::Mp4Writer::write_start(&mut out, &config).expect("write start");
        writer
            .add_track(&mp4::TrackConfig::from(avc_config))
            .expect("add track");
        writer
            .write_sample(
                1,
                &mp4::Mp4Sample {
                    start_time: 0,
                    duration: 1000,
                    rendering_offset: 0,
                    is_sync: true,
                    bytes: sample_bytes.into(),
                },
            )
            .expect("write sample");
        writer.write_end().expect("write end");
        out.into_inner()
    }

    #[tokio::test]
    async fn generates_a_thumbnail_for_an_mp4_video_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("clip.mp4");
        std::fs::write(&target, mp4_bytes(64, 64)).expect("write fixture");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&target), ThumbnailSize::Small)
            .await
            .expect("thumbnail must succeed");

        assert_eq!(thumbnail.content_type, "image/jpeg");
        image::load_from_memory(&thumbnail.bytes).expect("decode result");
    }

    /// Builds a minimal single-page PDF with one DCTDecode (JPEG) embedded
    /// image, using `lopdf`'s own writer - same technique `fm-metadata`'s
    /// own PDF fixture builder uses.
    fn pdf_bytes(width: u32, height: u32) -> Vec<u8> {
        use lopdf::{Dictionary, Document, Object, Stream, dictionary};

        let jpeg = png_bytes(width, height);
        let jpeg = {
            let decoded = image::load_from_memory(&jpeg).expect("decode fixture png");
            let mut out = Vec::new();
            decoded
                .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
                .expect("encode fixture jpeg");
            out
        };

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let image_dict: Dictionary = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        };
        let image_id = doc.add_object(Object::Stream(Stream::new(image_dict, jpeg)));
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im1" => image_id },
        });
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut buffer = Vec::new();
        doc.save_to(&mut buffer).expect("save fixture pdf");
        buffer
    }

    #[tokio::test]
    async fn generates_a_thumbnail_for_a_pdf_with_an_embedded_image() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("scan.pdf");
        std::fs::write(&target, pdf_bytes(120, 60)).expect("write fixture");

        let service = ThumbnailService::new(dir.path().join("cache"));
        let thumbnail = service
            .thumbnail(&providers(), &location_for(&target), ThumbnailSize::Medium)
            .await
            .expect("thumbnail must succeed");

        assert_eq!(thumbnail.content_type, "image/jpeg");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert_eq!(decoded.width(), decoded.height() * 2);
    }
}
