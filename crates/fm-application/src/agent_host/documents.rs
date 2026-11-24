//! Documents as text for the file tools and `@file` mentions: EPUB, Word, PDF, PowerPoint, Excel
//! and HTML files are converted by the same bounded engine that indexes them, so the model reads the
//! words of a book or report instead of a refusal for a "binary" file.

use std::sync::Arc;

use fm_semantic_conversion::{BudgetKind, Completeness, ConversionOutcome};
use fm_vfs::ProviderRegistry;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::sandbox::{
    FileCall, MAX_READ_BYTES, Sandbox, ToolError, ToolHost, ToolOutput, error_text,
};
use crate::document_conversion::DocumentConversionService;
use fm_agent::redact_secrets;

/// File name extensions read as documents.
/// HTML is among them so a saved web page reads as its words, not as markup, scripts and styles.
const EXTENSIONS: &[&str] = &[
    "epub", "docx", "pdf", "pptx", "xlsx", "html", "htm", "xhtml",
];

/// Whether a path names a document rather than a plain text file.
pub(crate) fn is_document(path: &str) -> bool {
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
pub(crate) struct Documents {
    service: DocumentConversionService,
    providers: ProviderRegistry,
    cache: Mutex<Vec<(String, Arc<str>)>>,
    /// How often a document was actually converted, for tests.
    #[cfg(test)]
    conversions: std::sync::atomic::AtomicUsize,
}

impl Documents {
    pub(crate) fn new(providers: &ProviderRegistry) -> Self {
        Self {
            service: DocumentConversionService::for_reading(providers.clone()),
            providers: providers.clone(),
            cache: Mutex::new(Vec::new()),
            #[cfg(test)]
            conversions: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[cfg(test)]
    pub(crate) fn conversions(&self) -> usize {
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
    pub(crate) async fn run(&self, host: &ToolHost, call: &FileCall) -> Option<ToolOutput> {
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
    pub(crate) async fn read(
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
        store: &fm_agent::StoreHandle,
        path: &str,
        offset: usize,
    ) -> Option<Result<String, fm_agent::DocumentError>> {
        // Documents of skills on another machine are read as plain text (task 0342).
        if store.id() != fm_agent::LOCAL_STORE_ID {
            return None;
        }
        let path = std::path::Path::new(path);
        let name = path.file_name()?.to_string_lossy().into_owned();
        if !is_document(&name) {
            return None;
        }
        // The harness has checked the path against the skill's sandbox; the file's own folder is
        // sandboxed again here, so the converter reads exactly that file.
        let folder = path.parent()?.to_path_buf();
        let sandbox = tokio::task::spawn_blocking(move || Sandbox::new(&folder))
            .await
            .ok()
            .flatten();
        let Some(sandbox) = sandbox else {
            return Some(Err(fm_agent::DocumentError(
                "the skill's folder cannot be read.".to_owned(),
            )));
        };
        let host = ToolHost::Local(sandbox);
        Some(
            self.documents
                .read(&host, &name, offset)
                .await
                .map_err(|error| fm_agent::DocumentError(error.to_string())),
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
        ConversionOutcome::OverBudget { budget, limit } => Err(fail(&over_budget(budget, limit))),
        ConversionOutcome::Skipped { .. } => Err(fail("no text found")),
        ConversionOutcome::Malformed { .. } => Err(fail("damaged or not a valid document")),
        _ => Err(fail("this kind of file cannot be read")),
    }
}

/// Which limit a conversion went over, in words: the reason a document cannot be read, so the
/// model is not told "too large" for a conversion that only took too long.
fn over_budget(budget: BudgetKind, limit: u64) -> String {
    match budget {
        BudgetKind::Time => format!(
            "converting it took longer than {} s; try again, or read a smaller document",
            limit.div_ceil(1000)
        ),
        BudgetKind::SourceBytes => {
            format!(
                "larger than the {} MB the converter reads",
                limit / (1024 * 1024)
            )
        }
        BudgetKind::ExpandedBytes => format!(
            "its unpacked contents are larger than the {} MB the converter reads",
            limit / (1024 * 1024)
        ),
        BudgetKind::Items => {
            format!("more than the {limit} pages, slides or sheets the converter reads")
        }
        other => format!("over the converter's {other} limit ({limit})"),
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
