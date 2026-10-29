//! Rebuilds running text from the text-layer PDF lines Docling reports.
//!
//! The text-layer path hands over one located paragraph per *line*, and a bracket set in a
//! different font than its neighbours (`(Durlak, 1997)`) comes back as its own tiny cell instead
//! of staying in the line. Left alone, a wrapped sentence becomes several paragraphs and a
//! citation loses its brackets. This pass puts each stray bracket back into the line it was
//! painted in, then joins the lines of one paragraph.

use docling_core::Node;

use crate::table_rows;

/// Horizontal tolerance, in Docling's 0–511 page units, for "same left edge" and "reaches the
/// right edge".
const EDGE_TOLERANCE: u16 = 3;
/// Lines of context used to find the right edge of the column a line belongs to.
const COLUMN_WINDOW: usize = 12;
/// Widest vertical gap, in line heights, between two lines of one paragraph.
const MAX_LINE_GAP: f32 = 2.2;
/// Widest indent, in page units, of a hanging continuation or a first-line indent.
const MAX_INDENT: u16 = 40;
/// Height, in page units, of the top and bottom bands where running headers and footers live.
const MARGIN_BAND: u16 = 40;
/// Pages a margin line must repeat on to count as running furniture.
const MIN_FURNITURE_PAGES: usize = 3;
/// Fewest letters a margin line needs to be matched across pages (a lone page number has none).
const MIN_FURNITURE_LETTERS: usize = 4;
/// Widest horizontal gap, in page units, between fragments painted on one line.
const SAME_LINE_GAP: u16 = 5;

/// A located one-line paragraph.
struct Line {
    location: [u16; 4],
    text: String,
    /// How the paragraph grew: a continuation indented under its first line (a hanging
    /// indent, as in reference lists) means the next line at the margin starts a new entry.
    hanging: bool,
    /// Lines joined into this paragraph so far.
    count: usize,
}

impl Line {
    fn new(location: [u16; 4], text: String) -> Self {
        Self {
            location,
            text,
            hanging: false,
            count: 1,
        }
    }
}

/// One word cell of a page in PDF points (top-left origin), as the parser reports it.
#[derive(Debug, Clone)]
pub(crate) struct WordBox {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub text: String,
}

/// The word cells of one page and the page size they are measured in.
#[derive(Debug, Clone, Default)]
pub(crate) struct PageWords {
    pub width: f32,
    pub height: f32,
    pub words: Vec<WordBox>,
}

/// Docling's location grid spans the page in 0–511 units on both axes.
const GRID: f32 = 511.0;
/// Largest gap, in points, between a bracket and the word it touches.
const TOUCHING: f32 = 3.0;
/// Lines searched on either side of a stray bracket for the word it belongs to.
const HOST_WINDOW: usize = 4;

/// Reads the word cells of every page of `pdf`.
pub(crate) fn page_words(pdf: &[u8]) -> Vec<PageWords> {
    docling_pdf::textparse::pdf_words(pdf)
        .into_iter()
        .map(|(width, height, cells)| PageWords {
            width,
            height,
            words: cells
                .into_iter()
                .map(|cell| WordBox {
                    left: cell.l,
                    top: cell.t,
                    right: cell.r,
                    text: cell.text,
                })
                .collect(),
        })
        .collect()
}

/// True when some paragraph is nothing but a bracket, i.e. word cells are worth consulting.
pub(crate) fn has_stray_brackets(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Located { inner, .. } => {
            matches!(&**inner, Node::Paragraph { text } if is_bracket(text).is_some())
        }
        _ => false,
    })
}

/// Joins wrapped lines into paragraphs and restores stray brackets, leaving every other node
/// untouched. `pages` holds the word cells per page (index 0 is page 1); it may be empty.
pub(crate) fn reflow_lines(nodes: Vec<Node>, pages: &[PageWords]) -> Vec<Node> {
    let furniture = running_furniture(&nodes);
    let mut output = Vec::with_capacity(nodes.len());
    let mut run: Vec<Line> = Vec::new();
    let mut page = 0_usize;
    for node in nodes {
        match node {
            Node::Located {
                location,
                inner: boxed,
            } => match *boxed {
                Node::Paragraph { text } => {
                    if !(in_margin_band(location) && furniture.contains(&furniture_key(&text))) {
                        run.push(Line::new(location, text));
                    }
                }
                inner => {
                    flush(&mut run, &mut output, pages.get(page));
                    output.push(Node::Located {
                        location,
                        inner: Box::new(inner),
                    });
                }
            },
            other => {
                flush(&mut run, &mut output, pages.get(page));
                if let Node::PageInfo { page_no, .. } = &other {
                    page = page_no.saturating_sub(1);
                }
                output.push(other);
            }
        }
    }
    flush(&mut run, &mut output, pages.get(page));
    output
}

/// Lines in the top or bottom band of the page that repeat on several pages are running
/// headers, footers or page labels, not content; each would otherwise become a chunk per page.
fn in_margin_band(location: [u16; 4]) -> bool {
    location[3] <= MARGIN_BAND || location[1] >= GRID as u16 - MARGIN_BAND
}

/// Page-independent identity of a header line: letters only, so a changing page number or
/// volume does not hide the repeat.
fn furniture_key(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect()
}

fn running_furniture(nodes: &[Node]) -> std::collections::HashSet<String> {
    let mut pages_seen: std::collections::HashMap<String, std::collections::BTreeSet<usize>> =
        std::collections::HashMap::new();
    let mut page = 0_usize;
    for node in nodes {
        match node {
            Node::PageInfo { page_no, .. } => page = *page_no,
            Node::Located { location, inner } if in_margin_band(*location) => {
                if let Node::Paragraph { text } = &**inner {
                    let key = furniture_key(text);
                    if key.chars().count() >= MIN_FURNITURE_LETTERS {
                        pages_seen.entry(key).or_default().insert(page);
                    }
                }
            }
            _ => {}
        }
    }
    pages_seen
        .into_iter()
        .filter(|(_, pages)| pages.len() >= MIN_FURNITURE_PAGES)
        .map(|(key, _)| key)
        .collect()
}

fn flush(run: &mut Vec<Line>, output: &mut Vec<Node>, page: Option<&PageWords>) {
    if run.is_empty() {
        return;
    }
    let lines = restore_brackets(std::mem::take(run), page);
    let cells: Vec<table_rows::Cell<'_>> = lines
        .iter()
        .map(|line| table_rows::Cell {
            location: line.location,
            text: &line.text,
        })
        .collect();
    let mut tables = table_rows::find_tables(&cells);
    // A table sits where its first cell was, splitting the paragraphs around it.
    let mut owner = vec![None; lines.len()];
    for (number, block) in tables.iter().enumerate() {
        for &member in &block.members {
            owner[member] = Some(number);
        }
    }
    let mut pending: Vec<Line> = Vec::new();
    let mut emitted = vec![false; tables.len()];
    let emit_lines = |pending: &mut Vec<Line>, output: &mut Vec<Node>| {
        output.extend(
            merge_lines(join_same_line(std::mem::take(pending)))
                .into_iter()
                .filter(has_content)
                .map(|line| Node::Located {
                    location: line.location,
                    inner: Box::new(Node::Paragraph { text: line.text }),
                }),
        );
    };
    for (index, line) in lines.into_iter().enumerate() {
        match owner[index] {
            None => pending.push(line),
            Some(number) if !emitted[number] => {
                emitted[number] = true;
                emit_lines(&mut pending, output);
                output.push(Node::Table(std::mem::take(&mut tables[number].table)));
            }
            Some(_) => {}
        }
    }
    emit_lines(&mut pending, output);
}

fn is_bracket(text: &str) -> Option<char> {
    let mut chars = text.trim().chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('(' | ')' | '[' | ']')), None) => Some(c),
        _ => None,
    }
}

/// A lone outline number such as `1.` or `2.3.`, the orphaned left column of a contents line.
/// The trailing full stop is required: a bare `25.1` is a table value, not an outline number.
pub(crate) fn is_bare_section_number(text: &str) -> bool {
    let Some(number) = text.trim().strip_suffix('.') else {
        return false;
    };
    number.len() <= 8
        && number
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Undoes the Markdown escaping Docling applies to prose.
pub(crate) fn unescape(token: &str) -> String {
    token
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("\\_", "_")
}

/// The word a stray bracket touches: the one after an opening bracket, before a closing one.
fn touching_word(bracket: char, location: [u16; 4], page: &PageWords) -> Option<String> {
    let scale_x = page.width / GRID;
    let scale_y = page.height / GRID;
    let (left, top) = (
        f32::from(location[0]) * scale_x,
        f32::from(location[1]) * scale_y,
    );
    let right = f32::from(location[2]) * scale_x;
    let opens = matches!(bracket, '(' | '[');
    page.words
        .iter()
        .filter(|word| (word.top - top).abs() <= 2.0 * scale_y.max(1.0))
        .filter(|word| is_bracket(&word.text).is_none())
        .filter_map(|word| {
            let gap = if opens {
                word.left - right
            } else {
                left - word.right
            };
            (-1.5..=TOUCHING)
                .contains(&gap)
                .then_some((gap.abs(), word))
        })
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, word)| word.text.trim().to_owned())
}

/// Puts each stray bracket back onto the word it touches in a neighbouring line.
fn restore_brackets(mut lines: Vec<Line>, page: Option<&PageWords>) -> Vec<Line> {
    let Some(page) = page else { return lines };
    let mut consumed = vec![false; lines.len()];
    for index in 0..lines.len() {
        let Some(bracket) = is_bracket(&lines[index].text) else {
            continue;
        };
        let Some(word) = touching_word(bracket, lines[index].location, page) else {
            continue;
        };
        let opens = matches!(bracket, '(' | '[');
        let from = index.saturating_sub(HOST_WINDOW);
        let to = (index + HOST_WINDOW + 1).min(lines.len());
        let mut candidates: Vec<usize> = (from..to)
            .filter(|candidate| {
                *candidate != index && is_bracket(&lines[*candidate].text).is_none()
            })
            .collect();
        candidates.sort_by_key(|candidate| candidate.abs_diff(index));
        let host = candidates
            .into_iter()
            .find(|candidate| attach(&mut lines[*candidate].text, &word, bracket, opens));
        if host.is_some() {
            consumed[index] = true;
        }
    }
    lines
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !consumed[*index])
        .map(|(_, line)| line)
        .collect()
}

/// Adds `bracket` to the first token of `text` equal to `word`; false when none matches.
fn attach(text: &mut String, word: &str, bracket: char, opens: bool) -> bool {
    let mut tokens: Vec<String> = text.split(' ').map(str::to_owned).collect();
    let Some(position) = tokens.iter().position(|token| {
        let plain = unescape(token);
        let plain = plain.trim_matches(['(', ')', '[', ']']);
        plain == word
            && if opens {
                !token.starts_with(bracket)
            } else {
                !token.ends_with(bracket)
            }
    }) else {
        return false;
    };
    if opens {
        tokens[position].insert(0, bracket);
    } else {
        tokens[position].push(bracket);
    }
    *text = tokens.join(" ");
    // The bracket's old slot left a gap before trailing punctuation: `1997 ,` -> `1997),`.
    for (gapped, closed) in [(") ,", "),"), (") .", ")."), (") ;", ");"), ("] ,", "],")] {
        *text = text.replace(gapped, closed);
    }
    true
}

/// Joins consecutive lines that continue one paragraph.
fn merge_lines(lines: Vec<Line>) -> Vec<Line> {
    let column_right: Vec<u16> = (0..lines.len())
        .map(|index| {
            let from = index.saturating_sub(COLUMN_WINDOW);
            let to = (index + COLUMN_WINDOW + 1).min(lines.len());
            lines[from..to]
                .iter()
                .filter(|other| {
                    other.location[0].abs_diff(lines[index].location[0]) <= EDGE_TOLERANCE
                        || other.location[0].abs_diff(lines[index].location[0]) <= MAX_INDENT
                })
                .map(|other| other.location[2])
                .max()
                .unwrap_or(lines[index].location[2])
        })
        .collect();
    let mut merged: Vec<Line> = Vec::with_capacity(lines.len());
    let mut previous_right = 0_u16;
    for (index, line) in lines.iter().enumerate() {
        if let Some(last) = merged.last_mut()
            && let Some(kind) = continues(
                last,
                previous_right,
                line,
                column_right[index],
                lines.get(index + 1),
            )
        {
            join(&mut last.text, &line.text);
            last.location[2] = last.location[2].max(line.location[2]);
            last.location[3] = line.location[3];
            last.count += 1;
            last.hanging |= kind == Continuation::Hanging;
            previous_right = line.location[2];
            continue;
        }
        previous_right = line.location[2];
        merged.push(Line::new(line.location, line.text.clone()));
    }
    merged
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Continuation {
    /// Same left edge as the paragraph.
    Flush,
    /// Indented under a first line that sits at the margin.
    Hanging,
    /// Back at the margin after an indented first line.
    Returning,
}

fn continues(
    previous: &Line,
    previous_right: u16,
    next: &Line,
    column_right: u16,
    after: Option<&Line>,
) -> Option<Continuation> {
    let [previous_left, _, _, previous_bottom] = previous.location;
    let [next_left, next_top, next_right, _] = next.location;
    let height = f32::from(
        previous.location[3]
            .saturating_sub(previous.location[1])
            .max(1),
    );
    // A last line stops short of the column's right edge; a first line is indented.
    let previous_is_full = previous_right.saturating_add(EDGE_TOLERANCE) >= column_right;
    let below = next_top.saturating_add(2) >= previous_bottom
        && f32::from(next_top.saturating_sub(previous_bottom)) <= MAX_LINE_GAP * height;
    if !previous_is_full || !below || next_right > column_right.saturating_add(EDGE_TOLERANCE) {
        return None;
    }
    let shift = next_left.abs_diff(previous_left);
    if shift <= EDGE_TOLERANCE {
        // After a hanging entry, a line back at the margin opens the next entry.
        return (!previous.hanging).then_some(Continuation::Flush);
    }
    if shift > MAX_INDENT {
        return None;
    }
    if next_left > previous_left {
        // Indented under a margin line: the wrapped half of a hanging entry. A full indented
        // line is only a continuation when another indented line follows; alone it is the
        // indented first line of a new paragraph.
        let next_is_full = next_right.saturating_add(EDGE_TOLERANCE) >= column_right;
        let followed_by_indent =
            after.is_some_and(|after| after.location[0].abs_diff(next_left) <= EDGE_TOLERANCE);
        return (!next_is_full || followed_by_indent).then_some(Continuation::Hanging);
    }
    // Back at the margin after an indented first line, but only for the paragraph's first line;
    // later lines at a different edge belong to something else.
    (previous.count == 1 && !previous.hanging).then_some(Continuation::Returning)
}

/// Joins fragments the text layer painted on one line (an italic abbreviation, a lone `=`).
fn join_same_line(lines: Vec<Line>) -> Vec<Line> {
    let mut joined: Vec<Line> = Vec::with_capacity(lines.len());
    for line in lines {
        if let Some(last) = joined.last_mut() {
            let height = f32::from(
                last.location[3]
                    .saturating_sub(last.location[1])
                    .max(line.location[3].saturating_sub(line.location[1]))
                    .max(1),
            );
            let centre = |l: &[u16; 4]| (f32::from(l[1]) + f32::from(l[3])) / 2.0;
            let same_row = (centre(&last.location) - centre(&line.location)).abs() <= height * 0.6;
            let gap = line.location[0].checked_sub(last.location[2]);
            if same_row && gap.is_some_and(|gap| gap <= SAME_LINE_GAP) {
                join_words(&mut last.text, &line.text);
                last.location[2] = line.location[2].max(last.location[2]);
                last.location[1] = last.location[1].min(line.location[1]);
                last.location[3] = last.location[3].max(line.location[3]);
                continue;
            }
        }
        joined.push(line);
    }
    joined
}

/// Appends a fragment to the text before it, without a space where punctuation closes up.
fn join_words(text: &mut String, next: &str) {
    let next = next.trim();
    let closes_up =
        next.starts_with([',', '.', ';', ':', ')', ']', '%']) || text.ends_with(['(', '[']);
    if !closes_up && !text.ends_with(' ') {
        text.push(' ');
    }
    text.push_str(next);
}

/// Whether a line carries any letters or digits; a lone `-`, `'` or `•` carries nothing.
fn has_content(line: &Line) -> bool {
    line.text.chars().any(char::is_alphanumeric)
}

/// Appends a continuation line, rejoining a word hyphenated at the line end.
fn join(text: &mut String, next: &str) {
    let next = next.trim_start();
    let hyphenated = {
        let mut tail = text.chars().rev();
        tail.next() == Some('-')
            && tail.next().is_some_and(char::is_alphabetic)
            && next.chars().next().is_some_and(char::is_lowercase)
    };
    if !hyphenated {
        text.push(' ');
    }
    text.push_str(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(location: [u16; 4], text: &str) -> Node {
        Node::Located {
            location,
            inner: Box::new(Node::Paragraph { text: text.into() }),
        }
    }

    fn texts(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .map(|node| match node {
                Node::Located { inner, .. } => match &**inner {
                    Node::Paragraph { text } => text.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            })
            .collect()
    }

    fn word(left: f32, top: f32, right: f32, text: &str) -> WordBox {
        WordBox {
            left,
            top,
            right,
            text: text.into(),
        }
    }

    #[test]
    fn stray_brackets_return_to_the_word_they_touch_and_wrapped_lines_join() {
        // One grid unit is exactly one point on this 511-point page.
        let page = PageWords {
            width: GRID,
            height: GRID,
            words: vec![
                word(282.0, 121.0, 285.0, "("),
                word(285.0, 121.0, 307.0, "Durlak,"),
                word(310.0, 121.0, 330.0, "1997"),
                word(330.0, 121.0, 333.0, ")"),
                word(363.0, 141.0, 366.0, "("),
                word(366.0, 141.0, 390.0, "Bloom,"),
                word(277.0, 148.0, 280.0, ")"),
                word(262.0, 148.0, 277.0, "1953"),
            ],
        };
        let nodes = vec![
            line(
                [262, 115, 461, 120],
                "Although prevention approaches have existed since the early",
            ),
            line([282, 121, 285, 127], "("),
            line([330, 121, 333, 127], ")"),
            line(
                [262, 121, 461, 127],
                "1900s Durlak, 1997 , the modern prevention movement related to the well-being",
            ),
            line(
                [262, 135, 461, 140],
                "years ago. It grew out of public health and epidemiological",
            ),
            line([363, 141, 366, 147], "("),
            line(
                [262, 141, 461, 147],
                "approaches to disease prevention Bloom, 1996; Leavell & Clark,",
            ),
            line([277, 148, 280, 153], ")"),
            line([262, 148, 300, 153], "1953 and also"),
        ];
        let out = texts(&reflow_lines(nodes, &[page]));
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(
            out[0].contains("1900s (Durlak, 1997), the modern"),
            "{}",
            out[0]
        );
        assert!(
            out[0].contains("prevention (Bloom, 1996; Leavell & Clark, 1953) and also"),
            "{}",
            out[0]
        );
    }

    #[test]
    fn hanging_indent_continuations_keep_each_reference_entry_together() {
        // Entries start at the margin (left 60); their wrapped lines are indented (left 72).
        let nodes = vec![
            line(
                [60, 100, 460, 106],
                "Afful, S. E., Strickland, J. R., Cottler, L., & Bierut, L. J. (2010). Exposure to trauma",
            ),
            line(
                [72, 107, 460, 113],
                "a comparison of cocaine-dependent cases and a community-matched sample. Drug and",
            ),
            line([72, 114, 200, 120], "Alcohol Dependence, 112, 46-53."),
            line(
                [60, 121, 460, 127],
                "Brown, M. J., Perera, R. A., Masho, S. W., Mezuk, B., & Cohen, S. A. (2015). Adverse",
            ),
            line(
                [72, 128, 460, 134],
                "childhood experiences and intimate partner aggression in the US: Sex differences and",
            ),
            line([72, 135, 160, 141], "similarities, 131, 48-57."),
            line(
                [60, 142, 460, 148],
                "Carlson, B. E., Shafer, M., & Duffee, D. (2010). Traumatic stress among women in drug",
            ),
            line([72, 149, 300, 155], "treatment, 23, 827-840."),
        ];
        let out = texts(&reflow_lines(nodes, &[]));
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(
            out[0].starts_with("Afful") && out[0].ends_with("112, 46-53."),
            "{out:?}"
        );
        assert!(
            out[1].starts_with("Brown") && out[1].ends_with("131, 48-57."),
            "{out:?}"
        );
    }

    #[test]
    fn prose_with_indented_first_lines_still_splits_into_paragraphs() {
        let nodes = vec![
            line(
                [60, 100, 460, 106],
                "First paragraph runs across the whole width of the column and wraps to a",
            ),
            line([60, 107, 300, 113], "second line that stops short."),
            line(
                [72, 114, 460, 120],
                "Second paragraph starts with an indent and also runs across the full width",
            ),
            line([60, 121, 250, 127], "before it ends."),
        ];
        assert_eq!(texts(&reflow_lines(nodes, &[])).len(), 2);
    }

    #[test]
    fn fragments_painted_on_one_line_are_rejoined() {
        let nodes = vec![
            line([60, 100, 150, 106], "fusiform face area; IOG"),
            line([152, 100, 158, 106], "="),
            line([160, 100, 260, 106], "inferior occipital gyrus;"),
            line([60, 114, 150, 120], "TS"),
            line([152, 114, 158, 120], "="),
            line([160, 114, 300, 120], "temporal sulcus of the brain"),
        ];
        let out = texts(&reflow_lines(nodes, &[]));
        assert_eq!(
            out,
            vec![
                "fusiform face area; IOG = inferior occipital gyrus;",
                "TS = temporal sulcus of the brain"
            ]
        );
    }

    #[test]
    fn punctuation_only_lines_are_dropped() {
        let nodes = vec![
            line(
                [60, 100, 460, 106],
                "A sentence that is real text and worth keeping around.",
            ),
            line([60, 140, 66, 146], "-"),
            line([60, 180, 66, 186], "'"),
            line(
                [60, 220, 460, 226],
                "Another sentence that is worth keeping as its own paragraph.",
            ),
        ];
        assert_eq!(texts(&reflow_lines(nodes, &[])).len(), 2);
    }

    #[test]
    fn running_headers_repeated_across_pages_are_dropped_but_body_text_stays() {
        let page = |number: usize| Node::PageInfo {
            page_no: number,
            width: 595.0,
            height: 842.0,
        };
        let mut nodes = Vec::new();
        for number in 1..=4 {
            nodes.push(page(number));
            nodes.push(line([200, 10, 330, 16], "ZYCH AND NASAESCU"));
            nodes.push(line(
                [60, 200, 460, 206],
                &format!("Body text of page {number} that is unique and stays in the document."),
            ));
        }
        // One occurrence in the margin band is not running furniture.
        nodes.push(page(5));
        nodes.push(line([200, 10, 330, 16], "Chapter Two"));
        let out: Vec<String> = texts(&reflow_lines(nodes, &[]))
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect();

        assert_eq!(out.len(), 5, "{out:?}");
        assert!(out.iter().all(|text| !text.contains("ZYCH")));
        assert!(out.iter().any(|text| text == "Chapter Two"));
    }

    #[test]
    fn a_short_line_or_an_indent_starts_a_new_paragraph() {
        let nodes = vec![
            line(
                [100, 10, 400, 16],
                "A full first line that reaches the right edge",
            ),
            line([100, 17, 250, 23], "short last line."),
            line(
                [100, 24, 400, 30],
                "Next paragraph begins here and runs to the edge",
            ),
            line([112, 31, 400, 37], "indented start of another paragraph"),
        ];
        let out = texts(&reflow_lines(nodes, &[]));
        assert_eq!(
            out,
            vec![
                "A full first line that reaches the right edge short last line.",
                "Next paragraph begins here and runs to the edge",
                "indented start of another paragraph",
            ]
        );
    }

    #[test]
    fn hyphenated_words_are_rejoined_and_other_nodes_pass_through() {
        let nodes = vec![
            line([100, 10, 400, 16], "protective well-"),
            line([100, 17, 400, 23], "being of youth"),
            Node::PageBreak,
            line([100, 10, 400, 16], "next page"),
        ];
        let out = reflow_lines(nodes, &[]);
        assert_eq!(texts(&out)[0], "protective well-being of youth");
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn only_dotted_outline_numbers_are_contents_numbers() {
        assert!(is_bare_section_number("1."));
        assert!(is_bare_section_number("2.3."));
        assert!(!is_bare_section_number("25.1"));
        assert!(!is_bare_section_number("2003"));
        assert!(!is_bare_section_number("."));
    }
}
