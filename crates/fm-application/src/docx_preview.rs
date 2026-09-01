//! Bounded, provider-neutral DOCX conversion for the F3 content viewer.

use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::sync::Arc;

use fm_domain::{EntryId, EntryKind, Location};
use fm_transport_dto::{
    DocxPreviewResourceDto, DocxPreviewSessionRequestDto, OpenDocxPreviewRequestDto,
    OpenDocxPreviewResponseDto, ReadDocxPreviewResourceRequestDto,
    ReadDocxPreviewResourceResponseDto,
};
use fm_vfs::{EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry};
use quick_xml::events::Event;
use tokio::io::AsyncReadExt;
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ApplicationError;
use crate::file_editor::read_stream_error;

pub(crate) const MAX_DOCX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_DOCX_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_DOCX_ZIP_ENTRIES: usize = 4_096;
pub(crate) const MAX_DOCX_XML_DEPTH: usize = 128;
pub(crate) const MAX_DOCX_IMAGES: usize = 128;
pub(crate) const MAX_DOCX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_DOCX_TOTAL_IMAGE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_DOCX_RENDERED_BYTES: usize = 4 * 1024 * 1024;
const MAX_DOCX_SESSIONS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedDocxResource {
    pub(crate) source: String,
    pub(crate) media_type: String,
    pub(crate) data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedDocx {
    pub(crate) html: String,
    pub(crate) resources: Vec<ParsedDocxResource>,
}

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

trait DocxConverter: Send + Sync {
    fn parse(
        &self,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ParsedDocx, ApplicationError>;
}

struct FerrodocConverter;

impl DocxConverter for FerrodocConverter {
    fn parse(
        &self,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ParsedDocx, ApplicationError> {
        parse_docx(bytes, cancellation)
    }
}

/// Shared DOCX preview session owner used by both HTTP and Tauri adapters.
#[derive(Clone)]
pub(crate) struct DocxPreviewService {
    providers: ProviderRegistry,
    sessions: Arc<RwLock<HashMap<Uuid, Arc<Session>>>>,
    session_slots: Arc<Semaphore>,
    converter: Arc<dyn DocxConverter>,
}

impl DocxPreviewService {
    pub(crate) fn new(providers: ProviderRegistry) -> Self {
        Self {
            providers,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_slots: Arc::new(Semaphore::new(MAX_DOCX_SESSIONS)),
            converter: Arc::new(FerrodocConverter),
        }
    }

    pub(crate) async fn open(
        &self,
        request: OpenDocxPreviewRequestDto,
    ) -> Result<OpenDocxPreviewResponseDto, ApplicationError> {
        let session_slot = self
            .session_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                ApplicationError::InvalidRequest(format!(
                    "at most {MAX_DOCX_SESSIONS} DOCX previews may be open; close another preview first"
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
        let guard = CancellationGuard::new(cancellation.clone());
        let summary = provider
            .inspect(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        if summary.kind != EntryKind::File {
            return Err(ApplicationError::InvalidRequest(
                "DOCX preview requires a regular file".to_owned(),
            ));
        }
        let source_bytes = provider
            .file_size(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        if source_bytes > MAX_DOCX_SOURCE_BYTES {
            return Err(budget_error("source file size", MAX_DOCX_SOURCE_BYTES));
        }
        let revision = source_revision(&summary, source_bytes);
        let reader = provider
            .open_read(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        let mut bytes = Vec::with_capacity(source_bytes as usize);
        reader
            .take(MAX_DOCX_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(read_stream_error)?;
        if bytes.len() as u64 > MAX_DOCX_SOURCE_BYTES {
            return Err(budget_error("source file size", MAX_DOCX_SOURCE_BYTES));
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
        let mut resources = HashMap::new();
        let mut resource_dtos = Vec::with_capacity(parsed.resources.len());
        for resource in parsed.resources {
            let resource_id = Uuid::new_v5(&session_id, resource.source.as_bytes());
            let byte_length = resource.data.len() as u64;
            resource_dtos.push(DocxPreviewResourceDto {
                resource_id,
                source: resource.source,
                media_type: resource.media_type.clone(),
                byte_length,
            });
            resources.insert(
                resource_id,
                RetainedResource {
                    media_type: resource.media_type,
                    data: resource.data,
                },
            );
        }
        let session = Arc::new(Session {
            provider,
            entry,
            revision: revision.clone(),
            cancellation: cancellation.clone(),
            resources,
            _slot: session_slot,
        });
        self.sessions.write().await.insert(session_id, session);
        guard.disarm();
        Ok(OpenDocxPreviewResponseDto {
            session_id,
            source_revision: revision,
            source_bytes,
            html: parsed.html,
            resources: resource_dtos,
            omitted_features: omitted_features(),
        })
    }

    pub(crate) async fn read_resource(
        &self,
        request: ReadDocxPreviewResourceRequestDto,
    ) -> Result<ReadDocxPreviewResourceResponseDto, ApplicationError> {
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        let resource = session
            .resources
            .get(&request.resource_id)
            .ok_or(ApplicationError::NotFound)?;
        Ok(ReadDocxPreviewResourceResponseDto {
            data: resource.data.clone(),
            media_type: resource.media_type.clone(),
        })
    }

    pub(crate) async fn close(
        &self,
        request: DocxPreviewSessionRequestDto,
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

struct CancellationGuard {
    cancellation: CancellationToken,
    armed: bool,
}

impl CancellationGuard {
    fn new(cancellation: CancellationToken) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
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
        "exact pagination",
        "floating objects",
        "text boxes",
        "charts",
        "headers and footers",
        "tracked changes",
        "field evaluation",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn parse_docx(
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<ParsedDocx, ApplicationError> {
    if cancellation.is_cancelled() {
        return Err(ApplicationError::OperationCancelled);
    }
    if bytes.len() as u64 > MAX_DOCX_SOURCE_BYTES {
        return Err(budget_error("source file size", MAX_DOCX_SOURCE_BYTES));
    }
    preflight_package(bytes, cancellation)?;
    let (document, media) = ferrodoc::parse_with_media(bytes, ferrodoc::Format::Docx)
        .map_err(|error| invalid_docx(error.to_string()))?;
    if cancellation.is_cancelled() {
        return Err(ApplicationError::OperationCancelled);
    }
    let rendered = ferrodoc::render(&document, ferrodoc::Format::Html)
        .map_err(|error| invalid_docx(error.to_string()))?;
    if rendered.len() > MAX_DOCX_RENDERED_BYTES {
        return Err(budget_error(
            "rendered output",
            MAX_DOCX_RENDERED_BYTES as u64,
        ));
    }
    let html = String::from_utf8(rendered)
        .map_err(|_| invalid_docx("the converter produced non-UTF-8 HTML"))?;
    let mut resources = media
        .into_iter()
        .filter_map(|(source, data)| {
            image_media_type(&source).map(|media_type| ParsedDocxResource {
                source,
                media_type: media_type.to_owned(),
                data,
            })
        })
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(ParsedDocx { html, resources })
}

fn invalid_docx(detail: impl AsRef<str>) -> ApplicationError {
    ApplicationError::InvalidRequest(format!(
        "DOCX preview is unavailable because the package is malformed: {}",
        detail.as_ref()
    ))
}

fn budget_error(resource: &str, limit: u64) -> ApplicationError {
    let limit = if limit.is_multiple_of(1024 * 1024) {
        format!("{} MiB", limit / (1024 * 1024))
    } else {
        format!("{limit} bytes")
    };
    ApplicationError::InvalidRequest(format!(
        "DOCX preview cannot open this file because it exceeds the {resource} limit of {limit}. Open it externally."
    ))
}

fn preflight_package(
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<(), ApplicationError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| invalid_docx(error.to_string()))?;
    if archive.len() > MAX_DOCX_ZIP_ENTRIES {
        return Err(ApplicationError::InvalidRequest(format!(
            "DOCX preview exceeds the ZIP entry-count budget ({MAX_DOCX_ZIP_ENTRIES}); open it in an external application"
        )));
    }
    let mut expanded_bytes = 0_u64;
    let mut image_count = 0_usize;
    let mut image_bytes = 0_u64;
    for index in 0..archive.len() {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        let mut file = archive
            .by_index(index)
            .map_err(|error| invalid_docx(error.to_string()))?;
        expanded_bytes = expanded_bytes
            .checked_add(file.size())
            .ok_or_else(|| budget_error("expanded ZIP", MAX_DOCX_EXPANDED_BYTES))?;
        if expanded_bytes > MAX_DOCX_EXPANDED_BYTES {
            return Err(budget_error("expanded ZIP", MAX_DOCX_EXPANDED_BYTES));
        }
        let name = file.name().to_owned();
        if image_media_type(&name).is_some() && !file.is_dir() {
            image_count += 1;
            if image_count > MAX_DOCX_IMAGES {
                return Err(ApplicationError::InvalidRequest(format!(
                    "DOCX preview exceeds the image-count budget ({MAX_DOCX_IMAGES}); open it in an external application"
                )));
            }
            if file.size() > MAX_DOCX_IMAGE_BYTES as u64 {
                return Err(budget_error("per-image bytes", MAX_DOCX_IMAGE_BYTES as u64));
            }
            image_bytes = image_bytes.checked_add(file.size()).ok_or_else(|| {
                budget_error("total image bytes", MAX_DOCX_TOTAL_IMAGE_BYTES as u64)
            })?;
            if image_bytes > MAX_DOCX_TOTAL_IMAGE_BYTES as u64 {
                return Err(budget_error(
                    "total image bytes",
                    MAX_DOCX_TOTAL_IMAGE_BYTES as u64,
                ));
            }
        }
        let lowercase_name = name.to_ascii_lowercase();
        if lowercase_name.ends_with(".xml") || lowercase_name.ends_with(".rels") {
            validate_xml_depth(&mut file, cancellation)?;
        }
    }
    Ok(())
}

fn validate_xml_depth(
    source: &mut impl Read,
    cancellation: &CancellationToken,
) -> Result<(), ApplicationError> {
    let mut reader = quick_xml::Reader::from_reader(BufReader::new(source));
    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    loop {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(_)) => {
                depth += 1;
                if depth > MAX_DOCX_XML_DEPTH {
                    return Err(ApplicationError::InvalidRequest(format!(
                        "DOCX preview exceeds the XML depth budget ({MAX_DOCX_XML_DEPTH}); open it in an external application"
                    )));
                }
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::DocType(_)) => {
                return Err(invalid_docx("DOCTYPE declarations are not allowed"));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(invalid_docx(error.to_string())),
        }
        buffer.clear();
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use fm_vfs::ProviderRegistry;
    use fm_vfs_local::LocalFileSystemProvider;
    use zip::write::SimpleFileOptions;

    use super::*;

    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

    fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut bytes);
            archive
                .start_file("[Content_Types].xml", SimpleFileOptions::default())
                .expect("start content types");
            archive
                .write_all(CONTENT_TYPES.as_bytes())
                .expect("write content types");
            for (name, data) in entries {
                archive
                    .start_file(*name, SimpleFileOptions::default())
                    .expect("start fixture entry");
                archive.write_all(data).expect("write fixture entry");
            }
            archive.finish().expect("finish fixture package");
        }
        bytes.into_inner()
    }

    fn document_xml(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"
 xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">
  <w:body>{body}<w:sectPr/></w:body>
</w:document>"#
        )
    }

    fn representative_docx() -> Vec<u8> {
        let document = document_xml(
            r#"
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Report</w:t></w:r></w:p>
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Bold</w:t></w:r><w:r><w:t xml:space="preserve"> and </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>italic</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Value</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:p><w:hyperlink r:id="rLink"><w:r><w:t>Example</w:t></w:r></w:hyperlink></w:p>
<w:p><w:r><w:footnoteReference w:id="2"/></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rImage"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
        );
        let relationships = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
  <Relationship Id="rImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#;
        let numbering = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="&#8226;"/></w:lvl></w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
</w:numbering>"#;
        let footnotes = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="2"><w:p><w:r><w:t>Footnote text</w:t></w:r></w:p></w:footnote>
</w:footnotes>"#;
        let styles = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>
</w:styles>"#;
        package(&[
            ("word/document.xml", document.as_bytes()),
            ("word/_rels/document.xml.rels", relationships),
            ("word/numbering.xml", numbering),
            ("word/footnotes.xml", footnotes),
            ("word/styles.xml", styles),
            ("word/media/image1.png", b"\x89PNG\r\n\x1a\nfixture"),
        ])
    }

    fn service() -> DocxPreviewService {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider::new()));
        DocxPreviewService::new(providers)
    }

    fn open_request(path: &std::path::Path) -> OpenDocxPreviewRequestDto {
        OpenDocxPreviewRequestDto {
            location: Location::from_native_path(path)
                .expect("fixture path must be a location")
                .into(),
        }
    }

    #[test]
    fn preserves_representative_semantic_content_and_images() {
        let parsed = parse_docx(&representative_docx(), &CancellationToken::new())
            .expect("representative DOCX must parse");

        assert!(parsed.html.contains("<h1"), "{}", parsed.html);
        assert!(parsed.html.contains("<strong>Bold</strong>"));
        assert!(parsed.html.contains("<em>italic</em>"));
        assert!(parsed.html.contains("<ul>"));
        assert!(parsed.html.contains("<table>"));
        assert!(parsed.html.contains("href=\"https://example.com\""));
        assert!(parsed.html.contains("Footnote text"));
        assert!(parsed.html.contains("<img"));
        assert_eq!(parsed.resources.len(), 1);
        assert!(
            parsed
                .html
                .contains(&format!("src=\"{}\"", parsed.resources[0].source))
        );
        assert_eq!(parsed.resources[0].media_type, "image/png");
    }

    #[test]
    fn rejects_malformed_ooxml() {
        let error = parse_docx(b"not a ZIP package", &CancellationToken::new())
            .expect_err("malformed input must fail");
        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }

    #[test]
    fn rejects_an_expanded_zip_bomb_before_parsing() {
        let oversized = vec![b'x'; (MAX_DOCX_EXPANDED_BYTES + 1) as usize];
        let bytes = package(&[("word/document.xml", &oversized)]);
        let error = parse_docx(&bytes, &CancellationToken::new())
            .expect_err("expanded package budget must be enforced");
        assert!(error.to_string().contains("expanded ZIP"));
    }

    #[test]
    fn enforces_image_count_outside_the_conventional_media_directory() {
        let names = (0..=MAX_DOCX_IMAGES)
            .map(|index| format!("word/other/image-{index}.PNG"))
            .collect::<Vec<_>>();
        let entries = names
            .iter()
            .map(|name| (name.as_str(), b"x".as_slice()))
            .collect::<Vec<_>>();
        let bytes = package(&entries);

        let error = parse_docx(&bytes, &CancellationToken::new())
            .expect_err("all package images must count toward the image budget");
        assert!(error.to_string().contains("image-count"));
    }

    #[test]
    fn rejects_excessive_xml_depth() {
        let nested = format!(
            "{}x{}",
            "<w:p>".repeat(MAX_DOCX_XML_DEPTH + 1),
            "</w:p>".repeat(MAX_DOCX_XML_DEPTH + 1)
        );
        let document = document_xml(&nested);
        let bytes = package(&[("word/document.xml", document.as_bytes())]);
        let error = parse_docx(&bytes, &CancellationToken::new())
            .expect_err("XML depth budget must be enforced");
        assert!(error.to_string().contains("XML depth"));
    }

    #[test]
    fn honours_cancellation_before_parsing() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = parse_docx(&representative_docx(), &cancellation)
            .expect_err("cancelled parsing must stop");
        assert_eq!(error, ApplicationError::OperationCancelled);
    }

    #[tokio::test]
    async fn invalidates_resources_when_the_source_revision_changes() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("report.docx");
        std::fs::write(&path, representative_docx()).expect("write DOCX fixture");
        let service = service();
        let opened = service
            .open(open_request(&path))
            .await
            .expect("open DOCX preview");
        let resource_id = opened.resources[0].resource_id;

        std::fs::write(&path, b"changed source").expect("replace DOCX fixture");
        let error = service
            .read_resource(ReadDocxPreviewResourceRequestDto {
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
    async fn close_cancels_and_releases_the_session_resources() {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("report.docx");
        std::fs::write(&path, representative_docx()).expect("write DOCX fixture");
        let service = service();
        let opened = service
            .open(open_request(&path))
            .await
            .expect("open DOCX preview");
        let resource_id = opened.resources[0].resource_id;
        service
            .close(DocxPreviewSessionRequestDto {
                session_id: opened.session_id,
            })
            .await
            .expect("close DOCX preview");

        let error = service
            .read_resource(ReadDocxPreviewResourceRequestDto {
                session_id: opened.session_id,
                resource_id,
            })
            .await
            .expect_err("closed session must be gone");
        assert_eq!(error, ApplicationError::NotFound);
    }
}
