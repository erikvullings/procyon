//! Delimiter-separated value conversion.
//!
//! A small RFC 4180 reader (quoted fields, doubled quotes, embedded newlines)
//! keeps the dependency surface at zero and the behaviour deterministic. Rows
//! are emitted in bands so that a wide table produces citable ranges instead
//! of one giant unit, and every band's provenance is the exact 0-based cell
//! range it came from.

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft};
use crate::model::{Provenance, TopLevelBoundary, UnitKind};

/// Rows per emitted unit.
pub(crate) const ROWS_PER_BAND: usize = 32;

/// Chooses the delimiter deterministically: an explicit hint wins, otherwise
/// the first line decides between a tab and a comma.
pub(crate) fn delimiter_for(
    source: &str,
    extension: Option<&str>,
    media_type: Option<&str>,
) -> char {
    if matches!(extension, Some("tsv")) || matches!(media_type, Some("text/tab-separated-values")) {
        return '\t';
    }
    let first_line = source.lines().next().unwrap_or_default();
    let tabs = first_line.matches('\t').count();
    let commas = first_line.matches(',').count();
    if tabs > commas { '\t' } else { ',' }
}

/// Parses `source` into rows of fields.
fn parse_rows(source: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut characters = source.chars().peekable();
    let mut has_content = false;
    while let Some(character) = characters.next() {
        if quoted {
            if character == '"' {
                if characters.peek() == Some(&'"') {
                    characters.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(character);
            }
            continue;
        }
        match character {
            '"' if field.is_empty() => quoted = true,
            character if character == delimiter => {
                row.push(std::mem::take(&mut field));
                has_content = true;
            }
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                if has_content || row.iter().any(|value| !value.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                has_content = false;
            }
            character => {
                field.push(character);
                has_content = true;
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|value| !value.is_empty()) {
            rows.push(row);
        }
    }
    rows
}

/// Converts delimiter-separated content into bands of rows.
pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    source: &str,
    delimiter: char,
) -> Result<(), Stop> {
    let rows = parse_rows(source, delimiter);
    for (band_index, band) in rows.chunks(ROWS_PER_BAND).enumerate() {
        builder.checkpoint()?;
        if builder.is_saturated() {
            break;
        }
        let start_row = (band_index * ROWS_PER_BAND) as u32;
        let end_row = start_row + band.len().saturating_sub(1) as u32;
        let end_column = band
            .iter()
            .map(|row| row.len())
            .max()
            .unwrap_or(0)
            .saturating_sub(1) as u32;
        let text = band
            .iter()
            .map(|row| row.join(" | "))
            .collect::<Vec<_>>()
            .join("\n");
        builder.push(UnitDraft {
            kind: UnitKind::Table,
            section_path: Vec::new(),
            text,
            provenance: Provenance::SpreadsheetRange {
                sheet: String::new(),
                start_row,
                start_column: 0,
                end_row,
                end_column,
            },
            boundary: TopLevelBoundary::Document,
            source_offset: None,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    fn convert_csv(source: &str, delimiter: char) -> ConvertedDocument {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Csv,
            &mut tracker,
        );
        convert(&mut builder, source, delimiter).expect("conversion");
        builder.finish()
    }

    #[test]
    fn quoted_fields_keep_delimiters_and_newlines() {
        let rows = parse_rows("a,\"b,c\",\"line1\nline2\"\nd,e,f\n", ',');
        assert_eq!(rows[0], ["a", "b,c", "line1\nline2"]);
        assert_eq!(rows[1], ["d", "e", "f"]);
    }

    #[test]
    fn doubled_quotes_are_unescaped() {
        let rows = parse_rows("\"say \"\"hi\"\"\",x\n", ',');
        assert_eq!(rows[0], ["say \"hi\"", "x"]);
    }

    #[test]
    fn the_delimiter_is_chosen_deterministically() {
        assert_eq!(delimiter_for("a\tb\tc\n", None, None), '\t');
        assert_eq!(delimiter_for("a,b,c\n", None, None), ',');
        assert_eq!(delimiter_for("a,b,c\n", Some("tsv"), None), '\t');
    }

    #[test]
    fn rows_are_banded_with_exact_cell_ranges() {
        let mut source = String::from("name,value\n");
        for index in 0..40 {
            source.push_str(&format!("row{index},{index}\n"));
        }
        let document = convert_csv(&source, ',');
        assert_eq!(document.units().len(), 2);
        assert_eq!(
            document.units()[0].provenance,
            Provenance::SpreadsheetRange {
                sheet: String::new(),
                start_row: 0,
                start_column: 0,
                end_row: 31,
                end_column: 1
            }
        );
        assert_eq!(
            document.units()[1].provenance,
            Provenance::SpreadsheetRange {
                sheet: String::new(),
                start_row: 32,
                start_column: 0,
                end_row: 40,
                end_column: 1
            }
        );
        assert!(
            document.units()[0]
                .text
                .starts_with("name | value\nrow0 | 0")
        );
    }
}
