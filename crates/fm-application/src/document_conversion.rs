//! Provider-neutral document conversion for semantic ingestion (task 0180).
//!
//! This service is the narrow bridge between the VFS and the pure
//! [`fm_semantic_conversion`] engine: it streams bounded bytes from whichever
//! provider owns the location, builds *trusted* metadata that deliberately
//! contains no path, and runs the CPU-bound conversion on a blocking thread
//! with the caller's cancellation token attached.
//!
//! There is no HTTP route and no Tauri command here. Ingestion wiring - what
//! is converted, when, and where the chunks are stored - belongs to task 0182;
//! this task only has to make one document convertible from any provider.

use std::sync::Arc;

use fm_domain::{EntryId, EntryKind, Location};
use fm_semantic_conversion::{
    Cancellation, CancellationSignal, ConversionBudgets, ConversionContext, ConversionOutcome,
    DocumentConverter, DocumentMetadata, SourceContent,
};
use fm_semantic_docling::converter_with_baseline_fallback;
use fm_vfs::{EntryRef, ProviderCapabilities, ProviderRegistry};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::file_editor::read_stream_error;

/// Adapts the host's cancellation token to the engine's runtime-free signal.
struct TokenSignal(CancellationToken);

impl CancellationSignal for TokenSignal {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// Converts documents read through the [`ProviderRegistry`].
#[derive(Clone)]
pub(crate) struct DocumentConversionService {
    providers: ProviderRegistry,
    converter: Arc<dyn DocumentConverter>,
    budgets: ConversionBudgets,
}

impl DocumentConversionService {
    /// Creates the service with deterministic Docling PDF extraction,
    /// baseline fallback, and default budgets.
    pub(crate) fn new(providers: ProviderRegistry) -> Self {
        Self {
            providers,
            converter: Arc::new(converter_with_baseline_fallback()),
            budgets: ConversionBudgets::default(),
        }
    }

    /// Converts one file into structural units.
    ///
    /// Only [`ProviderCapabilities::READ`] is required, so every provider -
    /// local, SFTP, WebDAV, S3, an archive - works identically. Oversized
    /// content, unsupported media types, encrypted or malformed packages and
    /// cancellation all surface as typed [`ConversionOutcome`]s rather than
    /// errors; an `Err` here means the *request* was wrong or the provider
    /// failed.
    pub(crate) async fn convert(
        &self,
        location: Location,
        cancellation: CancellationToken,
    ) -> Result<ConversionOutcome, ApplicationError> {
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
            location: location.clone(),
        };
        let summary = provider
            .inspect(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        if summary.kind != EntryKind::File {
            return Err(ApplicationError::InvalidRequest(
                "semantic conversion requires a regular file".to_owned(),
            ));
        }
        let max_source_bytes = self.budgets.max_source_bytes;
        if summary.size.is_some_and(|size| size > max_source_bytes) {
            return Ok(ConversionOutcome::OverBudget {
                budget: fm_semantic_conversion::BudgetKind::SourceBytes,
                limit: max_source_bytes,
            });
        }

        let reader = provider
            .open_read(&entry, cancellation.child_token())
            .await
            .map_err(ApplicationError::from)?;
        let mut bytes = Vec::new();
        reader
            .take(max_source_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .await
            .map_err(read_stream_error)?;
        if bytes.len() as u64 > max_source_bytes {
            return Ok(ConversionOutcome::OverBudget {
                budget: fm_semantic_conversion::BudgetKind::SourceBytes,
                limit: max_source_bytes,
            });
        }

        let metadata = trusted_metadata(&summary, bytes.len() as u64);
        let context = ConversionContext::new()
            .with_budgets(self.budgets.clone())
            .with_cancellation(Cancellation::new(Arc::new(TokenSignal(
                cancellation.clone(),
            ))));
        let converter = Arc::clone(&self.converter);
        let outcome = tokio::task::spawn_blocking(move || {
            converter.convert(SourceContent::Bytes(&bytes), &metadata, &context)
        })
        .await
        .map_err(|_| ApplicationError::Internal)?
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        Ok(outcome)
    }
}

/// Builds the metadata handed to the converter.
///
/// Only the media type, extension and byte length are passed on. The file name
/// and path stay behind deliberately: they must not be able to reach embedding
/// input, so that moving or renaming a file never invalidates its vectors.
fn trusted_metadata(summary: &fm_domain::EntrySummary, byte_length: u64) -> DocumentMetadata {
    let mut metadata = DocumentMetadata::unknown().with_byte_length(byte_length);
    if let Some(media_type) = summary.mime_type.as_deref() {
        metadata = metadata.with_media_type(media_type);
    }
    let extension = summary
        .extension
        .clone()
        .or_else(|| extension_of(&summary.name));
    if let Some(extension) = extension {
        metadata = metadata.with_extension(&extension);
    }
    metadata
}

fn extension_of(name: &str) -> Option<String> {
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then(|| extension.to_owned())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use async_trait::async_trait;
    use fm_domain::{EntryKind, EntrySummary, Location, ProviderId};
    use fm_vfs::{
        FileSystemProvider, ListOptions, ProviderReadStream, ProviderWriteStream, RemoveOptions,
        VfsError, WriteOptions,
    };
    use fm_vfs_local::LocalFileSystemProvider;
    use lopdf::{Document, Object, Stream, dictionary};

    use super::*;

    /// A read-only, in-memory provider: it proves conversion depends on
    /// nothing but `open_read` plus `inspect`, so any provider can feed it.
    struct MemoryProvider {
        contents: Vec<u8>,
        name: String,
    }

    #[async_trait]
    impl FileSystemProvider for MemoryProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("memory-conversion-test-double")
        }

        fn schemes(&self) -> &'static [&'static str] {
            &["memory"]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::READ
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::DirectoryPage, VfsError> {
            unreachable!("conversion never lists")
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntryMetadata, VfsError> {
            unreachable!("conversion never reads extended metadata")
        }

        async fn inspect(
            &self,
            entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<EntrySummary, VfsError> {
            Ok(EntrySummary {
                id: entry.id,
                location: entry.location.clone(),
                name: self.name.clone(),
                kind: EntryKind::File,
                size: Some(self.contents.len() as u64),
                modified_at: None,
                created_at: None,
                hidden: false,
                read_only: true,
                extension: None,
                mime_type: None,
                icon_key: None,
                metadata_revision: 0,
                git_status: None,
            })
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            unreachable!("read-only double")
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            unreachable!("read-only double")
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), VfsError> {
            unreachable!("read-only double")
        }

        async fn open_read(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<ProviderReadStream, VfsError> {
            Ok(Box::pin(std::io::Cursor::new(self.contents.clone())))
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<ProviderWriteStream, VfsError> {
            unreachable!("read-only double")
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderChangeStream, VfsError> {
            unreachable!("conversion never watches")
        }
    }

    fn memory_service(contents: &[u8], name: &str) -> DocumentConversionService {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(MemoryProvider {
            contents: contents.to_vec(),
            name: name.to_owned(),
        }));
        DocumentConversionService::new(providers)
    }

    fn local_service() -> DocumentConversionService {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider::new()));
        DocumentConversionService::new(providers)
    }

    fn positioned_pdf(content: &str) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id =
            document.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
                "Resources" => resources_id,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save positioned PDF");
        bytes
    }

    #[tokio::test]
    async fn converts_bytes_streamed_from_any_provider() {
        let service = memory_service(b"# Title\n\nBody paragraph.\n", "notes.md");
        let outcome = service
            .convert(
                Location::new(
                    ProviderId::new("memory-conversion-test-double"),
                    "memory://notes.md",
                ),
                CancellationToken::new(),
            )
            .await
            .expect("conversion");
        let document = outcome.document().expect("converted");
        assert_eq!(
            document.format(),
            fm_semantic_conversion::FormatKind::Markdown
        );
        assert_eq!(document.units()[0].text, "Title");
    }

    #[tokio::test]
    async fn default_conversion_uses_geometry_aware_docling_pdf_order() {
        let bytes = positioned_pdf(
            "BT /F1 12 Tf\n\
             1 0 0 1 330 720 Tm (Right column starts after the left column.) Tj\n\
             1 0 0 1 72 720 Tm (Left column starts first in reading order.) Tj\n\
             1 0 0 1 330 690 Tm (Right column continues after left finishes.) Tj\n\
             1 0 0 1 72 690 Tm (Left column continues before the right column.) Tj\n\
             ET\n",
        );
        let service = memory_service(&bytes, "columns.pdf");

        let outcome = service
            .convert(
                Location::new(
                    ProviderId::new("memory-conversion-test-double"),
                    "memory://columns.pdf",
                ),
                CancellationToken::new(),
            )
            .await
            .expect("conversion");

        let text = outcome
            .document()
            .expect("converted")
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.find("Left column starts").expect("left column")
                < text.find("Right column starts").expect("right column")
        );
    }

    #[tokio::test]
    async fn converts_a_local_file_identically() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("notes.md");
        let mut file = std::fs::File::create(&path).expect("create");
        file.write_all(b"# Title\n\nBody paragraph.\n")
            .expect("write");
        drop(file);
        let location = Location::from_native_path(&path).expect("location");
        let outcome = local_service()
            .convert(location, CancellationToken::new())
            .await
            .expect("conversion");
        let document = outcome.document().expect("converted");
        assert_eq!(
            document.format(),
            fm_semantic_conversion::FormatKind::Markdown
        );
        assert_eq!(document.units()[0].text, "Title");
    }

    #[tokio::test]
    async fn a_directory_is_rejected_as_an_invalid_request() {
        let directory = tempfile::tempdir().expect("temp dir");
        let location = Location::from_native_path(directory.path()).expect("location");
        let error = local_service()
            .convert(location, CancellationToken::new())
            .await
            .expect_err("directories are not documents");
        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn oversized_sources_report_an_over_budget_outcome() {
        let mut service = memory_service(&vec![b'a'; 4096], "big.txt");
        service.budgets = ConversionBudgets {
            max_source_bytes: 16,
            ..ConversionBudgets::default()
        };
        let outcome = service
            .convert(
                Location::new(
                    ProviderId::new("memory-conversion-test-double"),
                    "memory://big.txt",
                ),
                CancellationToken::new(),
            )
            .await
            .expect("conversion");
        assert!(matches!(
            outcome,
            ConversionOutcome::OverBudget {
                budget: fm_semantic_conversion::BudgetKind::SourceBytes,
                limit: 16
            }
        ));
    }

    #[tokio::test]
    async fn cancellation_is_propagated_into_the_blocking_conversion() {
        let service = memory_service(b"some text to convert", "notes.txt");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let outcome = service
            .convert(
                Location::new(
                    ProviderId::new("memory-conversion-test-double"),
                    "memory://notes.txt",
                ),
                cancellation,
            )
            .await
            .expect("conversion");
        assert_eq!(outcome, ConversionOutcome::Cancelled);
    }

    #[test]
    fn trusted_metadata_carries_no_path_or_file_name() {
        let summary = EntrySummary {
            id: EntryId::new(),
            location: Location::new(ProviderId::new("local"), "file:///secret/report.docx"),
            name: "report.docx".to_owned(),
            kind: EntryKind::File,
            size: Some(10),
            modified_at: None,
            created_at: None,
            hidden: false,
            read_only: false,
            extension: None,
            mime_type: None,
            icon_key: None,
            metadata_revision: 0,
            git_status: None,
        };
        let metadata = trusted_metadata(&summary, 10);
        assert_eq!(metadata.extension(), Some("docx"));
        assert_eq!(metadata.byte_length(), Some(10));
        let rendered = format!("{metadata:?}");
        assert!(!rendered.contains("report"));
        assert!(!rendered.contains("secret"));
    }
}
