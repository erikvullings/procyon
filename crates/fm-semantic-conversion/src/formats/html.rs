//! Bounded HTML conversion.
//!
//! This is a deliberately small, tolerant tokenizer rather than a full DOM
//! implementation: semantic extraction needs block text, headings and tables,
//! not layout or scripting. Nothing is fetched, no external entity is
//! resolved, and the contents of `script`, `style`, `head`-only metadata and
//! comments are discarded. Element nesting is charged against the nesting
//! budget so a pathologically deep document is refused rather than recursed
//! into.
//!
//! Unit text is reconstructed from decoded character data, so - as for
//! Markdown - HTML units carry a line range but no character-level source
//! map.

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft};
use crate::model::{Provenance, TopLevelBoundary, UnitKind};

#[derive(Debug, Clone, Copy)]
pub(crate) enum HtmlContext {
    Standalone,
    EpubSpine { spine_index: u32 },
}

/// Elements whose character data is discarded entirely.
const SKIPPED_ELEMENTS: &[&str] = &["script", "style", "noscript", "template", "svg"];

/// HTML void elements never contribute an unmatched nesting level even when
/// their source syntax omits a trailing slash.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements that end the current text block.
fn block_kind(name: &str) -> Option<UnitKind> {
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(UnitKind::Heading),
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "pre"
        | "figcaption" | "dd" | "dt" => Some(UnitKind::Paragraph),
        "li" => Some(UnitKind::ListItem),
        "blockquote" => Some(UnitKind::Quote),
        "table" => Some(UnitKind::Table),
        "title" => Some(UnitKind::Heading),
        _ => None,
    }
}

fn heading_level(name: &str) -> Option<u32> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        "title" => Some(0),
        _ => None,
    }
}

/// Decodes the small set of entities that appear in extracted text.
fn decode_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(position) = rest.find('&') {
        output.push_str(&rest[..position]);
        rest = &rest[position..];
        let Some(end) = rest[1..].find(';').map(|offset| offset + 1) else {
            output.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            numeric if numeric.starts_with("#x") || numeric.starts_with("#X") => {
                u32::from_str_radix(&numeric[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            numeric if numeric.starts_with('#') => {
                numeric[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        match decoded {
            Some(character) => output.push(character),
            None => output.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Block {
    kind: UnitKind,
    name: String,
    text: String,
    start_line: u32,
}

/// Converts bounded HTML into headings, paragraphs, list items and tables.
pub(crate) fn convert(builder: &mut DocumentBuilder<'_, '_>, source: &str) -> Result<(), Stop> {
    convert_with_context(builder, source, HtmlContext::Standalone)
}

pub(crate) fn convert_with_context(
    builder: &mut DocumentBuilder<'_, '_>,
    source: &str,
    context: HtmlContext,
) -> Result<(), Stop> {
    let bytes: Vec<char> = source.chars().collect();
    let mut position = 0_usize;
    let mut line = 1_u32;
    let mut depth = 0_u32;
    let mut skipped_elements: Vec<String> = Vec::new();
    let mut open: Vec<Block> = Vec::new();
    let mut headings: Vec<(u32, String)> = Vec::new();
    let mut pending_cell_separator = false;

    while position < bytes.len() {
        builder.checkpoint()?;
        if builder.is_saturated() {
            break;
        }
        let character = bytes[position];
        if character != '<' {
            if character == '\n' {
                line += 1;
            }
            if skipped_elements.is_empty()
                && let Some(block) = open.last_mut()
            {
                block.text.push(character);
            }
            position += 1;
            continue;
        }

        // Comments, doctypes and CDATA carry no extractable text.
        if source_starts_with(&bytes, position, "<!--") {
            let end = find_sequence(&bytes, position + 4, "-->").unwrap_or(bytes.len());
            line += count_newlines(&bytes, position, end);
            position = (end + 3).min(bytes.len());
            continue;
        }
        if source_starts_with(&bytes, position, "<!") || source_starts_with(&bytes, position, "<?")
        {
            let end = find_sequence(&bytes, position + 2, ">").unwrap_or(bytes.len());
            line += count_newlines(&bytes, position, end);
            position = (end + 1).min(bytes.len());
            continue;
        }

        let Some(tag_end) = find_tag_end(&bytes, position) else {
            position += 1;
            continue;
        };
        let raw: String = bytes[position + 1..tag_end].iter().collect();
        let closing = raw.starts_with('/');
        let qualified_name = raw
            .trim_start_matches('/')
            .split(|character: char| character.is_whitespace() || character == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let name = qualified_name
            .rsplit(':')
            .next()
            .unwrap_or_default()
            .to_owned();
        let self_closing = raw.trim_end().ends_with('/') || VOID_ELEMENTS.contains(&name.as_str());
        line += count_newlines(&bytes, position, tag_end);
        position = tag_end + 1;

        if closing {
            depth = depth.saturating_sub(1);
        } else if !self_closing {
            depth += 1;
            builder.tracker().charge_depth(depth)?;
        }

        if !skipped_elements.is_empty() {
            if closing && skipped_elements.last() == Some(&name) {
                skipped_elements.pop();
            } else if !closing && !self_closing {
                skipped_elements.push(name);
            }
            continue;
        }
        if !closing && SKIPPED_ELEMENTS.contains(&name.as_str()) && !self_closing {
            skipped_elements.push(name);
            continue;
        }

        if closing {
            if let Some(block) = open.last()
                && block.name == name
            {
                let block = open.pop().unwrap_or_else(|| unreachable!());
                emit(builder, block, line, &mut headings, context)?;
            }
            continue;
        }

        match name.as_str() {
            "br" => {
                if let Some(block) = open.last_mut() {
                    block.text.push('\n');
                }
            }
            "td" | "th" => {
                if let Some(block) = open.last_mut() {
                    if pending_cell_separator {
                        block.text.push_str(" | ");
                    }
                    pending_cell_separator = true;
                }
            }
            "tr" => {
                if let Some(block) = open.last_mut() {
                    block.text.push('\n');
                    pending_cell_separator = false;
                }
            }
            _ => {}
        }
        if self_closing {
            continue;
        }
        if let Some(kind) = block_kind(&name) {
            // A nested block closes the enclosing one, which keeps container
            // elements such as `div` from swallowing their children.
            if let Some(previous) = open.pop() {
                emit(builder, previous, line, &mut headings, context)?;
            }
            open.push(Block {
                kind,
                name,
                text: String::new(),
                start_line: line,
            });
        }
    }
    while let Some(block) = open.pop() {
        emit(builder, block, line, &mut headings, context)?;
    }
    Ok(())
}

fn emit(
    builder: &mut DocumentBuilder<'_, '_>,
    block: Block,
    end_line: u32,
    headings: &mut Vec<(u32, String)>,
    context: HtmlContext,
) -> Result<(), Stop> {
    let text = decode_entities(&block.text)
        .lines()
        .map(normalize)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return Ok(());
    }
    let level = heading_level(&block.name);
    if let Some(level) = level {
        while headings.last().is_some_and(|(open, _)| *open >= level) {
            headings.pop();
        }
    }
    let section_path = headings
        .iter()
        .map(|(_, title)| title.clone())
        .collect::<Vec<_>>();
    let boundary = match context {
        HtmlContext::Standalone => match (level, headings.first()) {
            (_, Some((_, title))) => TopLevelBoundary::Section(title.clone()),
            (Some(_), None) => TopLevelBoundary::Section(text.clone()),
            (None, None) => TopLevelBoundary::Document,
        },
        HtmlContext::EpubSpine { spine_index } => TopLevelBoundary::EpubSpine(spine_index),
    };
    let provenance = match context {
        HtmlContext::Standalone => Provenance::TextLines {
            start_line: block.start_line,
            end_line: end_line.max(block.start_line),
        },
        HtmlContext::EpubSpine { spine_index } => Provenance::EpubText {
            spine_index,
            start_line: block.start_line,
            end_line: end_line.max(block.start_line),
        },
    };
    builder.push(UnitDraft {
        kind: block.kind,
        section_path,
        text: text.clone(),
        provenance,
        boundary,
        source_offset: None,
    })?;
    if let Some(level) = level {
        headings.push((level, text));
    }
    Ok(())
}

fn source_starts_with(characters: &[char], position: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(offset, expected)| characters.get(position + offset) == Some(&expected))
}

fn find_sequence(characters: &[char], from: usize, needle: &str) -> Option<usize> {
    (from..characters.len()).find(|index| source_starts_with(characters, *index, needle))
}

fn count_newlines(characters: &[char], from: usize, to: usize) -> u32 {
    characters[from.min(characters.len())..to.min(characters.len())]
        .iter()
        .filter(|character| **character == '\n')
        .count() as u32
}

/// Finds the `>` closing a tag, honouring quoted attribute values.
fn find_tag_end(characters: &[char], from: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (index, character) in characters.iter().copied().enumerate().skip(from + 1) {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == '>' => return Some(index),
            None => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    fn convert_html(source: &str) -> ConvertedDocument {
        convert_html_with(source, ConversionBudgets::default())
    }

    fn convert_html_with(source: &str, budgets: ConversionBudgets) -> ConvertedDocument {
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Html,
            &mut tracker,
        );
        convert(&mut builder, source).expect("conversion");
        builder.finish()
    }

    const SAMPLE: &str = "<html><head><title>Doc</title><style>p{color:red}</style></head><body>\n<h1>Heading</h1>\n<p>First &amp; only paragraph.</p>\n<script>alert('x')</script>\n<ul><li>alpha</li><li>beta</li></ul>\n<table><tr><td>1</td><td>2</td></tr></table>\n</body></html>";

    #[test]
    fn script_and_style_content_is_discarded() {
        let document = convert_html(SAMPLE);
        let texts: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect();
        assert!(!texts.iter().any(|text| text.contains("alert")));
        assert!(!texts.iter().any(|text| text.contains("color:red")));
    }

    #[test]
    fn nested_skipped_elements_cannot_leak_text_into_an_enclosing_block() {
        let document =
            convert_html("<p>before<script><script>inner</script>still script</script>after</p>");

        assert_eq!(document.units()[0].text, "beforeafter");
    }

    #[test]
    fn skipped_elements_still_count_toward_the_nesting_budget() {
        let budgets = ConversionBudgets {
            max_nesting_depth: 2,
            ..ConversionBudgets::default()
        };
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Html,
            &mut tracker,
        );

        let outcome = convert(
            &mut builder,
            "<html><script><template>hidden</template></script></html>",
        );

        assert!(matches!(
            outcome,
            Err(Stop::OverBudget {
                kind: crate::budget::BudgetKind::NestingDepth,
                limit: 2
            })
        ));
    }

    #[test]
    fn headings_paragraphs_lists_and_tables_are_extracted_with_entities_decoded() {
        let document = convert_html(SAMPLE);
        let paragraph = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Paragraph)
            .expect("paragraph");
        assert_eq!(paragraph.text, "First & only paragraph.");
        assert_eq!(paragraph.section_path, ["Doc", "Heading"]);
        let items: Vec<&str> = document
            .units()
            .iter()
            .filter(|unit| unit.kind == UnitKind::ListItem)
            .map(|unit| unit.text.as_str())
            .collect();
        assert_eq!(items, ["alpha", "beta"]);
        let table = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Table)
            .expect("table");
        assert_eq!(table.text, "1 | 2");
    }

    #[test]
    fn line_ranges_track_the_source() {
        let document = convert_html(SAMPLE);
        let paragraph = document
            .units()
            .iter()
            .find(|unit| unit.kind == UnitKind::Paragraph)
            .expect("paragraph");
        assert_eq!(
            paragraph.provenance,
            Provenance::TextLines {
                start_line: 3,
                end_line: 3
            }
        );
    }

    #[test]
    fn excessive_nesting_is_refused_by_the_depth_budget() {
        let budgets = ConversionBudgets {
            max_nesting_depth: 8,
            ..ConversionBudgets::default()
        };
        let deep = "<div>".repeat(64) + "text" + &"</div>".repeat(64);
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Html,
            &mut tracker,
        );
        let outcome = convert(&mut builder, &deep);
        assert!(matches!(
            outcome,
            Err(Stop::OverBudget {
                kind: crate::budget::BudgetKind::NestingDepth,
                limit: 8
            })
        ));
        drop(convert_html_with("<p>ok</p>", ConversionBudgets::default()));
    }

    #[test]
    fn malformed_markup_still_yields_the_visible_text() {
        let document = convert_html("<p>unclosed paragraph<div>and a div");
        let texts: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect();
        assert_eq!(texts, ["unclosed paragraph", "and a div"]);
    }

    #[test]
    fn entities_decode_including_numeric_forms() {
        assert_eq!(decode_entities("a&amp;b&#65;c&#x42;d"), "a&bAcBd");
        assert_eq!(decode_entities("bare & ampersand"), "bare & ampersand");
    }
}
