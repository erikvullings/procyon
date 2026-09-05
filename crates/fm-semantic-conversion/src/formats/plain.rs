//! Plain-text and source-code conversion.
//!
//! Both split on blank lines, which is the only structure these formats
//! actually expose. Source code additionally reports the symbol declared at
//! the top of a block *when it can be read directly from the text* - there is
//! no language parser here, so an unrecognized block reports no symbol rather
//! than a guessed one.

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft};
use crate::formats::{TextBlock, split_blocks};
use crate::model::{FormatKind, Provenance, TopLevelBoundary, UnitKind};

/// Keywords that introduce a named symbol in the languages Procyon indexes.
const SYMBOL_KEYWORDS: &[&str] = &[
    "class",
    "def",
    "enum",
    "fn",
    "func",
    "function",
    "impl",
    "interface",
    "module",
    "package",
    "struct",
    "trait",
    "type",
];

/// Reads the symbol name declared on `line`, if the line is a recognizable
/// declaration.
fn symbol_on_line(line: &str) -> Option<String> {
    let mut tokens = line.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        if !SYMBOL_KEYWORDS.contains(&token) {
            continue;
        }
        let candidate = tokens.peek()?;
        let name: String = candidate
            .chars()
            .take_while(|character| {
                character.is_alphanumeric() || *character == '_' || *character == '$'
            })
            .collect();
        if name.is_empty() {
            return None;
        }
        return Some(name);
    }
    None
}

/// The first symbol declared inside a block, searched from its first line
/// outwards so a decorated or annotated declaration still resolves.
fn block_symbol(text: &str) -> Option<String> {
    text.lines().find_map(symbol_on_line)
}

fn push_block(
    builder: &mut DocumentBuilder<'_, '_>,
    source: &str,
    block: &TextBlock,
    code: bool,
) -> Result<(), Stop> {
    let text = block.text(source);
    let provenance = if code {
        Provenance::CodeLines {
            start_line: block.start_line,
            end_line: block.end_line,
            symbol: block_symbol(text),
        }
    } else {
        Provenance::TextLines {
            start_line: block.start_line,
            end_line: block.end_line,
        }
    };
    builder.push(UnitDraft {
        kind: if code {
            UnitKind::CodeBlock
        } else {
            UnitKind::Paragraph
        },
        section_path: Vec::new(),
        text: text.to_owned(),
        provenance,
        boundary: TopLevelBoundary::Document,
        source_offset: Some(block.char_offset),
    })?;
    Ok(())
}

/// Converts plain text or source code into blank-line separated units.
pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    source: &str,
    format: FormatKind,
) -> Result<(), Stop> {
    let code = matches!(format, FormatKind::SourceCode);
    for block in split_blocks(source) {
        builder.checkpoint()?;
        if builder.is_saturated() {
            break;
        }
        push_block(builder, source, &block, code)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::model::{ComponentVersion, ConvertedDocument};

    fn convert_text(source: &str, format: FormatKind) -> ConvertedDocument {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder =
            DocumentBuilder::new(ComponentVersion::new("baseline", 1), format, &mut tracker);
        convert(&mut builder, source, format).expect("conversion");
        builder.finish()
    }

    #[test]
    fn plain_text_units_carry_exact_line_ranges_and_source_offsets() {
        let source = "First paragraph line one.\nLine two.\n\nSecond paragraph.\n";
        let document = convert_text(source, FormatKind::PlainText);
        assert_eq!(document.units().len(), 2);
        assert_eq!(
            document.units()[0].provenance,
            Provenance::TextLines {
                start_line: 1,
                end_line: 2
            }
        );
        assert_eq!(
            document.units()[1].provenance,
            Provenance::TextLines {
                start_line: 4,
                end_line: 4
            }
        );
        let second = &document.units()[1];
        assert_eq!(second.text, "Second paragraph.");
        assert_eq!(
            second.source_map.first_source_offset(),
            Some(source.find("Second").expect("offset"))
        );
    }

    #[test]
    fn carriage_returns_are_removed_while_the_source_map_still_points_at_the_source() {
        let source = "alpha\r\nbeta\r\n";
        let document = convert_text(source, FormatKind::PlainText);
        let unit = &document.units()[0];
        assert_eq!(unit.text, "alpha\nbeta");
        assert_eq!(unit.source_map.source_offset(0), Some(0));
        // "beta" starts at character 7 of the source: a,l,p,h,a,\r,\n.
        assert_eq!(unit.source_map.source_offset(6), Some(7));
    }

    #[test]
    fn source_code_units_report_the_declared_symbol_when_one_is_readable() {
        let source = "fn alpha(value: u32) -> u32 {\n    value\n}\n\nlet x = 1;\n";
        let document = convert_text(source, FormatKind::SourceCode);
        assert_eq!(
            document.units()[0].provenance,
            Provenance::CodeLines {
                start_line: 1,
                end_line: 3,
                symbol: Some("alpha".to_owned())
            }
        );
        assert_eq!(
            document.units()[1].provenance,
            Provenance::CodeLines {
                start_line: 5,
                end_line: 5,
                symbol: None
            }
        );
        assert_eq!(document.units()[0].kind, UnitKind::CodeBlock);
    }

    #[test]
    fn symbols_are_read_from_several_languages_without_guessing() {
        assert_eq!(symbol_on_line("pub fn run() {"), Some("run".to_owned()));
        assert_eq!(
            symbol_on_line("export default class Widget extends Base {"),
            Some("Widget".to_owned())
        );
        assert_eq!(
            symbol_on_line("    def handler(self):"),
            Some("handler".to_owned())
        );
        assert_eq!(symbol_on_line("total = compute(1, 2)"), None);
        assert_eq!(symbol_on_line("fn ("), None);
    }
}
