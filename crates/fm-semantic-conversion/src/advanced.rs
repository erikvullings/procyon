//! Optional advanced converter adapter and baseline-safe fallback chain.

use std::collections::BTreeSet;
use std::io::Read;
use std::sync::Arc;

use crate::{
    BudgetKind, ComponentVersion, ConversionContext, ConversionError, ConversionOutcome,
    ConvertedDocument, DocumentConverter, DocumentMetadata, SourceContent,
};

/// Independently disclosed capability supplied by an advanced converter pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdvancedCapability {
    /// Optical character recognition for image-only content.
    Ocr,
    /// Reading-order and region-aware complex layout extraction.
    ComplexLayout,
    /// Table structure beyond baseline extraction.
    Tables,
    /// Optional image interpretation using a local vision-language model.
    ImageInterpretation,
}

/// Honest precision of the strongest provenance emitted by an advanced pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenancePrecision {
    /// Exact page, region, cell, or structural block.
    Exact,
    /// Approximate page or region supplied by the pack.
    Approximate,
    /// Only the containing file is known.
    FileOnly,
}

/// Typed result from an isolated advanced converter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancedConversion {
    /// Standard task-0180 conversion outcome.
    pub outcome: ConversionOutcome,
    /// Precision callers must expose with citations.
    pub provenance_precision: ProvenancePrecision,
}

/// Isolated pack boundary: bytes and trusted metadata only, never a path or network handle.
pub trait AdvancedConverterBackend: Send + Sync {
    /// Immutable converter identity covered by the installed pack manifest.
    fn version(&self) -> ComponentVersion;

    /// Independently disclosed pack features.
    fn capabilities(&self) -> &[AdvancedCapability];

    /// Converts caller-provided bounded bytes under the task-0180 context.
    fn convert_bytes(
        &self,
        bytes: &[u8],
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<AdvancedConversion, ConversionError>;
}

/// Validating adapter that implements the baseline [`DocumentConverter`] contract.
pub struct AdvancedConverterAdapter {
    backend: Arc<dyn AdvancedConverterBackend>,
}

impl AdvancedConverterAdapter {
    /// Wraps one installed advanced pack.
    #[must_use]
    pub fn new(backend: Arc<dyn AdvancedConverterBackend>) -> Self {
        Self { backend }
    }

    /// Converts content and retains provenance-precision reporting.
    pub fn convert_with_report(
        &self,
        content: SourceContent<'_>,
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<AdvancedConversion, ConversionError> {
        let bytes = match bounded_bytes(content, metadata, context)? {
            BoundedBytes::Bytes(bytes) => bytes,
            BoundedBytes::Outcome(outcome) => {
                return Ok(AdvancedConversion {
                    outcome,
                    provenance_precision: ProvenancePrecision::FileOnly,
                });
            }
        };
        if context.is_cancelled() {
            return Ok(cancelled());
        }
        let capabilities = self.backend.capabilities();
        if capabilities.is_empty()
            || capabilities.iter().copied().collect::<BTreeSet<_>>().len() != capabilities.len()
        {
            return Ok(malformed(
                "advanced converter capability manifest is invalid",
            ));
        }
        let mut converted = self.backend.convert_bytes(&bytes, metadata, context)?;
        if context.is_cancelled() {
            return Ok(cancelled());
        }
        if context.elapsed() > context.budgets().timeout {
            converted.outcome = ConversionOutcome::OverBudget {
                budget: BudgetKind::Time,
                limit: context
                    .budgets()
                    .timeout
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
            };
            return Ok(converted);
        }
        if let ConversionOutcome::Converted(document) = &converted.outcome
            && !valid_document(document, context)
        {
            return Ok(malformed(
                "advanced converter returned malformed or over-budget output",
            ));
        }
        Ok(converted)
    }
}

impl DocumentConverter for AdvancedConverterAdapter {
    fn version(&self) -> ComponentVersion {
        self.backend.version()
    }

    fn convert(
        &self,
        content: SourceContent<'_>,
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<ConversionOutcome, ConversionError> {
        Ok(self
            .convert_with_report(content, metadata, context)?
            .outcome)
    }
}

/// Selection policy for an installed advanced converter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdvancedSelection {
    BaselineGaps,
    Preferred,
}

/// Converter that keeps the baseline available while an optional pack is active.
pub struct OptionalConverter {
    baseline: Arc<dyn DocumentConverter>,
    advanced: Option<Arc<AdvancedConverterAdapter>>,
    selection: AdvancedSelection,
}

impl OptionalConverter {
    /// Creates a removable advanced layer over the always-available baseline.
    #[must_use]
    pub fn new(
        baseline: Arc<dyn DocumentConverter>,
        advanced: Option<Arc<AdvancedConverterAdapter>>,
    ) -> Self {
        Self {
            baseline,
            advanced,
            selection: AdvancedSelection::BaselineGaps,
        }
    }

    /// Creates a converter that tries the installed advanced Implementation
    /// first and falls back to the baseline on absence, incompatibility, or a
    /// recoverable runtime failure.
    #[must_use]
    pub fn prefer_advanced(
        baseline: Arc<dyn DocumentConverter>,
        advanced: Option<Arc<AdvancedConverterAdapter>>,
    ) -> Self {
        Self {
            baseline,
            advanced,
            selection: AdvancedSelection::Preferred,
        }
    }
}

impl DocumentConverter for OptionalConverter {
    fn version(&self) -> ComponentVersion {
        self.advanced
            .as_ref()
            .map_or_else(|| self.baseline.version(), |advanced| advanced.version())
    }

    fn convert(
        &self,
        content: SourceContent<'_>,
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<ConversionOutcome, ConversionError> {
        let bytes = match bounded_bytes(content, metadata, context)? {
            BoundedBytes::Bytes(bytes) => bytes,
            BoundedBytes::Outcome(outcome) => return Ok(outcome),
        };
        if self.selection == AdvancedSelection::Preferred
            && let Some(advanced) = &self.advanced
        {
            let advanced_outcome = advanced
                .convert_with_report(SourceContent::Bytes(&bytes), metadata, context)?
                .outcome;
            if matches!(
                advanced_outcome,
                ConversionOutcome::Converted(_)
                    | ConversionOutcome::Cancelled
                    | ConversionOutcome::NoTextLayer { .. }
                    | ConversionOutcome::OverBudget { .. }
                    | ConversionOutcome::Encrypted { .. }
            ) {
                return Ok(advanced_outcome);
            }
            let baseline =
                self.baseline
                    .convert(SourceContent::Bytes(&bytes), metadata, context)?;
            return Ok(match (&advanced_outcome, &baseline) {
                (_, ConversionOutcome::Converted(_))
                | (ConversionOutcome::Unsupported { .. }, _) => baseline,
                _ => advanced_outcome,
            });
        }
        let baseline = self
            .baseline
            .convert(SourceContent::Bytes(&bytes), metadata, context)?;
        if !matches!(
            baseline,
            ConversionOutcome::Unsupported { .. } | ConversionOutcome::NoTextLayer { .. }
        ) {
            return Ok(baseline);
        }
        let Some(advanced) = &self.advanced else {
            return Ok(baseline);
        };
        Ok(advanced
            .convert_with_report(SourceContent::Bytes(&bytes), metadata, context)?
            .outcome)
    }
}

enum BoundedBytes {
    Bytes(Vec<u8>),
    Outcome(ConversionOutcome),
}

fn bounded_bytes(
    content: SourceContent<'_>,
    metadata: &DocumentMetadata,
    context: &ConversionContext,
) -> Result<BoundedBytes, ConversionError> {
    let maximum = context.budgets().max_source_bytes;
    if metadata
        .byte_length()
        .is_some_and(|length| length > maximum)
    {
        return Ok(BoundedBytes::Outcome(ConversionOutcome::OverBudget {
            budget: BudgetKind::SourceBytes,
            limit: maximum,
        }));
    }
    let bytes = match content {
        SourceContent::Bytes(bytes) => bytes.to_vec(),
        SourceContent::Reader(reader) => {
            let mut bytes = Vec::new();
            reader
                .take(maximum.saturating_add(1))
                .read_to_end(&mut bytes)?;
            bytes
        }
    };
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum {
        return Ok(BoundedBytes::Outcome(ConversionOutcome::OverBudget {
            budget: BudgetKind::SourceBytes,
            limit: maximum,
        }));
    }
    Ok(BoundedBytes::Bytes(bytes))
}

fn valid_document(document: &ConvertedDocument, context: &ConversionContext) -> bool {
    let budgets = context.budgets();
    if document.units().is_empty()
        || document.units().len() > budgets.max_units as usize
        || document.converter().revision == 0
    {
        return false;
    }
    let mut output_chars = 0_u64;
    for (index, unit) in document.units().iter().enumerate() {
        let unit_chars = unit.text.chars().count() as u64;
        if unit.order as usize != index
            || unit.format != document.format()
            || unit.text.trim().is_empty()
            || unit_chars > u64::from(budgets.max_unit_chars)
        {
            return false;
        }
        let Some(total) = output_chars.checked_add(unit_chars) else {
            return false;
        };
        output_chars = total;
    }
    output_chars <= budgets.max_output_chars
}

fn cancelled() -> AdvancedConversion {
    AdvancedConversion {
        outcome: ConversionOutcome::Cancelled,
        provenance_precision: ProvenancePrecision::FileOnly,
    }
}

fn malformed(detail: &str) -> AdvancedConversion {
    AdvancedConversion {
        outcome: ConversionOutcome::Malformed {
            detail: detail.to_owned(),
        },
        provenance_precision: ProvenancePrecision::FileOnly,
    }
}
