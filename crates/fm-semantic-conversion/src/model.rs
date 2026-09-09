//! The conversion data model: media types, structural units, provenance,
//! warnings, omissions and typed outcomes.
//!
//! These types are the semantic representation, not a transport or preview
//! DTO. Nothing here is serialized to a client; task 0182 owns ingestion
//! wiring and any storage projection.

use crate::budget::BudgetKind;
use crate::text::SourceMap;
use serde::{Deserialize, Serialize};

/// Name and revision of a versioned component (a converter or the chunker).
///
/// The revision is part of every chunk fingerprint: changing extraction or
/// chunking behaviour must invalidate previously derived vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentVersion {
    /// Stable component name, for example `baseline`.
    pub name: &'static str,
    /// Monotonic revision of that component's behaviour.
    pub revision: u32,
}

impl ComponentVersion {
    /// Creates a version descriptor.
    #[must_use]
    pub const fn new(name: &'static str, revision: u32) -> Self {
        Self { name, revision }
    }
}

impl std::fmt::Display for ComponentVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}/{}", self.name, self.revision)
    }
}

/// A normalized media type: lowercase essence, parameters dropped.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MediaType(String);

impl MediaType {
    /// Parses a media type, keeping only the lowercase essence.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        let essence = value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        Self(essence)
    }

    /// The `charset` parameter of `value`, if present.
    #[must_use]
    pub fn charset_of(value: &str) -> Option<String> {
        value.split(';').skip(1).find_map(|parameter| {
            let (name, argument) = parameter.split_once('=')?;
            (name.trim().eq_ignore_ascii_case("charset"))
                .then(|| argument.trim().trim_matches('"').to_owned())
        })
    }

    /// The normalized essence string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this media type is textual by its top-level type.
    #[must_use]
    pub fn is_text(&self) -> bool {
        self.0.starts_with("text/")
    }
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Trusted description of the content being converted.
///
/// There is deliberately no path or file name field: the converter must not be
/// able to leak one into embedding input, and a rename must not change any
/// derived content. An `extension` hint is accepted because it disambiguates
/// otherwise identical text bytes (Markdown versus source code), and it is
/// used only for dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentMetadata {
    media_type: Option<MediaType>,
    charset: Option<String>,
    extension: Option<String>,
    language_hint: Option<String>,
    byte_length: Option<u64>,
}

impl DocumentMetadata {
    /// Metadata with no hints at all; the format is then decided by sniffing.
    #[must_use]
    pub fn unknown() -> Self {
        Self::default()
    }

    /// Sets the declared media type. A `charset` parameter, if present, is
    /// retained separately and used when decoding text.
    #[must_use]
    pub fn with_media_type(mut self, value: &str) -> Self {
        self.charset = MediaType::charset_of(value).or(self.charset);
        self.media_type = Some(MediaType::parse(value));
        self
    }

    /// Sets the file extension hint, without a leading dot.
    #[must_use]
    pub fn with_extension(mut self, extension: &str) -> Self {
        let normalized = extension.trim_start_matches('.').to_ascii_lowercase();
        self.extension = (!normalized.is_empty()).then_some(normalized);
        self
    }

    /// Sets a language hint (BCP-47 or a bare language subtag).
    #[must_use]
    pub fn with_language_hint(mut self, language: &str) -> Self {
        let normalized = language.trim().to_ascii_lowercase();
        self.language_hint = (!normalized.is_empty()).then_some(normalized);
        self
    }

    /// Sets the known source length in bytes.
    #[must_use]
    pub fn with_byte_length(mut self, bytes: u64) -> Self {
        self.byte_length = Some(bytes);
        self
    }

    /// The declared media type, if any.
    #[must_use]
    pub fn media_type(&self) -> Option<&MediaType> {
        self.media_type.as_ref()
    }

    /// The declared charset, if any.
    #[must_use]
    pub fn charset(&self) -> Option<&str> {
        self.charset.as_deref()
    }

    /// The extension hint, if any.
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.extension.as_deref()
    }

    /// The language hint, if any.
    #[must_use]
    pub fn language_hint(&self) -> Option<&str> {
        self.language_hint.as_deref()
    }

    /// The declared source length, if any.
    #[must_use]
    pub fn byte_length(&self) -> Option<u64> {
        self.byte_length
    }
}

/// Which baseline converter produced a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormatKind {
    /// Plain text with no further structure.
    PlainText,
    /// Source code, chunked by line ranges and top-level symbols.
    SourceCode,
    /// CommonMark Markdown.
    Markdown,
    /// Bounded HTML.
    Html,
    /// EPUB package with manifest-declared HTML spine content.
    Epub,
    /// WordprocessingML (`.docx`).
    Docx,
    /// PresentationML (`.pptx`).
    Pptx,
    /// SpreadsheetML (`.xlsx`).
    Spreadsheet,
    /// Delimiter-separated values.
    Csv,
    /// PDF with an extractable text layer.
    Pdf,
}

impl FormatKind {
    /// Stable identifier used in messages and tests.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlainText => "plain-text",
            Self::SourceCode => "source-code",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Epub => "epub",
            Self::Docx => "docx",
            Self::Pptx => "pptx",
            Self::Spreadsheet => "spreadsheet",
            Self::Csv => "csv",
            Self::Pdf => "pdf",
        }
    }
}

impl std::fmt::Display for FormatKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What a structural unit represents inside its document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    /// A heading; also contributes to the section path of following units.
    Heading,
    /// A prose paragraph or free-standing text block.
    Paragraph,
    /// One item of a list.
    ListItem,
    /// A fenced or indented code block, or a run of source code.
    CodeBlock,
    /// A table, or a bounded band of spreadsheet rows.
    Table,
    /// A quotation.
    Quote,
}

/// Where a unit came from, expressed only as precisely as the format allows.
///
/// There is no "page" variant for formats that have no pages: DOCX pagination
/// depends on a layout engine Procyon does not run, so DOCX units report a
/// block index rather than an invented page number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Provenance {
    /// Inclusive 1-based line range of a plain-text, Markdown or HTML source.
    TextLines {
        /// First line of the unit.
        start_line: u32,
        /// Last line of the unit.
        end_line: u32,
    },
    /// Inclusive 1-based line range of a source file, with the enclosing
    /// symbol name when one could be read directly from the text.
    CodeLines {
        /// First line of the unit.
        start_line: u32,
        /// Last line of the unit.
        end_line: u32,
        /// Symbol declared at the start of the unit, when recognizable.
        symbol: Option<String>,
    },
    /// A block of text on a 1-based PDF page.
    PdfBlock {
        /// 1-based page number.
        page_number: u32,
        /// 0-based index of the text block within the page.
        block_index: u32,
    },
    /// A shape's text on a 1-based slide.
    Slide {
        /// 1-based slide number in presentation order.
        slide_number: u32,
        /// 0-based index of the shape within the slide.
        shape_index: u32,
    },
    /// A rectangular range of spreadsheet cells, 0-based and inclusive.
    SpreadsheetRange {
        /// Sheet name, or the empty string for a single-sheet CSV.
        sheet: String,
        /// First row of the range.
        start_row: u32,
        /// First column of the range.
        start_column: u32,
        /// Last row of the range.
        end_row: u32,
        /// Last column of the range.
        end_column: u32,
    },
    /// A 0-based block index inside an OOXML word-processing part.
    DocxBlock {
        /// 0-based index in document body order.
        block_index: u32,
    },
    /// Inclusive 1-based line range in one EPUB spine item.
    EpubText {
        /// 0-based position of the resource in the declared EPUB spine.
        spine_index: u32,
        /// First line of the unit in the XHTML/HTML resource.
        start_line: u32,
        /// Last line of the unit in the XHTML/HTML resource.
        end_line: u32,
    },
}

/// The top-level boundary a unit belongs to.
///
/// The chunker never packs units from two different boundaries into one chunk:
/// a slide and the next slide, or two spreadsheet sheets, are separate
/// citations even when both are small.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TopLevelBoundary {
    /// The document as a whole (formats without a stronger boundary).
    Document,
    /// A 1-based PDF page.
    Page(u32),
    /// A 1-based slide.
    Slide(u32),
    /// A named spreadsheet sheet.
    Sheet(String),
    /// A top-level section, identified by its heading text.
    Section(String),
    /// A 0-based EPUB spine item.
    EpubSpine(u32),
}

/// One normalized piece of a converted document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralUnit {
    /// 0-based position in document order.
    pub order: u32,
    /// What the unit represents.
    pub kind: UnitKind,
    /// Which converter produced it.
    pub format: FormatKind,
    /// Heading hierarchy above the unit, outermost first.
    pub section_path: Vec<String>,
    /// Sanitized, normalized text.
    pub text: String,
    /// Where the text came from.
    pub provenance: Provenance,
    /// The top-level boundary the unit belongs to.
    pub boundary: TopLevelBoundary,
    /// Mapping from `text` offsets back to decoded-source offsets, where the
    /// format has a linear source text. Formats extracted from a package have
    /// an empty map.
    pub source_map: SourceMap,
    /// Whether the unit's text was truncated by the per-unit budget.
    pub truncated: bool,
}

/// A non-fatal observation about a conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionWarning {
    /// Bytes did not decode cleanly in the chosen encoding.
    LossyDecoding {
        /// Encoding that was used.
        encoding: String,
    },
    /// Invisible or control characters were removed by sanitization.
    RemovedInvisibleCharacters {
        /// How many characters were removed.
        count: u32,
    },
    /// A unit contains instruction-shaped text. The text is retained verbatim
    /// as untrusted evidence and must not be rewritten.
    InstructionLikeText {
        /// Order of the unit containing the text.
        unit_order: u32,
        /// Normalized excerpt of the matched text.
        excerpt: String,
    },
    /// A feature of the source that the baseline converter does not read.
    UnsupportedFeature {
        /// What was not read.
        detail: String,
    },
    /// OCR configuration and aggregate recognition confidence disclosed by an
    /// advanced converter.
    OcrAssessment {
        /// Stable language selection used by the OCR model.
        language: String,
        /// Mean OCR confidence in basis points (`10_000` = 1.0), when the
        /// backend could measure it.
        mean_confidence_basis_points: Option<u16>,
    },
}

/// Content that a bounded conversion deliberately left out.
///
/// Any non-empty list of omissions forces [`Completeness::Partial`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Omission {
    /// The unit budget was reached and later content was dropped.
    UnitLimit {
        /// The limit that was hit.
        limit: u32,
    },
    /// The total output budget was reached and later content was dropped.
    OutputCharLimit {
        /// The limit that was hit.
        limit: u64,
    },
    /// One unit's text was truncated.
    UnitTruncated {
        /// Order of the truncated unit.
        unit_order: u32,
        /// The per-unit character limit.
        limit: u32,
    },
    /// Some top-level items (pages, slides, sheets) were not converted.
    ItemsDropped {
        /// What kind of item was dropped, for example `page`.
        item: &'static str,
        /// How many were converted.
        converted: u32,
        /// How many the source contains.
        total: u32,
    },
    /// A part of the source could not be read, but the rest could.
    UnreadablePart {
        /// What could not be read.
        detail: String,
    },
}

/// Whether a converted document represents all of its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    /// Everything the converter supports was extracted.
    Complete,
    /// Bounded output dropped or truncated content; see the omissions.
    Partial,
}

/// A successfully converted document.
///
/// Construct through [`ConvertedDocument::new`], which derives
/// [`Completeness`] from the omissions so partial output can never be reported
/// as complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedDocument {
    converter: ComponentVersion,
    format: FormatKind,
    completeness: Completeness,
    units: Vec<StructuralUnit>,
    warnings: Vec<ConversionWarning>,
    omissions: Vec<Omission>,
}

impl ConvertedDocument {
    /// Builds a document, deriving completeness from `omissions`.
    #[must_use]
    pub fn new(
        converter: ComponentVersion,
        format: FormatKind,
        units: Vec<StructuralUnit>,
        warnings: Vec<ConversionWarning>,
        omissions: Vec<Omission>,
    ) -> Self {
        let completeness = if omissions.is_empty() {
            Completeness::Complete
        } else {
            Completeness::Partial
        };
        Self {
            converter,
            format,
            completeness,
            units,
            warnings,
            omissions,
        }
    }

    /// The converter that produced this document.
    #[must_use]
    pub fn converter(&self) -> ComponentVersion {
        self.converter
    }

    /// The format the converter recognized.
    #[must_use]
    pub fn format(&self) -> FormatKind {
        self.format
    }

    /// Whether the document is complete or explicitly partial.
    #[must_use]
    pub fn completeness(&self) -> Completeness {
        self.completeness
    }

    /// Whether bounded output dropped content.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        matches!(self.completeness, Completeness::Partial)
    }

    /// The structural units, in document order.
    #[must_use]
    pub fn units(&self) -> &[StructuralUnit] {
        &self.units
    }

    /// Non-fatal observations.
    #[must_use]
    pub fn warnings(&self) -> &[ConversionWarning] {
        &self.warnings
    }

    /// Content that was deliberately left out.
    #[must_use]
    pub fn omissions(&self) -> &[Omission] {
        &self.omissions
    }
}

/// Why a recognizable document was not converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The source contained no bytes.
    EmptySource,
    /// The source was recognized but contained no extractable text.
    NoTextContent,
}

impl SkipReason {
    /// Stable identifier used in messages and tests.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptySource => "empty source",
            Self::NoTextContent => "no text content",
        }
    }
}

/// The typed result of one conversion attempt.
///
/// Every variant is visible to the caller: there is no representation for
/// "succeeded with nothing extracted", because that is how silent data loss
/// enters a retrieval index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionOutcome {
    /// The document was converted. Bounded truncation is reported *inside*
    /// the document as [`Completeness::Partial`] plus omissions.
    Converted(ConvertedDocument),
    /// No baseline converter handles this media type.
    Unsupported {
        /// The media type that was resolved, when one could be.
        media_type: Option<MediaType>,
        /// Why the content is not handled.
        detail: String,
    },
    /// The document was recognized but deliberately not converted.
    Skipped {
        /// Why it was skipped.
        reason: SkipReason,
    },
    /// The container or markup is structurally broken.
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// The document is encrypted or password protected.
    Encrypted {
        /// What indicated encryption.
        detail: String,
    },
    /// The document is a recognized format with no text layer - a scanned or
    /// image-only PDF, for example. An OCR pack (task 0189) is what would
    /// convert this.
    NoTextLayer {
        /// What was inspected.
        detail: String,
    },
    /// A hard budget was exceeded before conversion could finish.
    OverBudget {
        /// Which budget stopped the conversion.
        budget: BudgetKind,
        /// The limit that was exceeded.
        limit: u64,
    },
    /// The caller cancelled the conversion.
    Cancelled,
}

impl ConversionOutcome {
    /// The converted document, when the outcome is [`Self::Converted`].
    #[must_use]
    pub fn document(&self) -> Option<&ConvertedDocument> {
        match self {
            Self::Converted(document) => Some(document),
            _ => None,
        }
    }

    /// Stable variant name, for logging and assertions.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Converted(_) => "converted",
            Self::Unsupported { .. } => "unsupported",
            Self::Skipped { .. } => "skipped",
            Self::Malformed { .. } => "malformed",
            Self::Encrypted { .. } => "encrypted",
            Self::NoTextLayer { .. } => "no-text-layer",
            Self::OverBudget { .. } => "over-budget",
            Self::Cancelled => "cancelled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_types_normalize_and_expose_their_charset() {
        let media = MediaType::parse("Text/Plain; charset=UTF-8");
        assert_eq!(media.as_str(), "text/plain");
        assert!(media.is_text());
        assert_eq!(
            MediaType::charset_of("text/plain; charset=\"iso-8859-1\""),
            Some("iso-8859-1".to_owned())
        );
    }

    #[test]
    fn metadata_never_carries_a_path_and_normalizes_its_hints() {
        let metadata = DocumentMetadata::unknown()
            .with_media_type("TEXT/MARKDOWN; charset=utf-8")
            .with_extension(".MD")
            .with_language_hint(" EN-GB ")
            .with_byte_length(12);
        assert_eq!(
            metadata.media_type().map(MediaType::as_str),
            Some("text/markdown")
        );
        assert_eq!(metadata.charset(), Some("utf-8"));
        assert_eq!(metadata.extension(), Some("md"));
        assert_eq!(metadata.language_hint(), Some("en-gb"));
        assert_eq!(metadata.byte_length(), Some(12));
    }

    #[test]
    fn omissions_force_partial_completeness() {
        let complete = ConvertedDocument::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::PlainText,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(complete.completeness(), Completeness::Complete);
        assert!(!complete.is_partial());

        let partial = ConvertedDocument::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::PlainText,
            Vec::new(),
            Vec::new(),
            vec![Omission::UnitLimit { limit: 4 }],
        );
        assert_eq!(partial.completeness(), Completeness::Partial);
        assert!(partial.is_partial());
    }
}
