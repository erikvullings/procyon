//! Bounded, provider-neutral PPTX-to-PDF conversion for the F3 viewer.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use fm_domain::{EntryId, EntryKind, Location};
use fm_pptx_renderer::render_pptx_to_pdf;
use fm_transport_dto::{
    OpenPptxPreviewRequestDto, OpenPptxPreviewResponseDto, PptxPreviewSessionRequestDto,
    ReadFileRangeResponseDto, ReadPptxPreviewPdfRequestDto,
};
use fm_vfs::{EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry};
use quick_xml::events::Event;
use tokio::io::AsyncReadExt;
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ApplicationError;
use crate::content_streaming::MAX_RANGE_LENGTH;
use crate::file_editor::read_stream_error;

pub(crate) const MAX_PPTX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_PPTX_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_PPTX_ZIP_ENTRIES: usize = 4_096;
pub(crate) const MAX_PPTX_XML_DEPTH: usize = 128;
pub(crate) const MAX_PPTX_SLIDES: usize = 1_000;
pub(crate) const MAX_PPTX_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_PPTX_MEDIA_ITEMS: usize = 128;
pub(crate) const MAX_PPTX_MEDIA_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_PPTX_SINGLE_MEDIA_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_PPTX_SINGLE_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_PPTX_TOTAL_IMAGE_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_PPTX_SESSIONS: usize = 8;

struct Session {
    provider: Arc<dyn FileSystemProvider>,
    entry: EntryRef,
    revision: String,
    cancellation: CancellationToken,
    pdf: Vec<u8>,
    _slot: OwnedSemaphorePermit,
}

trait PptxConverter: Send + Sync {
    fn render(&self, bytes: &[u8]) -> Result<Vec<u8>, ApplicationError>;
}

struct NativePptxConverter;

impl PptxConverter for NativePptxConverter {
    fn render(&self, bytes: &[u8]) -> Result<Vec<u8>, ApplicationError> {
        render_pptx_to_pdf(bytes).map_err(|error| {
            ApplicationError::InvalidRequest(format!(
                "PowerPoint preview rendering failed: {error}; open it in an external application"
            ))
        })
    }
}

/// Shared rendered-PDF session owner used by both HTTP and Tauri adapters.
#[derive(Clone)]
pub(crate) struct PptxPreviewService {
    providers: ProviderRegistry,
    sessions: Arc<RwLock<HashMap<Uuid, Arc<Session>>>>,
    session_slots: Arc<Semaphore>,
    converter: Arc<dyn PptxConverter>,
}

impl PptxPreviewService {
    pub(crate) fn new(providers: ProviderRegistry) -> Self {
        Self {
            providers,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_slots: Arc::new(Semaphore::new(MAX_PPTX_SESSIONS)),
            converter: Arc::new(NativePptxConverter),
        }
    }

    pub(crate) async fn open(
        &self,
        request: OpenPptxPreviewRequestDto,
    ) -> Result<OpenPptxPreviewResponseDto, ApplicationError> {
        let slot = self
            .session_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                ApplicationError::InvalidRequest(format!(
                    "at most {MAX_PPTX_SESSIONS} PowerPoint previews may be open; close another preview first"
                ))
            })?;
        let location: Location = request.location.into();
        let provider = self
            .providers
            .resolve(&location)
            .map_err(ApplicationError::from)?;
        provider
            .capabilities_for(&location)
            .map_err(ApplicationError::from)?
            .require(ProviderCapabilities::READ)
            .map_err(ApplicationError::from)?;
        let entry = EntryRef {
            id: EntryId::new(),
            location,
        };
        let cancellation = CancellationToken::new();
        let summary = provider
            .inspect(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        if summary.kind != EntryKind::File {
            return Err(ApplicationError::InvalidRequest(
                "PowerPoint preview requires a regular file".to_owned(),
            ));
        }
        let source_bytes = provider
            .file_size(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        if source_bytes > MAX_PPTX_SOURCE_BYTES {
            return Err(budget_error("source bytes", MAX_PPTX_SOURCE_BYTES));
        }
        let revision = source_revision(&summary, source_bytes);
        let reader = provider
            .open_read(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        let mut bytes = Vec::with_capacity(source_bytes as usize);
        reader
            .take(MAX_PPTX_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(read_stream_error)?;
        if bytes.len() as u64 > MAX_PPTX_SOURCE_BYTES {
            return Err(budget_error("source bytes", MAX_PPTX_SOURCE_BYTES));
        }
        preflight_package(&bytes, &cancellation)?;
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        let converter = Arc::clone(&self.converter);
        let pdf = tokio::task::spawn_blocking(move || converter.render(&bytes))
            .await
            .map_err(|_| ApplicationError::Internal)??;
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }

        let session_id = Uuid::new_v4();
        let pdf_bytes = pdf.len() as u64;
        self.sessions.write().await.insert(
            session_id,
            Arc::new(Session {
                provider,
                entry,
                revision: revision.clone(),
                cancellation,
                pdf,
                _slot: slot,
            }),
        );
        Ok(OpenPptxPreviewResponseDto {
            session_id,
            source_revision: revision,
            source_bytes,
            pdf_bytes,
        })
    }

    pub(crate) async fn read_pdf(
        &self,
        request: ReadPptxPreviewPdfRequestDto,
    ) -> Result<ReadFileRangeResponseDto, ApplicationError> {
        if request.length == 0 || request.length > MAX_RANGE_LENGTH {
            return Err(ApplicationError::InvalidRequest(format!(
                "length must be between 1 and {MAX_RANGE_LENGTH} bytes"
            )));
        }
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        let start = usize::try_from(request.offset)
            .unwrap_or(usize::MAX)
            .min(session.pdf.len());
        let requested_end = request.offset.saturating_add(request.length);
        let end = usize::try_from(requested_end)
            .unwrap_or(usize::MAX)
            .min(session.pdf.len());
        let data = session.pdf[start..end].to_vec();
        Ok(ReadFileRangeResponseDto {
            offset: request.offset,
            length: data.len() as u64,
            eof: end == session.pdf.len(),
            data,
            probably_binary: (request.offset == 0).then_some(true),
        })
    }

    pub(crate) async fn close(
        &self,
        request: PptxPreviewSessionRequestDto,
    ) -> Result<(), ApplicationError> {
        let session = self
            .sessions
            .write()
            .await
            .remove(&request.session_id)
            .ok_or(ApplicationError::NotFound)?;
        session.cancellation.cancel();
        Ok(())
    }

    async fn session(&self, id: Uuid) -> Result<Arc<Session>, ApplicationError> {
        self.sessions
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or(ApplicationError::NotFound)
    }
}

fn source_revision(summary: &fm_domain::EntrySummary, size: u64) -> String {
    format!(
        "{size}:{}:{}",
        summary
            .modified_at
            .and_then(|value| value.timestamp_nanos_opt())
            .unwrap_or_default(),
        summary.metadata_revision
    )
}

async fn validate_revision(session: &Session) -> Result<(), ApplicationError> {
    let summary = session
        .provider
        .inspect(&session.entry, session.cancellation.child_token())
        .await
        .map_err(ApplicationError::from)?;
    let size = session
        .provider
        .file_size(&session.entry, session.cancellation.child_token())
        .await
        .map_err(ApplicationError::from)?;
    let actual = source_revision(&summary, size);
    if actual != session.revision {
        session.cancellation.cancel();
        return Err(ApplicationError::FileRevisionConflict {
            expected_revision: session.revision.clone(),
            actual_revision: actual,
        });
    }
    Ok(())
}

fn preflight_package(
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<(), ApplicationError> {
    if bytes.len() as u64 > MAX_PPTX_SOURCE_BYTES {
        return Err(budget_error("source bytes", MAX_PPTX_SOURCE_BYTES));
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(invalid_pptx)?;
    if archive.len() > MAX_PPTX_ZIP_ENTRIES {
        return Err(ApplicationError::InvalidRequest(format!(
            "PowerPoint preview exceeds the ZIP entry-count budget ({MAX_PPTX_ZIP_ENTRIES}); open it in an external application"
        )));
    }
    let mut expanded_bytes = 0_u64;
    let mut slide_count = 0_usize;
    let mut text_bytes = 0_usize;
    let mut media_count = 0_usize;
    let mut media_bytes = 0_u64;
    let mut image_pixels = 0_u64;
    for index in 0..archive.len() {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        let mut entry = archive.by_index(index).map_err(invalid_pptx)?;
        expanded_bytes = expanded_bytes
            .checked_add(entry.size())
            .ok_or_else(|| budget_error("expanded ZIP", MAX_PPTX_EXPANDED_BYTES))?;
        if expanded_bytes > MAX_PPTX_EXPANDED_BYTES {
            return Err(budget_error("expanded ZIP", MAX_PPTX_EXPANDED_BYTES));
        }
        let name = entry.name().to_owned();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            slide_count += 1;
            if slide_count > MAX_PPTX_SLIDES {
                return Err(ApplicationError::InvalidRequest(format!(
                    "PowerPoint preview exceeds the slide-count budget ({MAX_PPTX_SLIDES}); open it in an external application"
                )));
            }
        }
        if name.starts_with("ppt/media/") && !entry.is_dir() {
            media_count += 1;
            if media_count > MAX_PPTX_MEDIA_ITEMS {
                return Err(ApplicationError::InvalidRequest(format!(
                    "PowerPoint preview exceeds the media-count budget ({MAX_PPTX_MEDIA_ITEMS}); open it in an external application"
                )));
            }
            if entry.size() > MAX_PPTX_SINGLE_MEDIA_BYTES as u64 {
                return Err(budget_error(
                    "per-media bytes",
                    MAX_PPTX_SINGLE_MEDIA_BYTES as u64,
                ));
            }
            media_bytes = media_bytes
                .checked_add(entry.size())
                .ok_or_else(|| budget_error("total media bytes", MAX_PPTX_MEDIA_BYTES as u64))?;
            if media_bytes > MAX_PPTX_MEDIA_BYTES as u64 {
                return Err(budget_error(
                    "total media bytes",
                    MAX_PPTX_MEDIA_BYTES as u64,
                ));
            }
            if let Some(format) = raster_image_format(&name) {
                let mut data = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut data).map_err(invalid_pptx)?;
                let (width, height) =
                    image::ImageReader::with_format(std::io::Cursor::new(data), format)
                        .into_dimensions()
                        .map_err(invalid_pptx)?;
                let pixels = u64::from(width) * u64::from(height);
                if pixels > MAX_PPTX_SINGLE_IMAGE_PIXELS {
                    return Err(pixel_budget_error(
                        "per-image decoded pixels",
                        MAX_PPTX_SINGLE_IMAGE_PIXELS,
                    ));
                }
                image_pixels = image_pixels.checked_add(pixels).ok_or_else(|| {
                    pixel_budget_error("total decoded image pixels", MAX_PPTX_TOTAL_IMAGE_PIXELS)
                })?;
                if image_pixels > MAX_PPTX_TOTAL_IMAGE_PIXELS {
                    return Err(pixel_budget_error(
                        "total decoded image pixels",
                        MAX_PPTX_TOTAL_IMAGE_PIXELS,
                    ));
                }
            }
        }
        let lowercase = name.to_ascii_lowercase();
        if lowercase.ends_with(".xml") || lowercase.ends_with(".rels") {
            text_bytes = text_bytes
                .checked_add(validate_xml(&mut entry, cancellation)?)
                .ok_or_else(|| budget_error("text", MAX_PPTX_TEXT_BYTES as u64))?;
            if text_bytes > MAX_PPTX_TEXT_BYTES {
                return Err(budget_error("text", MAX_PPTX_TEXT_BYTES as u64));
            }
        }
    }
    Ok(())
}

fn raster_image_format(name: &str) -> Option<image::ImageFormat> {
    let extension = std::path::Path::new(name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some(image::ImageFormat::Png),
        "jpg" | "jpeg" => Some(image::ImageFormat::Jpeg),
        "gif" => Some(image::ImageFormat::Gif),
        "webp" => Some(image::ImageFormat::WebP),
        _ => None,
    }
}

fn validate_xml(
    source: &mut impl Read,
    cancellation: &CancellationToken,
) -> Result<usize, ApplicationError> {
    let mut reader = quick_xml::Reader::from_reader(std::io::BufReader::new(source));
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut text_bytes = 0_usize;
    loop {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(_)) => {
                depth += 1;
                if depth > MAX_PPTX_XML_DEPTH {
                    return Err(ApplicationError::InvalidRequest(format!(
                        "PowerPoint preview exceeds the XML depth budget ({MAX_PPTX_XML_DEPTH}); open it in an external application"
                    )));
                }
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Text(text)) => {
                text_bytes = text_bytes
                    .checked_add(text.len())
                    .ok_or_else(|| budget_error("text", MAX_PPTX_TEXT_BYTES as u64))?;
            }
            Ok(Event::DocType(_)) => {
                return Err(invalid_pptx("DOCTYPE declarations are not allowed"));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(invalid_pptx(error)),
        }
        buffer.clear();
    }
    Ok(text_bytes)
}

fn budget_error(resource: &str, limit: u64) -> ApplicationError {
    ApplicationError::InvalidRequest(format!(
        "PowerPoint preview exceeds the {resource} budget ({limit} bytes); open it in an external application"
    ))
}

fn pixel_budget_error(resource: &str, limit: u64) -> ApplicationError {
    ApplicationError::InvalidRequest(format!(
        "PowerPoint preview exceeds the {resource} budget ({limit} pixels); open it in an external application"
    ))
}

fn invalid_pptx(error: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::InvalidRequest(format!(
        "PowerPoint preview is unavailable because the package is malformed: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use fm_vfs::ProviderRegistry;
    use fm_vfs_local::LocalFileSystemProvider;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut bytes);
            for (name, data) in entries {
                archive
                    .start_file(
                        *name,
                        SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Deflated),
                    )
                    .expect("start fixture entry");
                archive.write_all(data).expect("write fixture entry");
            }
            archive.finish().expect("finish fixture package");
        }
        bytes.into_inner()
    }

    fn presentation_package() -> Vec<u8> {
        package(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#,
            ),
            (
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="10000000" cy="1000000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="3200"/><a:t>Hello PPTX</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            ),
        ])
    }

    fn service() -> PptxPreviewService {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider::new()));
        PptxPreviewService::new(providers)
    }

    fn open_request(path: &std::path::Path) -> OpenPptxPreviewRequestDto {
        OpenPptxPreviewRequestDto {
            location: Location::from_native_path(path)
                .expect("fixture path must be a location")
                .into(),
        }
    }

    #[tokio::test]
    async fn opens_a_rendered_pdf_and_reads_it_in_bounded_ranges() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("briefing.pptx");
        std::fs::write(&path, presentation_package()).expect("write PPTX fixture");
        let service = service();

        let opened = service
            .open(open_request(&path))
            .await
            .expect("open rendered preview");
        let range = service
            .read_pdf(ReadPptxPreviewPdfRequestDto {
                session_id: opened.session_id,
                offset: 0,
                length: 16,
            })
            .await
            .expect("read rendered PDF");

        assert!(opened.pdf_bytes > 16);
        assert!(range.data.starts_with(b"%PDF-"));
        assert!(!range.eof);
    }

    #[test]
    fn rejects_expanded_zip_and_entry_count_budget_overruns() {
        let oversized = vec![b'x'; (MAX_PPTX_EXPANDED_BYTES + 1) as usize];
        let bytes = package(&[("ppt/presentation.xml", oversized.as_slice())]);
        let expanded_error = preflight_package(&bytes, &CancellationToken::new())
            .expect_err("expanded package budget must be enforced");
        assert!(expanded_error.to_string().contains("expanded ZIP"));

        let names = (0..=MAX_PPTX_ZIP_ENTRIES)
            .map(|index| format!("custom/item-{index}.bin"))
            .collect::<Vec<_>>();
        let entries = names
            .iter()
            .map(|name| (name.as_str(), b"x".as_slice()))
            .collect::<Vec<_>>();
        let bytes = package(&entries);
        let count_error = preflight_package(&bytes, &CancellationToken::new())
            .expect_err("entry-count budget must be enforced");
        assert!(count_error.to_string().contains("entry-count"));
    }

    #[test]
    fn rejects_raster_images_with_an_excessive_decoded_size() {
        let mut gif = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff\
            \x21\xf9\x04\x01\0\0\0\0\x2c\0\0\0\0\x01\0\x01\0\0\
            \x02\x02\x44\x01\0\x3b"
            .to_vec();
        gif[6..10].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        let bytes = package(&[("ppt/media/image1.gif", &gif)]);

        let error = preflight_package(&bytes, &CancellationToken::new())
            .expect_err("decoded image pixel budget must be enforced");

        assert!(error.to_string().contains("decoded pixels"), "{error}");
    }

    #[test]
    fn honours_cancellation_before_rendering() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = preflight_package(&presentation_package(), &cancellation)
            .expect_err("cancelled conversion must stop");
        assert_eq!(error, ApplicationError::OperationCancelled);
    }

    #[tokio::test]
    async fn invalidates_pdf_ranges_when_the_source_revision_changes() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("briefing.pptx");
        std::fs::write(&path, presentation_package()).expect("write PPTX fixture");
        let service = service();
        let opened = service
            .open(open_request(&path))
            .await
            .expect("open PPTX preview");

        std::fs::write(&path, b"changed source").expect("replace PPTX fixture");
        let error = service
            .read_pdf(ReadPptxPreviewPdfRequestDto {
                session_id: opened.session_id,
                offset: 0,
                length: 16,
            })
            .await
            .expect_err("changed source must invalidate the session");

        assert!(matches!(
            error,
            ApplicationError::FileRevisionConflict { .. }
        ));
    }

    #[tokio::test]
    async fn close_cancels_and_releases_the_session() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("briefing.pptx");
        std::fs::write(&path, presentation_package()).expect("write PPTX fixture");
        let service = service();
        let opened = service
            .open(open_request(&path))
            .await
            .expect("open PPTX preview");
        service
            .close(PptxPreviewSessionRequestDto {
                session_id: opened.session_id,
            })
            .await
            .expect("close PPTX preview");

        let error = service
            .close(PptxPreviewSessionRequestDto {
                session_id: opened.session_id,
            })
            .await
            .expect_err("closed session must be gone");
        assert_eq!(error, ApplicationError::NotFound);
    }
}
