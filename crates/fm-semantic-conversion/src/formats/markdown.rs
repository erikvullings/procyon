//! CommonMark conversion.
//!
//! Blocks come from `pulldown-cmark`, so the heading hierarchy, list items,
//! fenced code and tables are read from a real CommonMark parser rather than
//! from line heuristics. Unit text is the parser's *inline* text (emphasis
//! markers, link syntax and entity escapes resolved), which is what belongs in
//! an embedding; because that text is reconstructed rather than sliced, a
//! Markdown unit carries no character-level source map. Its line range is
//! exact, taken from the parser's byte offsets.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft};
use crate::formats::LineIndex;
use crate::model::{Provenance, TopLevelBoundary, UnitKind};

struct Capture {
    kind: UnitKind,
    heading_level: u32,
    text: String,
    start: usize,
    end: usize,
    depth: u32,
}

fn boundary(stack: &[(u32, String)]) -> TopLevelBoundary {
    stack
        .first()
        .map_or(TopLevelBoundary::Document, |(_, title)| {
            TopLevelBoundary::Section(title.clone())
        })
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Converts Markdown into headings, paragraphs, list items, code blocks and
/// tables.
pub(crate) fn convert(builder: &mut DocumentBuilder<'_, '_>, source: &str) -> Result<(), Stop> {
    let index = LineIndex::new(source);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut headings: Vec<(u32, String)> = Vec::new();
    let mut capture: Option<Capture> = None;

    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        builder.checkpoint()?;
        if builder.is_saturated() {
            break;
        }
        match event {
            Event::Start(tag) => {
                if let Some(active) = capture.as_mut() {
                    active.depth += 1;
                    if matches!(tag, Tag::Paragraph) && active.kind == UnitKind::ListItem {
                        active.text.push(' ');
                    }
                    continue;
                }
                let kind = match tag {
                    Tag::Heading { .. } => UnitKind::Heading,
                    Tag::Paragraph => UnitKind::Paragraph,
                    Tag::CodeBlock(_) => UnitKind::CodeBlock,
                    Tag::Item => UnitKind::ListItem,
                    Tag::Table(_) => UnitKind::Table,
                    Tag::BlockQuote(_) => UnitKind::Quote,
                    _ => continue,
                };
                let heading_level = match tag {
                    Tag::Heading { level, .. } => level as u32,
                    _ => 0,
                };
                capture = Some(Capture {
                    kind,
                    heading_level,
                    text: String::new(),
                    start: range.start,
                    end: range.end,
                    depth: 0,
                });
            }
            Event::End(end) => {
                let Some(active) = capture.as_mut() else {
                    continue;
                };
                if active.depth > 0 {
                    if active.kind == UnitKind::Table {
                        match end {
                            TagEnd::TableCell => active.text.push_str(" | "),
                            TagEnd::TableHead | TagEnd::TableRow => active.text.push('\n'),
                            _ => {}
                        }
                    }
                    active.depth -= 1;
                    continue;
                }
                let closes = matches!(
                    (&end, active.kind),
                    (TagEnd::Heading(_), UnitKind::Heading)
                        | (TagEnd::Paragraph, UnitKind::Paragraph)
                        | (TagEnd::CodeBlock, UnitKind::CodeBlock)
                        | (TagEnd::Item, UnitKind::ListItem)
                        | (TagEnd::Table, UnitKind::Table)
                        | (TagEnd::BlockQuote(_), UnitKind::Quote)
                );
                if !closes {
                    continue;
                }
                let finished = capture.take().unwrap_or_else(|| unreachable!());
                let text = if finished.kind == UnitKind::CodeBlock {
                    finished.text.trim_end().to_owned()
                } else {
                    finished
                        .text
                        .lines()
                        .map(normalize)
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                if text.is_empty() {
                    continue;
                }
                // A heading belongs to the section it opens, not to the one
                // it closes, so the stack is rewound before the unit is
                // emitted and the heading's own title is excluded from its
                // section path.
                if finished.kind == UnitKind::Heading {
                    while headings
                        .last()
                        .is_some_and(|(level, _)| *level >= finished.heading_level)
                    {
                        headings.pop();
                    }
                }
                let section_path = headings
                    .iter()
                    .map(|(_, title)| title.clone())
                    .collect::<Vec<_>>();
                let unit_boundary = if finished.kind == UnitKind::Heading {
                    let mut prospective = headings.clone();
                    prospective.push((finished.heading_level, text.clone()));
                    boundary(&prospective)
                } else {
                    boundary(&headings)
                };
                builder.push(UnitDraft {
                    kind: finished.kind,
                    section_path,
                    text: text.clone(),
                    provenance: Provenance::TextLines {
                        start_line: index.line_of(finished.start),
                        end_line: index.line_of(finished.end.saturating_sub(1).max(finished.start)),
                    },
                    boundary: unit_boundary,
                    source_offset: None,
                })?;
                if finished.kind == UnitKind::Heading {
                    headings.push((finished.heading_level, text));
                }
            }
            Event::Text(text) | Event::Code(text) | Event::InlineMath(text) => {
                if let Some(active) = capture.as_mut() {
                    active.text.push_str(&text);
                    active.end = active.end.max(range.end);
                }
            }
            Event::SoftBreak => {
                if let Some(active) = capture.as_mut() {
                    active.text.push(' ');
                }
            }
            Event::HardBreak | Event::Rule => {
                if let Some(active) = capture.as_mut() {
                    active.text.push('\n');
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    fn convert_markdown(source: &str) -> ConvertedDocument {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Markdown,
            &mut tracker,
        );
        convert(&mut builder, source).expect("conversion");
        builder.finish()
    }

    const SAMPLE: &str = "# Title\n\nIntro paragraph with *emphasis*.\n\n## Section A\n\nBody of A.\n\n- first item\n- second item\n\n```rust\nfn main() {}\n```\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";

    #[test]
    fn headings_build_the_section_path_of_later_units() {
        let document = convert_markdown(SAMPLE);
        let body = document
            .units()
            .iter()
            .find(|unit| unit.text == "Body of A.")
            .expect("body unit");
        assert_eq!(body.section_path, ["Title", "Section A"]);
        assert_eq!(body.boundary, TopLevelBoundary::Section("Title".to_owned()));
        let title = &document.units()[0];
        assert_eq!(title.kind, UnitKind::Heading);
        assert!(title.section_path.is_empty());
    }

    #[test]
    fn list_items_code_blocks_and_tables_become_their_own_units() {
        let document = convert_markdown(SAMPLE);
        let kinds: Vec<UnitKind> = document.units().iter().map(|unit| unit.kind).collect();
        assert!(kinds.contains(&UnitKind::ListItem));
        assert!(kinds.contains(&UnitKind::CodeBlock));
        assert!(kinds.contains(&UnitKind::Table));
        let code = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::CodeBlock)
            .expect("code block");
        assert_eq!(code.text, "fn main() {}");
        let table = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Table)
            .expect("table");
        assert!(table.text.contains("a | b"));
        assert!(table.text.contains("1 | 2"));
    }

    #[test]
    fn line_ranges_are_taken_from_the_parser_offsets() {
        let document = convert_markdown(SAMPLE);
        assert_eq!(
            document.units()[0].provenance,
            Provenance::TextLines {
                start_line: 1,
                end_line: 1
            }
        );
        let body = document
            .units()
            .iter()
            .find(|unit| unit.text == "Body of A.")
            .expect("body unit");
        assert_eq!(
            body.provenance,
            Provenance::TextLines {
                start_line: 7,
                end_line: 7
            }
        );
    }

    #[test]
    fn deeper_headings_pop_back_to_their_ancestor() {
        let document = convert_markdown("# A\n\n## B\n\ntext b\n\n# C\n\ntext c\n");
        let text_c = document
            .units()
            .iter()
            .find(|unit| unit.text == "text c")
            .expect("unit");
        assert_eq!(text_c.section_path, ["C"]);
        assert_eq!(text_c.boundary, TopLevelBoundary::Section("C".to_owned()));
    }

    #[test]
    fn conversion_is_deterministic() {
        let first = convert_markdown(SAMPLE);
        let second = convert_markdown(SAMPLE);
        assert_eq!(first, second);
    }
}
