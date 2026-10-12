//! Bounded, provider-neutral document conversion and structural chunking for
//! semantic retrieval (task 0180).
//!
//! This crate is deliberately pure: it accepts already-read bytes (or a
//! bounded [`std::io::Read`]) plus *trusted* [`DocumentMetadata`] and returns
//! normalized structural units and opt-in bounded visual attachments with
//! warnings, omissions and provenance. It never
//! opens a path, resolves a VFS provider, performs network I/O or depends on a
//! host runtime, which is what allows the same conversion to run behind the
//! HTTP host, the Tauri host and the semantic worker unchanged.
//!
//! # Shape of the pipeline
//!
//! ```text
//! bytes + DocumentMetadata
//!   -> format resolution (deterministic sniffing, [`sniff`])
//!   -> per-format converter (bounded, cancellable)
//!   -> normalized [`StructuralUnit`]s with provenance
//!   -> [`Chunker`] -> [`Chunk`]s with embedding input and fingerprints
//! ```
//!
//! Every failure mode is a *typed, visible* [`ConversionOutcome`] rather than
//! an empty success: unsupported media types, encrypted or malformed
//! packages, PDFs without a text layer, and budget overruns are all
//! distinguishable by the caller. Bounded output that had to drop content is
//! marked [`Completeness::Partial`] and carries [`Omission`]s; there is no
//! silent partial success.
//! EPUB conversion is signature-verified and follows only readable HTML
//! resources declared by the package spine. Optional visuals are limited to
//! local image resources that those spine documents reference and the OPF
//! manifest declares; other bundled media is never crawled.

mod advanced;
mod budget;
mod builder;
mod cancellation;
mod chunk;
mod converter;
mod formats;
mod model;
mod pdf_toc;
mod sniff;
mod text;
mod tokens;

/// Maximum eager decompression of one PDF object or xref stream while loading.
/// This is not a limit on total parser memory or a substitute for isolation.
pub const MAX_PDF_STRUCTURAL_STREAM_BYTES: usize = 16 * 1024 * 1024;
/// Checks that an untrusted PDF's loaded page tree matches its declared page count.
///
/// `lopdf` can omit an oversized compressed object stream when loading it.
/// Without this check, its missing page objects could be mistaken for a
/// shorter, successfully converted document.
pub fn checked_pdf_pages(
    document: &lopdf::Document,
) -> Result<std::collections::BTreeMap<u32, lopdf::ObjectId>, String> {
    let catalog = document
        .catalog()
        .map_err(|error| format!("PDF catalog is unreadable: {error}"))?;
    let pages_id = catalog
        .get(b"Pages")
        .and_then(lopdf::Object::as_reference)
        .map_err(|error| format!("PDF page tree is unreadable: {error}"))?;
    let count = document
        .get_dictionary(pages_id)
        .and_then(|pages| pages.get(b"Count"))
        .and_then(lopdf::Object::as_i64)
        .map_err(|error| format!("PDF page count is unreadable: {error}"))?;
    let pages = document.get_pages();
    if count < 1 || usize::try_from(count) != Ok(pages.len()) {
        return Err(format!(
            "PDF page tree is incomplete: declared {count} pages, loaded {}",
            pages.len()
        ));
    }
    Ok(pages)
}

pub use advanced::{
    AdvancedCapability, AdvancedConversion, AdvancedConverterAdapter, AdvancedConverterBackend,
    OptionalConverter, ProvenancePrecision,
};
pub use budget::{BudgetKind, Clock, ConversionBudgets, ManualClock, SystemClock};
pub use cancellation::{Cancellation, CancellationFlag, CancellationSignal};
pub use chunk::{
    Chunk, ChunkPart, ChunkProvenance, Chunker, ChunkerOptions, InvalidChunkerOptions,
    STRUCTURAL_CHUNKER_VERSION,
};
pub use converter::{
    BASELINE_CONVERTER_VERSION, BaselineConverter, ConversionContext, ConversionError,
    DocumentConverter, InvalidPdfPageWindow, PdfPageWindow, SourceContent,
};
pub use model::{
    Completeness, ComponentVersion, ConversionOutcome, ConversionWarning, ConvertedDocument,
    DocumentMetadata, FormatKind, MAX_TOTAL_VISUAL_BYTES, MAX_VISUAL_ATTACHMENTS, MAX_VISUAL_BYTES,
    MAX_VISUAL_PIXELS, MediaType, Omission, Provenance, SkipReason, StructuralUnit,
    TopLevelBoundary, UnitKind, VisualAttachment, VisualOmission, VisualProvenance,
};
pub use pdf_toc::{clean_pdf_toc_text, is_pdf_toc_section, repair_spaced_elsevier_doi};
pub use sniff::{ResolvedFormat, resolve_format};
pub use text::{
    DecodedText, SanitizedText, SourceMap, SourceSegment, decode, instruction_like_excerpt,
    sanitize,
};
pub use tokens::{TOKEN_ESTIMATOR_VERSION, estimate_tokens};
