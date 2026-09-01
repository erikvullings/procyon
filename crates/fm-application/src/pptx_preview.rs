//! Bounded, provider-neutral PPTX conversion for the F3 content viewer.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;

use fm_domain::{EntryId, EntryKind, Location};
use fm_transport_dto::{
    OpenPptxPreviewRequestDto, OpenPptxPreviewResponseDto, PptxPreviewResourceDto,
    PptxPreviewSessionRequestDto, PptxPreviewSlideDto, ReadPptxPreviewResourceRequestDto,
    ReadPptxPreviewResourceResponseDto,
};
use fm_vfs::{EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry};
use pptx_to_md::{
    ImageHandlingMode, MarkdownOptions, ParserConfig, PptxContainer, ReadingOrder,
    SlideBlockContent, TextRole,
};
use quick_xml::events::{BytesStart, Event};
use tokio::io::AsyncReadExt;
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ApplicationError;
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
const MAX_PPTX_SESSIONS: usize = 8;

struct RetainedResource {
    media_type: String,
    data: Vec<u8>,
}

struct Session {
    provider: Arc<dyn FileSystemProvider>,
    entry: EntryRef,
    revision: String,
    cancellation: CancellationToken,
    resources: HashMap<Uuid, RetainedResource>,
    _slot: OwnedSemaphorePermit,
}

trait PptxConverter: Send + Sync {
    fn parse(
        &self,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ParsedPptx, ApplicationError>;
}

struct NativePptxConverter;

impl PptxConverter for NativePptxConverter {
    fn parse(
        &self,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ParsedPptx, ApplicationError> {
        parse_pptx(bytes, cancellation)
    }
}

/// Shared PPTX preview session owner used by both HTTP and Tauri adapters.
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
                    "at most {MAX_PPTX_SESSIONS} PPTX previews may be open; close another preview first"
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
                "PPTX preview requires a regular file".to_owned(),
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
        let parse_cancellation = cancellation.clone();
        let converter = Arc::clone(&self.converter);
        let parsed =
            tokio::task::spawn_blocking(move || converter.parse(&bytes, &parse_cancellation))
                .await
                .map_err(|_| ApplicationError::Internal)??;
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }

        let session_id = Uuid::new_v4();
        let slides = parsed
            .slides
            .into_iter()
            .enumerate()
            .map(|(index, slide)| PptxPreviewSlideDto {
                index: index as u32,
                title: slide.title,
                markdown: slide.markdown,
            })
            .collect();
        let mut resources = HashMap::new();
        let mut resource_dtos = Vec::with_capacity(parsed.resources.len());
        for resource in parsed.resources {
            let resource_id = Uuid::new_v5(&session_id, resource.source.as_bytes());
            resource_dtos.push(PptxPreviewResourceDto {
                resource_id,
                source: resource.source,
                media_type: resource.media_type.clone(),
                byte_length: resource.data.len() as u64,
            });
            resources.insert(
                resource_id,
                RetainedResource {
                    media_type: resource.media_type,
                    data: resource.data,
                },
            );
        }
        self.sessions.write().await.insert(
            session_id,
            Arc::new(Session {
                provider,
                entry,
                revision: revision.clone(),
                cancellation,
                resources,
                _slot: slot,
            }),
        );
        Ok(OpenPptxPreviewResponseDto {
            session_id,
            source_revision: revision,
            source_bytes,
            slides,
            resources: resource_dtos,
            omitted_features: omitted_features(),
        })
    }

    pub(crate) async fn read_resource(
        &self,
        request: ReadPptxPreviewResourceRequestDto,
    ) -> Result<ReadPptxPreviewResourceResponseDto, ApplicationError> {
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        let resource = session
            .resources
            .get(&request.resource_id)
            .ok_or(ApplicationError::NotFound)?;
        Ok(ReadPptxPreviewResourceResponseDto {
            data: resource.data.clone(),
            media_type: resource.media_type.clone(),
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

fn omitted_features() -> Vec<String> {
    [
        "themes and precise geometry",
        "fonts",
        "transitions and animations",
        "SmartArt and charts",
        "embedded objects",
        "audio and video",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedPptxSlide {
    pub(crate) title: Option<String>,
    pub(crate) markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedPptxResource {
    pub(crate) source: String,
    pub(crate) media_type: String,
    pub(crate) data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedPptx {
    pub(crate) slides: Vec<ParsedPptxSlide>,
    pub(crate) resources: Vec<ParsedPptxResource>,
}

fn parse_pptx(
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<ParsedPptx, ApplicationError> {
    if cancellation.is_cancelled() {
        return Err(ApplicationError::OperationCancelled);
    }
    preflight_package(bytes, cancellation)?;
    let order = presentation_order(bytes)?;
    let mut source = tempfile::NamedTempFile::new().map_err(|_| ApplicationError::Internal)?;
    source
        .write_all(bytes)
        .map_err(|_| ApplicationError::Internal)?;
    let config = ParserConfig::builder()
        .extract_images(true)
        .image_handling_mode(ImageHandlingMode::Manually)
        .include_slide_number_as_comment(false)
        .include_speaker_notes(true)
        .include_presentation_metadata(false)
        .build();
    let mut container = PptxContainer::open(source.path(), config).map_err(invalid_pptx)?;
    let mut slides = container.parse_all().map_err(invalid_pptx)?;
    slides.sort_by_key(|slide| {
        order
            .iter()
            .position(|path| path == &slide.rel_path)
            .unwrap_or(usize::MAX)
    });
    let options = MarkdownOptions {
        reading_order: ReadingOrder::Source,
        include_slide_number_as_comment: false,
        include_speaker_notes: true,
        include_comments: false,
        render_unsupported_comments: true,
    };
    let mut resources_by_source = HashMap::<String, ParsedPptxResource>::new();
    let mut text_bytes = 0_usize;
    let slides = slides
        .into_iter()
        .map(|slide| {
            let title = slide.blocks.iter().find_map(|block| match &block.content {
                SlideBlockContent::Text(text) if text.role == TextRole::Title => text
                    .paragraphs
                    .iter()
                    .map(|paragraph| paragraph.text())
                    .map(|value| value.trim().to_owned())
                    .find(|value| !value.is_empty()),
                _ => None,
            });
            let mut markdown = slide.to_markdown(&options).map_err(invalid_pptx)?;
            for image in &slide.images {
                let Some(data) = slide.image_data.get(&image.id) else {
                    markdown.push_str("\n\n_[Embedded image omitted: resource unavailable]_\n");
                    continue;
                };
                let Some(media_type) = image_media_type(&image.target) else {
                    markdown.push_str("\n\n_[Embedded image omitted: unsupported format]_\n");
                    continue;
                };
                markdown.push_str(&format!(
                    "\n\n![Embedded image](pptx-resource:{})\n",
                    image.target
                ));
                resources_by_source
                    .entry(image.target.clone())
                    .or_insert_with(|| ParsedPptxResource {
                        source: image.target.clone(),
                        media_type: media_type.to_owned(),
                        data: data.clone(),
                    });
            }
            text_bytes = text_bytes
                .checked_add(markdown.len())
                .ok_or_else(|| budget_error("text", MAX_PPTX_TEXT_BYTES as u64))?;
            if text_bytes > MAX_PPTX_TEXT_BYTES {
                return Err(budget_error("text", MAX_PPTX_TEXT_BYTES as u64));
            }
            Ok(ParsedPptxSlide { title, markdown })
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    let mut resources = resources_by_source.into_values().collect::<Vec<_>>();
    resources.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(ParsedPptx { slides, resources })
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
            "PPTX content preview exceeds the ZIP entry-count budget ({MAX_PPTX_ZIP_ENTRIES}); open it in an external application"
        )));
    }
    let mut expanded_bytes = 0_u64;
    let mut slide_count = 0_usize;
    let mut media_count = 0_usize;
    let mut media_bytes = 0_u64;
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
                    "PPTX content preview exceeds the slide-count budget ({MAX_PPTX_SLIDES}); open it in an external application"
                )));
            }
        }
        if image_media_type(&name).is_some() && !entry.is_dir() {
            media_count += 1;
            if media_count > MAX_PPTX_MEDIA_ITEMS {
                return Err(ApplicationError::InvalidRequest(format!(
                    "PPTX content preview exceeds the media-count budget ({MAX_PPTX_MEDIA_ITEMS}); open it in an external application"
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
        }
        let lowercase = name.to_ascii_lowercase();
        if lowercase.ends_with(".xml") || lowercase.ends_with(".rels") {
            validate_xml_depth(&mut entry, cancellation)?;
        }
    }
    Ok(())
}

fn validate_xml_depth(
    source: &mut impl Read,
    cancellation: &CancellationToken,
) -> Result<(), ApplicationError> {
    let mut reader = quick_xml::Reader::from_reader(std::io::BufReader::new(source));
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    loop {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(_)) => {
                depth += 1;
                if depth > MAX_PPTX_XML_DEPTH {
                    return Err(ApplicationError::InvalidRequest(format!(
                        "PPTX content preview exceeds the XML depth budget ({MAX_PPTX_XML_DEPTH}); open it in an external application"
                    )));
                }
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::DocType(_)) => {
                return Err(invalid_pptx("DOCTYPE declarations are not allowed"));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(invalid_pptx(error)),
        }
        buffer.clear();
    }
    Ok(())
}

fn budget_error(resource: &str, limit: u64) -> ApplicationError {
    ApplicationError::InvalidRequest(format!(
        "PPTX content preview exceeds the {resource} budget ({limit} bytes); open it in an external application"
    ))
}

fn image_media_type(source: &str) -> Option<&'static str> {
    match source.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn invalid_pptx(error: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::InvalidRequest(format!(
        "PPTX content preview is unavailable because the package is malformed: {error}"
    ))
}

fn presentation_order(bytes: &[u8]) -> Result<Vec<String>, ApplicationError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(invalid_pptx)?;
    let presentation = read_archive_entry(&mut archive, "ppt/presentation.xml")?;
    let relationships = read_archive_entry(&mut archive, "ppt/_rels/presentation.xml.rels")?;
    let ids = attribute_values(&presentation, b"sldId", b"r:id")?;
    let targets = relationship_targets(&relationships)?;
    Ok(ids
        .into_iter()
        .filter_map(|id| targets.get(&id))
        .filter_map(|target| resolve_presentation_target(target))
        .collect())
}

fn resolve_presentation_target(target: &str) -> Option<String> {
    let package_path = target
        .strip_prefix('/')
        .map(str::to_owned)
        .unwrap_or_else(|| format!("ppt/{target}"));
    let mut normalized = Vec::new();
    for component in package_path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                normalized.pop()?;
            }
            value => normalized.push(value),
        }
    }
    Some(normalized.join("/"))
}

fn read_archive_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    path: &str,
) -> Result<Vec<u8>, ApplicationError> {
    let mut entry = archive.by_name(path).map_err(invalid_pptx)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(invalid_pptx)?;
    Ok(bytes)
}

fn attribute_values(
    xml: &[u8],
    element_name: &[u8],
    attribute_name: &[u8],
) -> Result<Vec<String>, ApplicationError> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut values = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == element_name =>
            {
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(invalid_pptx)?;
                    if attribute.key.as_ref() == attribute_name {
                        values.push(
                            String::from_utf8(attribute.value.into_owned())
                                .map_err(invalid_pptx)?,
                        );
                    }
                }
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
    Ok(values)
}

fn relationship_targets(xml: &[u8]) -> Result<HashMap<String, String>, ApplicationError> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut targets = HashMap::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"Relationship" =>
            {
                if let (Some(id), Some(target)) = (
                    attribute_value(&element, b"Id")?,
                    attribute_value(&element, b"Target")?,
                ) {
                    targets.insert(id, target);
                }
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
    Ok(targets)
}

fn attribute_value(
    element: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, ApplicationError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(invalid_pptx)?;
        if attribute.key.local_name().as_ref() == name {
            return String::from_utf8(attribute.value.into_owned())
                .map(Some)
                .map_err(invalid_pptx);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use fm_vfs::ProviderRegistry;
    use fm_vfs_local::LocalFileSystemProvider;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn slide_xml(title: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
 <p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="1" name="Title"/>
 <p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
 <p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{title}</a:t></a:r></a:p>
 </p:txBody></p:sp></p:spTree></p:cSld>
</p:sld>"#
        )
    }

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

    fn image_package(image_target: &str, include_image: bool) -> Vec<u8> {
        let slide = br#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:pic><p:nvPicPr><p:cNvPr id="1" name="Picture"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rImage"/></p:blipFill><p:spPr/></p:pic></p:spTree></p:cSld></p:sld>"#;
        let presentation = br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
        let presentation_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
        let slide_rels = format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="{image_target}"/></Relationships>"#
        );
        let mut entries = vec![
            ("ppt/presentation.xml", presentation.as_slice()),
            (
                "ppt/_rels/presentation.xml.rels",
                presentation_rels.as_slice(),
            ),
            ("ppt/slides/slide1.xml", slide.as_slice()),
            ("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
        ];
        if include_image {
            entries.push(("ppt/media/image1.png", b"\x89PNG\r\n\x1a\nfixture"));
        }
        package(&entries)
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

    #[test]
    fn preserves_package_declared_slide_order_instead_of_filename_order() {
        let first = slide_xml("Presented first");
        let second = slide_xml("Presented second");
        let presentation = br#"<?xml version="1.0" encoding="UTF-8"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
 <p:sldIdLst><p:sldId id="257" r:id="rId2"/><p:sldId id="256" r:id="rId1"/></p:sldIdLst>
</p:presentation>"#;
        let relationships = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
 <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="/ppt/slides/slide2.xml"/>
</Relationships>"#;
        let bytes = package(&[
            ("ppt/slides/slide1.xml", second.as_bytes()),
            ("ppt/slides/slide2.xml", first.as_bytes()),
            ("ppt/presentation.xml", presentation),
            ("ppt/_rels/presentation.xml.rels", relationships),
        ]);

        let parsed =
            parse_pptx(&bytes, &CancellationToken::new()).expect("representative PPTX must parse");

        assert_eq!(
            parsed
                .slides
                .iter()
                .map(|slide| slide.title.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("Presented first"), Some("Presented second")]
        );
    }

    #[test]
    fn preserves_titles_lists_tables_links_notes_images_and_unsupported_placeholders() {
        let slide = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
 <p:cSld><p:spTree>
  <p:sp><p:nvSpPr><p:cNvPr id="1" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/>
   <p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Quarterly report</a:t></a:r></a:p></p:txBody></p:sp>
  <p:sp><p:nvSpPr><p:cNvPr id="2" name="Body"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/>
   <p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr lvl="0"><a:buChar char="&#8226;"/></a:pPr><a:r><a:t>First item</a:t></a:r></a:p>
   <a:p><a:r><a:rPr><a:hlinkClick r:id="rLink"/></a:rPr><a:t>Example</a:t></a:r></a:p></p:txBody></p:sp>
  <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="3" name="Table"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/>
   <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr/><a:tblGrid><a:gridCol w="1"/><a:gridCol w="1"/></a:tblGrid>
   <a:tr h="1"><a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Name</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>
   <a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Value</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>
  <p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture" descr="Quarter chart"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>
   <p:blipFill><a:blip r:embed="rImage"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr/></p:pic>
  <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="5" name="Chart"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/>
   <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"/></a:graphic></p:graphicFrame>
 </p:spTree></p:cSld>
</p:sld>"#;
        let presentation = br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#;
        let presentation_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#;
        let slide_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Id="rLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
 <Relationship Id="rImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>
 <Relationship Id="rNotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide1.xml"/>
</Relationships>"#;
        let notes = br#"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="1" name="Notes"/><p:cNvSpPr/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Discuss retention</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#;
        let bytes = package(&[
            ("ppt/presentation.xml", presentation),
            ("ppt/_rels/presentation.xml.rels", presentation_rels),
            ("ppt/slides/slide1.xml", slide),
            ("ppt/slides/_rels/slide1.xml.rels", slide_rels),
            ("ppt/notesSlides/notesSlide1.xml", notes),
            ("ppt/media/image1.png", b"\x89PNG\r\n\x1a\nfixture"),
        ]);

        let parsed =
            parse_pptx(&bytes, &CancellationToken::new()).expect("representative PPTX must parse");
        let slide = &parsed.slides[0];

        assert_eq!(slide.title.as_deref(), Some("Quarterly report"));
        assert!(slide.markdown.contains("First item"), "{}", slide.markdown);
        assert!(slide.markdown.contains("| Name"), "{}", slide.markdown);
        assert!(
            slide.markdown.contains("[Example](https://example.com)"),
            "{}",
            slide.markdown
        );
        assert!(
            slide.markdown.contains("Discuss retention"),
            "{}",
            slide.markdown
        );
        assert!(
            slide.markdown.contains("Unsupported slide element"),
            "{}",
            slide.markdown
        );
        assert!(slide.markdown.contains("pptx-resource:../media/image1.png"));
        assert_eq!(parsed.resources.len(), 1);
        assert_eq!(parsed.resources[0].media_type, "image/png");
    }

    #[test]
    fn rejects_expanded_zip_and_entry_count_budget_overruns() {
        let oversized = vec![b'x'; (MAX_PPTX_EXPANDED_BYTES + 1) as usize];
        let bytes = package(&[("ppt/presentation.xml", oversized.as_slice())]);
        let expanded_error = parse_pptx(&bytes, &CancellationToken::new())
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
        let count_error = parse_pptx(&bytes, &CancellationToken::new())
            .expect_err("entry-count budget must be enforced");
        assert!(count_error.to_string().contains("entry-count"));
    }

    #[test]
    fn malformed_image_relationships_render_an_explicit_placeholder() {
        let parsed = parse_pptx(
            &image_package("../media/missing.png", false),
            &CancellationToken::new(),
        )
        .expect("missing image relationships should not discard slide text");

        assert!(parsed.resources.is_empty());
        assert!(parsed.slides[0].markdown.contains("resource unavailable"));
    }

    #[test]
    fn honours_cancellation_before_parsing() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = parse_pptx(&image_package("../media/image1.png", true), &cancellation)
            .expect_err("cancelled parsing must stop");
        assert_eq!(error, ApplicationError::OperationCancelled);
    }

    #[tokio::test]
    async fn invalidates_resources_when_the_source_revision_changes() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("briefing.pptx");
        std::fs::write(&path, image_package("../media/image1.png", true))
            .expect("write PPTX fixture");
        let service = service();
        let opened = service
            .open(open_request(&path))
            .await
            .expect("open PPTX preview");
        let resource_id = opened.resources[0].resource_id;

        std::fs::write(&path, b"changed source").expect("replace PPTX fixture");
        let error = service
            .read_resource(ReadPptxPreviewResourceRequestDto {
                session_id: opened.session_id,
                resource_id,
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
        std::fs::write(&path, image_package("../media/image1.png", true))
            .expect("write PPTX fixture");
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
