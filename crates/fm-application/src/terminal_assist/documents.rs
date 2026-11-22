//! Documents as text for the file tools and `@file` mentions: EPUB, Word, PDF, PowerPoint and
//! Excel files are converted by the same bounded engine that indexes them, so the model reads the
//! words of a book or report instead of a refusal for a "binary" file.

use std::sync::Arc;

use fm_semantic_conversion::{Completeness, ConversionOutcome};
use fm_vfs::ProviderRegistry;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::tools::{FileCall, MAX_READ_BYTES, ToolError, ToolHost, ToolOutput, error_text};
use crate::document_conversion::DocumentConversionService;
use fm_agent::redact_secrets;

/// File name extensions read as documents.
const EXTENSIONS: &[&str] = &["epub", "docx", "pdf", "pptx", "xlsx"];

/// Whether a path names a document rather than a plain text file.
pub(super) fn is_document(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        EXTENSIONS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    })
}

/// Most converted documents kept for one question; the oldest is dropped beyond this.
const CACHED_DOCUMENTS: usize = 4;

/// Converts documents inside the working folder. A converted text is kept for the life of this
/// value (one question), so reading a long document window by window converts it once.
pub(super) struct Documents {
    service: DocumentConversionService,
    providers: ProviderRegistry,
    cache: Mutex<Vec<(String, Arc<str>)>>,
    /// How often a document was actually converted, for tests.
    #[cfg(test)]
    conversions: std::sync::atomic::AtomicUsize,
}

impl Documents {
    pub(super) fn new(providers: &ProviderRegistry) -> Self {
        Self {
            service: DocumentConversionService::new(providers.clone()),
            providers: providers.clone(),
            cache: Mutex::new(Vec::new()),
            #[cfg(test)]
            conversions: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[cfg(test)]
    pub(super) fn conversions(&self) -> usize {
        self.conversions.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The converted text of a document, from the cache when it was read before.
    async fn text_of(&self, host: &ToolHost, relative: &str) -> Result<Arc<str>, ToolError> {
        if let Some((_, text)) = self
            .cache
            .lock()
            .await
            .iter()
            .find(|(name, _)| name == relative)
        {
            return Ok(Arc::clone(text));
        }
        let location = host.document_location(&self.providers, relative).await?;
        let outcome = self
            .service
            .convert(location, CancellationToken::new())
            .await
            .map_err(|_| ToolError::Unreadable(relative.to_owned()))?;
        #[cfg(test)]
        self.conversions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let text: Arc<str> = converted_text(outcome, relative)?.into();
        let mut cache = self.cache.lock().await;
        if cache.len() >= CACHED_DOCUMENTS {
            cache.remove(0);
        }
        cache.push((relative.to_owned(), Arc::clone(&text)));
        Ok(text)
    }

    /// Runs `read_file` on a document. `None` when the call is not a read of a document, so the
    /// plain tools handle it.
    pub(super) async fn run(&self, host: &ToolHost, call: &FileCall) -> Option<ToolOutput> {
        let FileCall::ReadFile { path, offset } = call else {
            return None;
        };
        if !is_document(path) {
            return None;
        }
        let text = self
            .read(host, path, offset.unwrap_or(0))
            .await
            .unwrap_or_else(|error| error_text(&error));
        Some(ToolOutput {
            step: format!("Read {path}"),
            text,
        })
    }

    /// The text of a document from byte `offset` of its converted text, bounded and redacted.
    pub(super) async fn read(
        &self,
        host: &ToolHost,
        relative: &str,
        offset: usize,
    ) -> Result<String, ToolError> {
        let text = self.text_of(host, relative).await?;
        Ok(redact_secrets(&window(&text, offset)))
    }
}

/// Documents inside a skill's folder, as text (task 0339): the folder is sandboxed like a working
/// folder, so a document cannot be a way out of it, and secret-looking names are refused.
pub(crate) struct SkillFolderDocuments {
    documents: Documents,
}

impl SkillFolderDocuments {
    pub(crate) fn new(providers: &ProviderRegistry) -> Self {
        Self {
            documents: Documents::new(providers),
        }
    }
}

#[async_trait::async_trait]
impl fm_agent::SkillDocuments for SkillFolderDocuments {
    async fn read(
        &self,
        folder: &std::path::Path,
        relative: &str,
        offset: usize,
    ) -> Option<Result<String, String>> {
        if !is_document(relative) {
            return None;
        }
        let Some(sandbox) = super::tools::Sandbox::new(folder) else {
            return Some(Err("Error: the skill's folder cannot be read.".to_owned()));
        };
        let host = ToolHost::Local(sandbox);
        Some(
            self.documents
                .read(&host, relative, offset)
                .await
                .map_err(|error| error_text(&error)),
        )
    }
}

/// The text of a converted document, or why there is none.
fn converted_text(outcome: ConversionOutcome, name: &str) -> Result<String, ToolError> {
    let fail = |detail: &str| ToolError::Document(format!("{name}: {detail}"));
    match outcome {
        ConversionOutcome::Converted(document) => {
            let mut text = document
                .units()
                .iter()
                .map(|unit| unit.text.trim())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            if document.completeness() == Completeness::Partial {
                text.push_str("\n\n[the document is longer than the converter reads]");
            }
            if text.is_empty() {
                return Err(fail("no text found"));
            }
            Ok(text)
        }
        ConversionOutcome::Encrypted { .. } => Err(fail("encrypted or password protected")),
        ConversionOutcome::NoTextLayer { .. } => {
            Err(fail("no text layer (a scan); it would need OCR"))
        }
        ConversionOutcome::OverBudget { .. } => Err(fail("too large to read")),
        ConversionOutcome::Skipped { .. } => Err(fail("no text found")),
        ConversionOutcome::Malformed { .. } => Err(fail("damaged or not a valid document")),
        _ => Err(fail("this kind of file cannot be read")),
    }
}

/// At most [`MAX_READ_BYTES`] of `text` from byte `offset` on, cut at character boundaries, with a
/// note on how to continue when more follows.
fn window(text: &str, offset: usize) -> String {
    let mut start = offset.min(text.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + MAX_READ_BYTES).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[start..end].to_owned();
    if end < text.len() {
        out.push_str(&format!(
            "\n[truncated at byte {end}: call read_file with \"offset\":{end} to continue]"
        ));
    }
    out
}

#[cfg(test)]
mod tests;
