//! Bounded, provider-neutral document conversion and structural chunking for
//! semantic retrieval (task 0180).
//!
//! This crate is deliberately pure: it accepts already-read bytes (or a
//! bounded [`std::io::Read`]) plus *trusted* [`DocumentMetadata`] and returns
//! normalized structural units, warnings, omissions and provenance. It never
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

mod advanced;
mod budget;
mod builder;
mod cancellation;
mod chunk;
mod converter;
mod formats;
mod model;
mod sniff;
mod text;
mod tokens;

pub use advanced::{
    AdvancedCapability, AdvancedConversion, AdvancedConverterAdapter, AdvancedConverterBackend,
    OptionalConverter, ProvenancePrecision,
};
pub use budget::{BudgetKind, Clock, ConversionBudgets, ManualClock, SystemClock};
pub use cancellation::{Cancellation, CancellationFlag, CancellationSignal};
pub use chunk::{
    Chunk, ChunkPart, ChunkProvenance, Chunker, ChunkerOptions, InvalidChunkerOptions,
};
pub use converter::{
    BASELINE_CONVERTER_VERSION, BaselineConverter, ConversionContext, ConversionError,
    DocumentConverter, SourceContent,
};
pub use model::{
    Completeness, ComponentVersion, ConversionOutcome, ConversionWarning, ConvertedDocument,
    DocumentMetadata, FormatKind, MediaType, Omission, Provenance, SkipReason, StructuralUnit,
    TopLevelBoundary, UnitKind,
};
pub use sniff::{ResolvedFormat, resolve_format};
pub use text::{
    DecodedText, SanitizedText, SourceMap, SourceSegment, decode, instruction_like_excerpt,
    sanitize,
};
pub use tokens::{TOKEN_ESTIMATOR_VERSION, estimate_tokens};
