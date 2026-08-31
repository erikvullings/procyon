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
    is_supported_image_extension, is_supported_pdf_extension, is_supported_video_extension,
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
        if let Some(cached) = self.cache.get(&key) {
            return Ok(GeneratedThumbnail {
                bytes: cached,
                content_type: "image/jpeg",
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
    let extension = extension_of(&name)
        .ok_or_else(|| map_thumbnail_error(ThumbnailError::UnsupportedFormat))?;

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
    } else {
        Err(map_thumbnail_error(ThumbnailError::UnsupportedFormat))
    }
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
        let image = image::DynamicImage::new_rgba8(width, height);
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
