//! Optional `docling.rs` PDF conversion Adapter.
//!
//! This Module is deliberately separate from `fm-semantic-conversion`: ordinary
//! indexing keeps the pure-Rust baseline and does not link the optional
//! PDFium/ONNX implementation. The `ml` feature enables those native
//! dependencies for managed advanced-pack builds.

use docling_core::{DoclingDocument, FieldItem, Node, Table};
use docling_pdf::{PdfError, convert_text_layer_pages};
use fm_semantic_conversion::{
    AdvancedCapability, AdvancedConversion, AdvancedConverterBackend, ComponentVersion,
    ConversionContext, ConversionError, ConversionOutcome, ConversionWarning, ConvertedDocument,
    DocumentMetadata, FormatKind, Omission, Provenance, ProvenancePrecision, SkipReason, SourceMap,
    StructuralUnit, TopLevelBoundary, UnitKind, instruction_like_excerpt, sanitize,
};

/// Version of the Docling extraction and Procyon structural mapping behavior.
pub const DOCLING_PDF_CONVERTER_VERSION: ComponentVersion =
    ComponentVersion::new("docling-pdf", 1_036_000);

/// OCR language bundled by the audited Docling release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrLanguage {
    /// English recognition model.
    English,
    /// Docling's multilingual Chinese/Latin recognition model.
    Multilingual,
}

impl OcrLanguage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Multilingual => "ch",
        }
    }
}

/// Failure to initialize the managed ML pipeline.
#[cfg(feature = "ml")]
#[derive(Debug)]
pub struct DoclingInitError(String);

#[cfg(feature = "ml")]
impl std::fmt::Display for DoclingInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(feature = "ml")]
impl std::error::Error for DoclingInitError {}

/// Docling PDF backend with deterministic and managed-ML modes.
pub struct DoclingPdfBackend {
    #[cfg(feature = "ml")]
    pipeline: Option<std::sync::Mutex<docling_pdf::Pipeline>>,
    #[cfg(feature = "ml")]
    ocr_language: Option<OcrLanguage>,
}

impl std::fmt::Debug for DoclingPdfBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("DoclingPdfBackend");
        #[cfg(feature = "ml")]
        debug.field("ml", &self.pipeline.is_some());
        debug.finish_non_exhaustive()
    }
}

impl Default for DoclingPdfBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DoclingPdfBackend {
    /// Creates the deterministic backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(feature = "ml")]
            pipeline: None,
            #[cfg(feature = "ml")]
            ocr_language: None,
        }
    }

    /// Creates the layout/OCR/table backend used by a managed advanced pack.
    ///
    /// The caller must set Docling's model and PDFium paths to files from the
    /// verified pack before construction; this function performs no download.
    #[cfg(feature = "ml")]
    pub fn try_ml(language: OcrLanguage) -> Result<Self, DoclingInitError> {
        let mut missing = docling_pdf::model_inventory()
            .into_iter()
            .filter(|asset| asset.stage != "ocr.rec" && asset.stage != "ocr.dict")
            .filter(|asset| !asset.found)
            .map(|asset| format!("{} ({})", asset.stage, asset.path))
            .collect::<Vec<_>>();
        for (stage, path) in requested_ocr_assets(language) {
            if !std::path::Path::new(&path).is_file() {
                missing.push(format!("{stage} ({path})"));
            }
        }
        if !missing.is_empty() {
            return Err(DoclingInitError(format!(
                "the verified Docling pack is incomplete; missing {}",
                missing.join(", ")
            )));
        }
        let mut pipeline =
            docling_pdf::Pipeline::new().map_err(|error| DoclingInitError(error.to_string()))?;
        let upstream_language = match language {
            OcrLanguage::English => docling_pdf::OcrLang::En,
            OcrLanguage::Multilingual => docling_pdf::OcrLang::Ch,
        };
        pipeline.set_ocr_lang(Some(upstream_language));
        pipeline.set_heading_hierarchy(docling_pdf::HeadingHierarchyOptions::enabled(true));
        Ok(Self {
            pipeline: Some(std::sync::Mutex::new(pipeline)),
            ocr_language: Some(language),
        })
    }
}

#[cfg(feature = "ml")]
fn requested_ocr_assets(language: OcrLanguage) -> [(&'static str, String); 2] {
    let (recognizer, dictionary) = match language {
        OcrLanguage::English => (".models/ocr_rec_en.onnx", ".models/en_dict.txt"),
        OcrLanguage::Multilingual => (".models/ocr_rec.onnx", ".models/ppocr_keys_v1.txt"),
    };
    [
        (
            "ocr.rec",
            docling_core::env::nonempty("DOCLING_OCR_REC_ONNX")
                .unwrap_or_else(|| docling_core::assets::resolve(recognizer)),
        ),
        (
            "ocr.dict",
            docling_core::env::nonempty("DOCLING_OCR_DICT")
                .unwrap_or_else(|| docling_core::assets::resolve(dictionary)),
        ),
    ]
}

impl AdvancedConverterBackend for DoclingPdfBackend {
    fn version(&self) -> ComponentVersion {
        DOCLING_PDF_CONVERTER_VERSION
    }

    fn capabilities(&self) -> &[AdvancedCapability] {
        #[cfg(feature = "ml")]
        if self.pipeline.is_some() {
            return &[
                AdvancedCapability::Ocr,
                AdvancedCapability::ComplexLayout,
                AdvancedCapability::Tables,
            ];
        }
        &[AdvancedCapability::ComplexLayout]
    }

    fn convert_bytes(
        &self,
        bytes: &[u8],
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<AdvancedConversion, ConversionError> {
        if !is_pdf(bytes, metadata) {
            return Ok(AdvancedConversion {
                outcome: ConversionOutcome::Unsupported {
                    media_type: metadata.media_type().cloned(),
                    detail: "the Docling Adapter accepts PDF documents only".into(),
                },
                provenance_precision: ProvenancePrecision::Approximate,
            });
        }

        #[cfg(feature = "ml")]
        if let Some(pipeline) = &self.pipeline {
            return Ok(convert_ml(
                bytes,
                context,
                self.version(),
                pipeline,
                self.ocr_language.expect("ML mode has a language"),
            ));
        }

        Ok(convert_text_layer(bytes, context, self.version()))
    }
}

fn convert_text_layer(
    bytes: &[u8],
    context: &ConversionContext,
    version: ComponentVersion,
) -> AdvancedConversion {
    if let Some(outcome) = stopped(context) {
        return conversion(outcome);
    }

    let pdf = match lopdf::Document::load_mem(bytes) {
        Ok(pdf) => pdf,
        Err(error) => {
            return conversion(ConversionOutcome::Malformed {
                detail: format!("Docling could not parse the PDF container: {error}"),
            });
        }
    };
    if pdf.is_encrypted() {
        return conversion(ConversionOutcome::Encrypted {
            detail: "Docling does not open password-protected PDFs".into(),
        });
    }

    let total_pages = pdf.get_pages().len();
    if total_pages > 0 && context.budgets().max_items == 0 {
        return conversion(ConversionOutcome::OverBudget {
            budget: fm_semantic_conversion::BudgetKind::Items,
            limit: 0,
        });
    }
    let selected_pages = total_pages.min(context.budgets().max_items as usize);
    let range = (selected_pages > 0).then_some((1, selected_pages));
    let document = match convert_text_layer_pages(bytes, "document.pdf", range) {
        Ok(document) => document,
        Err(error) => return conversion(pdf_error_outcome(error)),
    };
    if document.nodes.is_empty() {
        return conversion(ConversionOutcome::Skipped {
            reason: SkipReason::NoTextContent,
        });
    }

    let mut mapper = Mapper::new(version, context, total_pages, selected_pages, None);
    mapper.map_document(document);
    conversion(mapper.finish())
}

#[cfg(feature = "ml")]
fn convert_ml(
    bytes: &[u8],
    context: &ConversionContext,
    version: ComponentVersion,
    pipeline: &std::sync::Mutex<docling_pdf::Pipeline>,
    language: OcrLanguage,
) -> AdvancedConversion {
    if let Some(outcome) = stopped(context) {
        return conversion(outcome);
    }
    let pdf = match lopdf::Document::load_mem(bytes) {
        Ok(pdf) => pdf,
        Err(error) => {
            return conversion(ConversionOutcome::Malformed {
                detail: format!("Docling could not parse the PDF container: {error}"),
            });
        }
    };
    if pdf.is_encrypted() {
        return conversion(ConversionOutcome::Encrypted {
            detail: "Docling does not open password-protected PDFs".into(),
        });
    }
    let total_pages = pdf.get_pages().len();
    if total_pages > 0 && context.budgets().max_items == 0 {
        return conversion(ConversionOutcome::OverBudget {
            budget: fm_semantic_conversion::BudgetKind::Items,
            limit: 0,
        });
    }
    let selected_pages = total_pages.min(context.budgets().max_items as usize);
    let mut pipeline = loop {
        match pipeline.try_lock() {
            Ok(pipeline) => break pipeline,
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return conversion(ConversionOutcome::Malformed {
                    detail: "the Docling inference pipeline became unavailable".into(),
                });
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                if let Some(outcome) = stopped(context) {
                    return conversion(outcome);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    };
    let mut combined = DoclingDocument::new("document.pdf");
    let mut omissions = Vec::new();
    let mut first_error = None;
    for page in 1..=selected_pages {
        if let Some(outcome) = stopped(context) {
            pipeline.set_pages(None);
            return conversion(outcome);
        }
        // One-page windows bound resident raster/tensor memory and create a
        // cancellation checkpoint around every inference unit.
        pipeline.set_pages(Some((page, page)));
        match pipeline.convert(bytes, None, "document.pdf") {
            Ok(mut document) => {
                combined.nodes.append(&mut document.nodes);
                combined.links.append(&mut document.links);
                if let Some(confidence) = document.confidence {
                    let target = combined.confidence.get_or_insert_with(Default::default);
                    target.pages.extend(confidence.pages);
                }
            }
            Err(error) => {
                let detail = format!("Docling could not convert PDF page {page}: {error}");
                if first_error.is_none() {
                    first_error = Some(error);
                }
                omissions.push(Omission::UnreadablePart { detail });
            }
        }
    }
    pipeline.set_pages(None);
    if combined.nodes.is_empty()
        && let Some(error) = first_error
    {
        return conversion(pdf_error_outcome(error));
    }

    let mut mapper = Mapper::new(
        version,
        context,
        total_pages,
        selected_pages,
        Some(language),
    );
    for omission in omissions {
        mapper.omit(omission);
    }
    mapper.map_document(combined);
    conversion(mapper.finish())
}

fn conversion(outcome: ConversionOutcome) -> AdvancedConversion {
    AdvancedConversion {
        outcome,
        // Procyon's current provenance represents the exact page and reading-order
        // block, but not Docling's region coordinates.
        provenance_precision: ProvenancePrecision::Approximate,
    }
}

fn pdf_error_outcome(error: PdfError) -> ConversionOutcome {
    let detail = error.to_string();
    if detail.to_ascii_lowercase().contains("password")
        || detail.to_ascii_lowercase().contains("encrypted")
    {
        ConversionOutcome::Encrypted { detail }
    } else {
        ConversionOutcome::Malformed { detail }
    }
}

fn stopped(context: &ConversionContext) -> Option<ConversionOutcome> {
    if context.is_cancelled() {
        Some(ConversionOutcome::Cancelled)
    } else if context.elapsed() > context.budgets().timeout {
        Some(ConversionOutcome::OverBudget {
            budget: fm_semantic_conversion::BudgetKind::Time,
            limit: context.budgets().timeout.as_millis() as u64,
        })
    } else {
        None
    }
}

struct Mapper<'a> {
    version: ComponentVersion,
    context: &'a ConversionContext,
    total_pages: usize,
    current_page: u32,
    block_index: u32,
    section_path: Vec<String>,
    units: Vec<StructuralUnit>,
    warnings: Vec<ConversionWarning>,
    omissions: Vec<Omission>,
    output_chars: u64,
    removed_characters: u32,
    saturated: bool,
    stopped: Option<ConversionOutcome>,
    ocr_language: Option<OcrLanguage>,
}

impl<'a> Mapper<'a> {
    fn new(
        version: ComponentVersion,
        context: &'a ConversionContext,
        total_pages: usize,
        selected_pages: usize,
        ocr_language: Option<OcrLanguage>,
    ) -> Self {
        let omissions = (selected_pages < total_pages)
            .then_some(Omission::ItemsDropped {
                item: "page",
                converted: selected_pages as u32,
                total: total_pages as u32,
            })
            .into_iter()
            .collect();
        Self {
            version,
            context,
            total_pages,
            current_page: 1,
            block_index: 0,
            section_path: Vec::new(),
            units: Vec::new(),
            warnings: Vec::new(),
            omissions,
            output_chars: 0,
            removed_characters: 0,
            saturated: false,
            stopped: None,
            ocr_language,
        }
    }

    fn map_document(&mut self, document: DoclingDocument) {
        if let Some(language) = self.ocr_language {
            let confidence = document
                .confidence
                .as_ref()
                .and_then(docling_core::confidence::ConfidenceReport::ocr_score)
                .map(Self::confidence_basis_points);
            self.warn(ConversionWarning::OcrAssessment {
                language: language.as_str().to_owned(),
                mean_confidence_basis_points: confidence,
            });
        }
        for node in document.nodes {
            if self.saturated || self.checkpoint().is_err() {
                break;
            }
            self.map_node(node, 0);
        }
    }

    fn confidence_basis_points(score: f64) -> u16 {
        (score.clamp(0.0, 1.0) * 10_000.0).round() as u16
    }

    fn map_node(&mut self, node: Node, depth: u32) {
        if self.saturated || self.checkpoint().is_err() {
            return;
        }
        if depth > self.context.budgets().max_nesting_depth {
            self.stopped = Some(ConversionOutcome::OverBudget {
                budget: fm_semantic_conversion::BudgetKind::NestingDepth,
                limit: u64::from(self.context.budgets().max_nesting_depth),
            });
            return;
        }
        match node {
            Node::PageInfo { page_no, .. } => {
                self.current_page = u32::try_from(page_no).unwrap_or(u32::MAX).max(1);
                self.block_index = 0;
            }
            Node::PageBreak => {}
            Node::Heading { level, text } => self.heading(level, text),
            Node::Paragraph { text } | Node::TextDump(text) => {
                if !self.is_page_number(&text) {
                    self.emit(UnitKind::Paragraph, text);
                }
            }
            Node::InlineGroup { md_text, .. } => self.emit(UnitKind::Paragraph, md_text),
            Node::CheckboxItem { checked, text } => {
                self.emit(
                    UnitKind::ListItem,
                    format!("[{}] {text}", if checked { "x" } else { " " }),
                );
            }
            Node::ListItem { text, .. } => self.emit(UnitKind::ListItem, text),
            Node::Code { text, pretty, .. } => {
                self.emit(UnitKind::CodeBlock, pretty.unwrap_or(text));
            }
            Node::Formula { latex, .. } => self.emit(UnitKind::CodeBlock, latex),
            Node::Table(table) => self.emit(UnitKind::Table, table_text(&table)),
            Node::Chart { caption, table, .. } => {
                let mut text = caption.unwrap_or_default();
                let rows = table_text(&table);
                if !text.is_empty() && !rows.is_empty() {
                    text.push('\n');
                }
                text.push_str(&rows);
                self.emit(UnitKind::Table, text);
            }
            Node::Picture { caption, .. } => {
                if let Some(caption) = caption {
                    self.emit(UnitKind::Paragraph, caption);
                } else {
                    self.warn(ConversionWarning::UnsupportedFeature {
                        detail: "an uncaptioned PDF image was not added to semantic text".into(),
                    });
                }
            }
            Node::FieldRegion { items } => {
                self.emit(UnitKind::Paragraph, field_region_text(&items));
            }
            Node::Group {
                layer, children, ..
            } => {
                if layer.is_none() {
                    for child in children {
                        self.map_node(child, depth.saturating_add(1));
                    }
                }
            }
            Node::Located { inner, .. } | Node::Commented { inner, .. } => {
                self.map_node(*inner, depth.saturating_add(1));
            }
            Node::Furniture { .. } | Node::PageFurniture { .. } | Node::CommentSection { .. } => {}
            Node::DoclangOnly(_) => self.warn(ConversionWarning::UnsupportedFeature {
                detail: "DocLang-only PDF content was not added to semantic text".into(),
            }),
        }
    }

    fn heading(&mut self, level: u8, text: String) {
        let level = usize::from(level.clamp(1, 6));
        self.section_path.truncate(level.saturating_sub(1));
        let parent_path = self.section_path.clone();
        if self.emit_with_path(UnitKind::Heading, text, parent_path)
            && let Some(heading) = self.units.last()
        {
            self.section_path.push(heading.text.clone());
        }
    }

    fn emit(&mut self, kind: UnitKind, text: String) {
        self.emit_with_path(kind, text, self.section_path.clone());
    }

    fn emit_with_path(&mut self, kind: UnitKind, text: String, section_path: Vec<String>) -> bool {
        if self.saturated || self.checkpoint().is_err() {
            return false;
        }
        if self.units.len() as u64 >= u64::from(self.context.budgets().max_units) {
            self.saturated = true;
            self.omit(Omission::UnitLimit {
                limit: self.context.budgets().max_units,
            });
            return false;
        }

        let sanitized = sanitize(text.trim(), 0);
        self.removed_characters = self.removed_characters.saturating_add(sanitized.removed);
        if sanitized.text.trim().is_empty() {
            return false;
        }
        let order = self.units.len() as u32;
        let mut text = sanitized.text;
        let mut truncated = false;
        let unit_limit = self.context.budgets().max_unit_chars as usize;
        if text.chars().count() > unit_limit {
            text = text.chars().take(unit_limit).collect();
            truncated = true;
            self.omit(Omission::UnitTruncated {
                unit_order: order,
                limit: self.context.budgets().max_unit_chars,
            });
        }

        let remaining = self
            .context
            .budgets()
            .max_output_chars
            .saturating_sub(self.output_chars);
        if text.chars().count() as u64 > remaining {
            self.saturated = true;
            self.omit(Omission::OutputCharLimit {
                limit: self.context.budgets().max_output_chars,
            });
            let keep = usize::try_from(remaining).unwrap_or(usize::MAX);
            if keep == 0 {
                return false;
            }
            text = text.chars().take(keep).collect();
            truncated = true;
        }
        self.output_chars = self
            .output_chars
            .saturating_add(text.chars().count() as u64);
        if let Some(excerpt) = instruction_like_excerpt(&text) {
            self.warn(ConversionWarning::InstructionLikeText {
                unit_order: order,
                excerpt,
            });
        }

        self.units.push(StructuralUnit {
            order,
            kind,
            format: FormatKind::Pdf,
            section_path,
            text,
            provenance: Provenance::PdfBlock {
                page_number: self.current_page,
                block_index: self.block_index,
            },
            boundary: TopLevelBoundary::Page(self.current_page),
            source_map: SourceMap::default(),
            truncated,
        });
        self.block_index = self.block_index.saturating_add(1);
        true
    }

    fn checkpoint(&mut self) -> Result<(), ()> {
        if let Some(outcome) = stopped(self.context) {
            self.stopped = Some(outcome);
            Err(())
        } else {
            Ok(())
        }
    }

    fn is_page_number(&self, text: &str) -> bool {
        text.trim().parse::<usize>().ok().is_some_and(|number| {
            number == self.current_page as usize && number <= self.total_pages
        })
    }

    fn warn(&mut self, warning: ConversionWarning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    fn omit(&mut self, omission: Omission) {
        if !self.omissions.contains(&omission) {
            self.omissions.push(omission);
        }
    }

    fn finish(mut self) -> ConversionOutcome {
        if let Some(outcome) = self.stopped {
            return outcome;
        }
        if self.removed_characters > 0 {
            let count = self.removed_characters;
            self.warn(ConversionWarning::RemovedInvisibleCharacters { count });
        }
        if self.units.is_empty() {
            return ConversionOutcome::Skipped {
                reason: SkipReason::NoTextContent,
            };
        }
        ConversionOutcome::Converted(ConvertedDocument::new(
            self.version,
            FormatKind::Pdf,
            self.units,
            self.warnings,
            self.omissions,
        ))
    }
}

fn table_text(table: &Table) -> String {
    let mut lines = Vec::new();
    if let Some(caption) = table
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        lines.push(caption.to_owned());
    }
    lines.extend(table.rows.iter().map(|row| {
        row.iter()
            .map(|cell| cell.replace(['\n', '\t'], " ").trim().to_owned())
            .collect::<Vec<_>>()
            .join("\t")
    }));
    lines.join("\n")
}

fn field_region_text(items: &[FieldItem]) -> String {
    items
        .iter()
        .filter_map(|item| {
            let mut parts = Vec::new();
            if let Some(marker) = item
                .marker
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(marker.to_owned());
            }
            if let Some(key) = item.key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                parts.push(key.to_owned());
            }
            let mut text = parts.join(" ");
            if let Some(value) = item
                .value
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if !text.is_empty() {
                    text.push_str(": ");
                }
                text.push_str(value);
            }
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_pdf(bytes: &[u8], metadata: &DocumentMetadata) -> bool {
    bytes.starts_with(b"%PDF-")
        || metadata
            .media_type()
            .is_some_and(|media_type| media_type.as_str() == "application/pdf")
        || metadata.extension() == Some("pdf")
}

#[cfg(test)]
mod tests {
    use super::*;
    use docling_core::Table;

    #[test]
    fn maps_reading_order_sections_tables_and_page_provenance() {
        let document = DoclingDocument {
            name: "fixture".into(),
            nodes: vec![
                Node::PageInfo {
                    page_no: 1,
                    width: 612.0,
                    height: 792.0,
                },
                Node::Heading {
                    level: 1,
                    text: "Methods".into(),
                },
                Node::Paragraph {
                    text: "First column before second column.".into(),
                },
                Node::ListItem {
                    ordered: false,
                    number: 0,
                    first_in_list: true,
                    text: "Measured result".into(),
                    level: 0,
                    marker: None,
                    location: None,
                    dclx: None,
                    href: None,
                    layer: None,
                },
                Node::Table(Table {
                    rows: vec![
                        vec!["Metric".into(), "Value".into()],
                        vec!["Accuracy".into(), "98%".into()],
                    ],
                    ..Table::default()
                }),
                Node::PageFurniture {
                    footer: true,
                    location: [0, 0, 511, 10],
                    text: "Repeated footer".into(),
                },
                Node::PageInfo {
                    page_no: 2,
                    width: 612.0,
                    height: 792.0,
                },
                Node::Paragraph { text: "2".into() },
                Node::Paragraph {
                    text: "Continuation.".into(),
                },
            ],
            strict_markdown: false,
            compact_tables: false,
            links: Vec::new(),
            confidence: None,
        };
        let context = ConversionContext::new();
        let mut mapper = Mapper::new(DOCLING_PDF_CONVERTER_VERSION, &context, 2, 2, None);
        mapper.map_document(document);
        let outcome = mapper.finish();
        let converted = outcome.document().expect("converted");

        assert_eq!(converted.units().len(), 5);
        assert_eq!(converted.units()[1].section_path, ["Methods"]);
        assert_eq!(converted.units()[3].kind, UnitKind::Table);
        assert_eq!(converted.units()[3].text, "Metric\tValue\nAccuracy\t98%");
        assert_eq!(
            converted.units()[4].provenance,
            Provenance::PdfBlock {
                page_number: 2,
                block_index: 0
            }
        );
        assert!(
            converted
                .units()
                .iter()
                .all(|unit| !unit.text.contains("Repeated footer"))
        );
    }

    #[test]
    fn mapping_enforces_output_limits() {
        let context =
            ConversionContext::new().with_budgets(fm_semantic_conversion::ConversionBudgets {
                max_unit_chars: 4,
                max_output_chars: 6,
                ..fm_semantic_conversion::ConversionBudgets::default()
            });
        let document = DoclingDocument {
            name: "fixture".into(),
            nodes: vec![
                Node::Paragraph {
                    text: "abcdef".into(),
                },
                Node::Paragraph {
                    text: "ghijkl".into(),
                },
            ],
            strict_markdown: false,
            compact_tables: false,
            links: Vec::new(),
            confidence: None,
        };
        let mut mapper = Mapper::new(DOCLING_PDF_CONVERTER_VERSION, &context, 1, 1, None);
        mapper.map_document(document);
        let outcome = mapper.finish();
        let converted = outcome.document().expect("converted");

        assert_eq!(converted.units()[0].text, "abcd");
        assert_eq!(converted.units()[1].text, "gh");
        assert!(converted.is_partial());
    }

    #[test]
    fn heading_paths_are_sanitized_and_ocr_confidence_is_visible() {
        let mut pages = std::collections::BTreeMap::new();
        pages.insert(
            1,
            docling_core::confidence::PageConfidence {
                ocr_score: Some(0.8765),
                ..docling_core::confidence::PageConfidence::default()
            },
        );
        let document = DoclingDocument {
            name: "fixture".into(),
            nodes: vec![
                Node::Heading {
                    level: 1,
                    text: "Safe\u{202e} heading".into(),
                },
                Node::Paragraph {
                    text: "Body".into(),
                },
            ],
            strict_markdown: false,
            compact_tables: false,
            links: Vec::new(),
            confidence: Some(docling_core::confidence::ConfidenceReport::from_pages(
                pages,
            )),
        };
        let context = ConversionContext::new();
        let mut mapper = Mapper::new(
            DOCLING_PDF_CONVERTER_VERSION,
            &context,
            1,
            1,
            Some(OcrLanguage::English),
        );
        mapper.map_document(document);
        let outcome = mapper.finish();
        let converted = outcome.document().expect("converted");

        assert_eq!(converted.units()[1].section_path, ["Safe heading"]);
        assert!(
            converted
                .warnings()
                .contains(&ConversionWarning::OcrAssessment {
                    language: "en".into(),
                    mean_confidence_basis_points: Some(8765),
                })
        );
    }
}
