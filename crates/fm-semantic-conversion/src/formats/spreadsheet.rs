//! SpreadsheetML conversion.
//!
//! Calamine parses the workbook; this module only bounds it and turns cells
//! into citable bands. Sheets are visited in workbook order, rows in ascending
//! order, and every unit records the exact 0-based cell range it came from.
//! Formulas, charts, pivot caches and images are not read - a spreadsheet's
//! semantic content is its cell values.

use std::io::Cursor;

use calamine::{Data, Reader};

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft};
use crate::formats::csv::ROWS_PER_BAND;
use crate::model::{Omission, Provenance, TopLevelBoundary, UnitKind};

/// Maximum columns read from one sheet.
const MAX_COLUMNS: u32 = 2_048;

/// Why a workbook could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkbookError {
    /// The workbook is structurally broken or unsupported.
    Malformed(String),
    /// The workbook is encrypted or password protected.
    Encrypted(String),
    /// A hard budget stopped the work, or the caller cancelled it.
    Stopped(Stop),
}

impl From<Stop> for WorkbookError {
    fn from(stop: Stop) -> Self {
        Self::Stopped(stop)
    }
}

fn cell_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        other => other.to_string(),
    }
}

/// Converts an XLSX workbook into bands of rows per sheet.
pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    bytes: &[u8],
) -> Result<(), WorkbookError> {
    let mut workbook =
        calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|error| {
            let detail = error.to_string();
            if detail.to_ascii_lowercase().contains("password")
                || detail.to_ascii_lowercase().contains("encrypt")
            {
                WorkbookError::Encrypted(detail)
            } else {
                WorkbookError::Malformed(format!("the workbook is not readable: {detail}"))
            }
        })?;
    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(WorkbookError::Malformed(
            "the workbook contains no worksheets".to_owned(),
        ));
    }
    builder
        .tracker()
        .charge_items(sheet_names.len() as u64)
        .map_err(WorkbookError::from)?;

    for name in sheet_names {
        builder.checkpoint().map_err(WorkbookError::from)?;
        if builder.is_saturated() {
            break;
        }
        let range = match workbook.worksheet_range(&name) {
            Ok(range) => range,
            Err(error) => {
                builder.omit(Omission::UnreadablePart {
                    detail: format!("worksheet '{name}' could not be read: {error}"),
                });
                continue;
            }
        };
        let Some((start_row, start_column)) = range.start() else {
            continue;
        };
        let Some((end_row, end_column)) = range.end() else {
            continue;
        };
        let end_column = end_column.min(start_column.saturating_add(MAX_COLUMNS - 1));
        if end_column
            < start_column
                .saturating_add(range.width() as u32)
                .saturating_sub(1)
        {
            builder.omit(Omission::UnreadablePart {
                detail: format!("worksheet '{name}' was truncated to {MAX_COLUMNS} columns"),
            });
        }

        let mut band_start = start_row;
        while band_start <= end_row {
            builder.checkpoint().map_err(WorkbookError::from)?;
            if builder.is_saturated() {
                break;
            }
            let band_end = band_start
                .saturating_add(ROWS_PER_BAND as u32 - 1)
                .min(end_row);
            let mut lines = Vec::new();
            for row in band_start..=band_end {
                let cells: Vec<String> = (start_column..=end_column)
                    .map(|column| {
                        range
                            .get_value((row, column))
                            .map(cell_text)
                            .unwrap_or_default()
                    })
                    .collect();
                if cells.iter().any(|cell| !cell.is_empty()) {
                    lines.push(cells.join(" | "));
                }
            }
            if !lines.is_empty() {
                builder
                    .push(UnitDraft {
                        kind: UnitKind::Table,
                        section_path: vec![name.clone()],
                        text: lines.join("\n"),
                        provenance: Provenance::SpreadsheetRange {
                            sheet: name.clone(),
                            start_row: band_start,
                            start_column,
                            end_row: band_end,
                            end_column,
                        },
                        boundary: TopLevelBoundary::Sheet(name.clone()),
                        source_offset: None,
                    })
                    .map_err(WorkbookError::from)?;
            }
            band_start = band_end.saturating_add(1);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::formats::ooxml::tests::package;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    /// Builds a minimal but valid XLSX package with inline strings, so the
    /// fixture stays generated text rather than a checked-in binary.
    pub(crate) fn workbook(sheets: &[(&str, &[&[&str]])]) -> Vec<u8> {
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        let mut sheet_entries = String::new();
        let mut content_overrides = String::new();
        for (index, (name, rows)) in sheets.iter().enumerate() {
            let number = index + 1;
            sheet_entries.push_str(&format!(
                r#"<sheet name="{name}" sheetId="{number}" r:id="rId{number}"/>"#
            ));
            content_overrides.push_str(&format!(
                r#"<Override PartName="/xl/worksheets/sheet{number}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            ));
            let mut sheet_data = String::new();
            for (row_index, row) in rows.iter().enumerate() {
                let row_number = row_index + 1;
                sheet_data.push_str(&format!("<row r=\"{row_number}\">"));
                for (column_index, value) in row.iter().enumerate() {
                    let column = char::from(b'A' + column_index as u8);
                    sheet_data.push_str(&format!(
                        "<c r=\"{column}{row_number}\" t=\"inlineStr\"><is><t>{value}</t></is></c>"
                    ));
                }
                sheet_data.push_str("</row>");
            }
            entries.push((
                format!("xl/worksheets/sheet{number}.xml"),
                format!(
                    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sheet_data}</sheetData></worksheet>"#
                )
                .into_bytes(),
            ));
        }
        let relationships = sheets
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let number = index + 1;
                format!(
                    r#"<Relationship Id="rId{number}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{number}.xml"/>"#
                )
            })
            .collect::<String>();
        entries.push((
            "xl/workbook.xml".to_owned(),
            format!(
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheet_entries}</sheets></workbook>"#
            )
            .into_bytes(),
        ));
        entries.push((
            "xl/_rels/workbook.xml.rels".to_owned(),
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
            )
            .into_bytes(),
        ));
        entries.push((
            "_rels/.rels".to_owned(),
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec(),
        ));
        entries.push((
            "[Content_Types].xml".to_owned(),
            format!(
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>{content_overrides}</Types>"#
            )
            .into_bytes(),
        ));
        let borrowed: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        package(&borrowed)
    }

    fn convert_workbook(bytes: &[u8]) -> Result<ConvertedDocument, WorkbookError> {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Spreadsheet,
            &mut tracker,
        );
        convert(&mut builder, bytes)?;
        Ok(builder.finish())
    }

    #[test]
    fn sheets_become_bands_with_exact_cell_ranges() {
        let bytes = workbook(&[(
            "Sales",
            &[
                &["Region", "Total"] as &[&str],
                &["North", "12"],
                &["South", "7"],
            ],
        )]);
        let document = convert_workbook(&bytes).expect("conversion");
        assert_eq!(document.units().len(), 1);
        let unit = &document.units()[0];
        assert_eq!(unit.text, "Region | Total\nNorth | 12\nSouth | 7");
        assert_eq!(unit.section_path, ["Sales"]);
        assert_eq!(unit.boundary, TopLevelBoundary::Sheet("Sales".to_owned()));
        assert_eq!(
            unit.provenance,
            Provenance::SpreadsheetRange {
                sheet: "Sales".to_owned(),
                start_row: 0,
                start_column: 0,
                end_row: 2,
                end_column: 1
            }
        );
    }

    #[test]
    fn every_sheet_is_visited_in_workbook_order() {
        let rows: &[&[&str]] = &[&["a"]];
        let bytes = workbook(&[("First", rows), ("Second", rows)]);
        let document = convert_workbook(&bytes).expect("conversion");
        let sheets: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| match &unit.provenance {
                Provenance::SpreadsheetRange { sheet, .. } => sheet.as_str(),
                other => panic!("unexpected provenance {other:?}"),
            })
            .collect();
        assert_eq!(sheets, ["First", "Second"]);
    }

    #[test]
    fn long_sheets_are_split_into_bands() {
        let rows: Vec<Vec<String>> = (0..40)
            .map(|index| vec![format!("row{index}"), index.to_string()])
            .collect();
        let borrowed_rows: Vec<Vec<&str>> = rows
            .iter()
            .map(|row| row.iter().map(String::as_str).collect())
            .collect();
        let row_slices: Vec<&[&str]> = borrowed_rows.iter().map(Vec::as_slice).collect();
        let bytes = workbook(&[("Data", row_slices.as_slice())]);
        let document = convert_workbook(&bytes).expect("conversion");
        assert_eq!(document.units().len(), 2);
        assert_eq!(
            document.units()[1].provenance,
            Provenance::SpreadsheetRange {
                sheet: "Data".to_owned(),
                start_row: 32,
                start_column: 0,
                end_row: 39,
                end_column: 1
            }
        );
    }

    #[test]
    fn a_broken_workbook_is_malformed() {
        let bytes = package(&[("xl/workbook.xml", b"<workbook/>")]);
        assert!(matches!(
            convert_workbook(&bytes),
            Err(WorkbookError::Malformed(_))
        ));
    }
}
