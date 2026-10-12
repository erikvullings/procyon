//! Bounded, path-free PDF text evidence passed from an isolated converter.

use serde::{Deserialize, Serialize};

use crate::{
    BASELINE_CONVERTER_VERSION, ConversionBudgets, ConversionWarning, ConvertedDocument,
    FormatKind, Provenance, SourceMap, StructuralUnit, TopLevelBoundary, UnitKind,
};

/// A conversion child cannot claim a complete PDF document with invalid evidence.
#[derive(Debug, thiserror::Error)]
#[error("isolated PDF text report is invalid: {0}")]
pub struct InvalidPdfTextReport(pub String);

/// A complete text-only PDF result; the source digest is checked separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfTextReport {
    total_pages: u32,
    units: Vec<PdfTextUnit>,
    warnings: Vec<ConversionWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PdfTextUnit {
    order: u32,
    kind: UnitKind,
    section_path: Vec<String>,
    text: String,
    page_number: u32,
    block_index: u32,
}

impl PdfTextReport {
    /// Extracts only complete baseline PDF text, without inventing page evidence.
    pub fn from_document(
        total_pages: u32,
        document: &ConvertedDocument,
        budgets: &ConversionBudgets,
    ) -> Result<Self, InvalidPdfTextReport> {
        let invalid = |detail: &str| InvalidPdfTextReport(detail.into());
        if document.format() != FormatKind::Pdf
            || document.converter() != BASELINE_CONVERTER_VERSION
            || document.is_partial()
            || !document.visuals().is_empty()
            || !document.visual_omissions().is_empty()
        {
            return Err(invalid("the source is not complete baseline PDF text"));
        }
        let units = document
            .units()
            .iter()
            .map(|unit| {
                let Provenance::PdfBlock {
                    page_number,
                    block_index,
                } = unit.provenance
                else {
                    return Err(invalid("a unit has no PDF page citation"));
                };
                if unit.format != FormatKind::Pdf
                    || unit.boundary != TopLevelBoundary::Page(page_number)
                    || !unit.source_map.segments().is_empty()
                    || unit.truncated
                {
                    return Err(invalid("a PDF text unit has incomplete provenance"));
                }
                Ok(PdfTextUnit {
                    order: unit.order,
                    kind: unit.kind,
                    section_path: unit.section_path.clone(),
                    text: unit.text.clone(),
                    page_number,
                    block_index,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let report = Self {
            total_pages,
            units,
            warnings: document.warnings().to_vec(),
        };
        report.validate(budgets)?;
        Ok(report)
    }

    /// Validates a bounded child report before it can enter the normal chunker.
    pub fn into_document(
        self,
        budgets: &ConversionBudgets,
    ) -> Result<ConvertedDocument, InvalidPdfTextReport> {
        self.validate(budgets)?;
        let units = self
            .units
            .into_iter()
            .map(|unit| StructuralUnit {
                order: unit.order,
                kind: unit.kind,
                format: FormatKind::Pdf,
                section_path: unit.section_path,
                text: unit.text,
                provenance: Provenance::PdfBlock {
                    page_number: unit.page_number,
                    block_index: unit.block_index,
                },
                boundary: TopLevelBoundary::Page(unit.page_number),
                source_map: SourceMap::default(),
                truncated: false,
            })
            .collect();
        Ok(ConvertedDocument::new(
            BASELINE_CONVERTER_VERSION,
            FormatKind::Pdf,
            units,
            self.warnings,
            Vec::new(),
        ))
    }

    fn validate(&self, budgets: &ConversionBudgets) -> Result<(), InvalidPdfTextReport> {
        let invalid = |detail: &str| InvalidPdfTextReport(detail.into());
        if self.total_pages == 0 || self.total_pages > budgets.max_items {
            return Err(invalid("page count exceeds the PDF budget"));
        }
        if self.units.is_empty()
            || self.units.len() > budgets.max_units as usize
            || self.warnings.len() > budgets.max_units as usize
        {
            return Err(invalid("text unit or warning count exceeds the PDF budget"));
        }
        let mut characters = 0_u64;
        let mut last_page = 1;
        for (index, unit) in self.units.iter().enumerate() {
            if unit.order != index as u32
                || unit.page_number < last_page
                || unit.page_number > self.total_pages
                || !matches!(unit.kind, UnitKind::Paragraph | UnitKind::Navigation)
                || !unit.section_path.is_empty()
                || unit.text.is_empty()
            {
                return Err(invalid(
                    "text units have invalid order, kind or page citations",
                ));
            }
            last_page = unit.page_number;
            let length = unit.text.chars().count() as u64;
            characters = characters
                .checked_add(length)
                .ok_or_else(|| invalid("text length overflowed"))?;
            if length > u64::from(budgets.max_unit_chars) || characters > budgets.max_output_chars {
                return Err(invalid("PDF text exceeds the output budget"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BaselineConverter, ConversionContext, DocumentConverter, DocumentMetadata, SourceContent,
    };

    #[test]
    fn round_trip_preserves_exact_pdf_text_and_citations_and_rejects_corruption() {
        let bytes = crate::formats::pdf::tests::text_pdf(&[&["First"], &["Middle"], &["Last"]]);
        let converted = BaselineConverter::new()
            .convert(
                SourceContent::Bytes(&bytes),
                &DocumentMetadata::unknown().with_media_type("application/pdf"),
                &ConversionContext::new(),
            )
            .unwrap();
        let original = converted.document().unwrap();
        let budgets = ConversionBudgets::default();
        let report = PdfTextReport::from_document(3, original, &budgets).unwrap();
        let encoded = serde_json::to_vec(&report).unwrap();
        let decoded: PdfTextReport = serde_json::from_slice(&encoded).unwrap();
        let restored = decoded.into_document(&budgets).unwrap();
        assert_eq!(restored, *original);

        let mut invalid = report.clone();
        invalid.units[0].page_number = 4;
        assert!(invalid.into_document(&budgets).is_err());
        let mut invalid = report.clone();
        invalid.units[1].order = 7;
        assert!(invalid.into_document(&budgets).is_err());
        let mut invalid = report;
        invalid.units[1].text = "x".repeat(budgets.max_unit_chars as usize + 1);
        assert!(invalid.into_document(&budgets).is_err());
    }
}
