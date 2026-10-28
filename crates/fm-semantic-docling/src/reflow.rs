//! Rebuilds running text from the text-layer PDF lines Docling reports.
//!
//! The text-layer path hands over one located paragraph per *line*, and a bracket set in a
//! different font than its neighbours (`(Durlak, 1997)`) comes back as its own tiny cell instead
//! of staying in the line. Left alone, a wrapped sentence becomes several paragraphs and a
//! citation loses its brackets. This pass puts each stray bracket back into the line it was
//! painted in, then joins the lines of one paragraph.

use docling_core::Node;

/// Horizontal tolerance, in Docling's 0–511 page units, for "same left edge" and "reaches the
/// right edge".
const EDGE_TOLERANCE: u16 = 3;
/// Lines of context used to find the right edge of the column a line belongs to.
const COLUMN_WINDOW: usize = 12;
/// Widest vertical gap, in line heights, between two lines of one paragraph.
const MAX_LINE_GAP: f32 = 2.2;

/// A located one-line paragraph.
struct Line {
    location: [u16; 4],
    text: String,
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
    let mut output = Vec::with_capacity(nodes.len());
    let mut run: Vec<Line> = Vec::new();
    let mut page = 0_usize;
    for node in nodes {
        match node {
            Node::Located {
                location,
                inner: boxed,
            } => match *boxed {
                Node::Paragraph { text } => run.push(Line { location, text }),
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

fn flush(run: &mut Vec<Line>, output: &mut Vec<Node>, page: Option<&PageWords>) {
    if run.is_empty() {
        return;
    }
    let lines = merge_lines(restore_brackets(std::mem::take(run), page));
    output.extend(lines.into_iter().map(|line| Node::Located {
        location: line.location,
        inner: Box::new(Node::Paragraph { text: line.text }),
    }));
}

fn is_bracket(text: &str) -> Option<char> {
    let mut chars = text.trim().chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('(' | ')' | '[' | ']')), None) => Some(c),
        _ => None,
    }
}

/// A lone outline number such as `1.` or `2.3`, the orphaned left column of a contents line.
pub(crate) fn is_bare_section_number(text: &str) -> bool {
    let trimmed = text.trim().trim_end_matches('.');
    !trimmed.is_empty()
        && trimmed.len() <= 8
        && trimmed
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
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
                })
                .map(|other| other.location[2])
                .max()
                .unwrap_or(lines[index].location[2])
        })
        .collect();
    let mut merged: Vec<Line> = Vec::with_capacity(lines.len());
    let mut previous_right = 0_u16;
    for (index, line) in lines.into_iter().enumerate() {
        if let Some(last) = merged.last_mut()
            && continues(last, previous_right, &line, column_right[index])
        {
            join(&mut last.text, &line.text);
            last.location[2] = last.location[2].max(line.location[2]);
            last.location[3] = line.location[3];
            previous_right = line.location[2];
            continue;
        }
        previous_right = line.location[2];
        merged.push(line);
    }
    merged
}

fn continues(previous: &Line, previous_right: u16, next: &Line, column_right: u16) -> bool {
    let [previous_left, _, _, previous_bottom] = previous.location;
    let [next_left, next_top, next_right, _] = next.location;
    let height = f32::from(
        previous.location[3]
            .saturating_sub(previous.location[1])
            .max(1),
    );
    // A last line stops short of the column's right edge; a first line is indented.
    let previous_is_full = previous_right.saturating_add(EDGE_TOLERANCE) >= column_right;
    previous_is_full
        && next_left.abs_diff(previous_left) <= EDGE_TOLERANCE
        && next_right <= column_right.saturating_add(EDGE_TOLERANCE)
        && next_top.saturating_add(2) >= previous_bottom
        && f32::from(next_top.saturating_sub(previous_bottom)) <= MAX_LINE_GAP * height
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
}
