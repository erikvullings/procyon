//! Logical roles of text-layer paragraphs, read from typography.
//!
//! The text layer is flat: every line is a paragraph. The type runs of [`crate::pdf_styles`] say
//! which paragraphs are set larger or bolder than the body (headings, so chunks can start at
//! section starts), which carry raised citation markers (stripped from the prose, where
//! `lifespan.5` is noise for readers and embeddings), and which are footnotes (smaller text at
//! the foot of the page, kept as their own units instead of trailing the last body paragraph).

use std::collections::BTreeMap;

use docling_core::Node;

use crate::pdf_styles::{PageStyles, Run};
use crate::reflow::GRID;

/// What a paragraph is, as far as typography can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Body,
    /// A section heading; level 1 is the largest style in the document.
    Heading(u8),
    /// Footnote text, set smaller at the foot of the page.
    Footnote,
}

/// Longest heading, in characters; longer text is a paragraph that happens to be bold or large.
const MAX_HEADING_CHARS: usize = 140;
/// Characters of the text before a marker used to anchor it in the paragraph text.
const HOST_TAIL_CHARS: usize = 6;
/// Most words of any heading; longer text is a paragraph set in a heading style.
const MAX_HEADING_WORDS: usize = 16;
/// Most words of a heading recognised only by its face.
const MAX_FACE_HEADING_WORDS: usize = 12;
/// Most text lines a heading may wrap over.
const MAX_HEADING_LINES: f32 = 3.3;
/// A heading set larger than the body is at least this much larger.
const LARGER_THAN_BODY: f32 = 1.12;
/// A face this much wider than the body face counts as emphasis.
const WIDER_FACE: f32 = 1.04;
/// Footnote text is at most this share of the body size.
const FOOTNOTE_SIZE: f32 = 0.93;
/// Fewest characters and words of a footnote, and the text width that must sit above it.
const MIN_FOOTNOTE_CHARS: usize = 20;
const MIN_FOOTNOTE_WORDS: usize = 3;
const MIN_BODY_ABOVE: f32 = 150.0;
/// Footnotes sit below this share of the page height.
const FOOTNOTE_ZONE: f32 = 0.5;
/// Raised markers are at most this share of the size of the text they follow.
const MARKER_SIZE: f32 = 0.84;
/// Most page units between the boxes of a heading wrapped over two lines.
const WRAPPED_HEADING_GAP: u16 = 5;
/// Consecutive headings from which a block counts as front matter rather than structure.
const MAX_HEADING_RUN: usize = 3;
/// Deepest heading level assigned.
const MAX_LEVEL: u8 = 4;

/// Size key: the font size in half points.
fn size_key(size: f32) -> i32 {
    (size * 2.0).round() as i32
}

/// A style a heading can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Look {
    size: i32,
    bold: bool,
    italic: bool,
    font: u32,
}

impl Look {
    fn of(run: &Run) -> Self {
        Self {
            size: size_key(run.size),
            bold: run.bold,
            italic: run.italic,
            font: run.font,
        }
    }
}

/// The body style of a document, and the letter width of every font in it.
struct Body {
    look: Look,
    widths: BTreeMap<u32, f32>,
}

/// The typography found inside the box of one paragraph.
struct Found<'a> {
    runs: Vec<&'a Run>,
    dominant: Option<Look>,
}

fn runs_in<'a>(page: &'a PageStyles, location: [u16; 4]) -> Found<'a> {
    let scale_x = page.width / GRID;
    let scale_y = page.height / GRID;
    let left = f32::from(location[0]) * scale_x - 3.0;
    let top = f32::from(location[1]) * scale_y;
    let right = f32::from(location[2]) * scale_x + 3.0;
    let bottom = f32::from(location[3]) * scale_y;
    // A baseline sits in the lower part of its line box; the previous line's baseline sits just
    // above the box, the next one well below it.
    let floor = top + (0.3 * (bottom - top)).min(3.0);
    let runs: Vec<&Run> = page
        .runs
        .iter()
        .filter(|run| {
            run.right >= left
                && run.x <= right
                && run.baseline >= floor
                && run.baseline <= bottom + 3.0
        })
        .collect();
    let mut weights: BTreeMap<Look, f32> = BTreeMap::new();
    for run in &runs {
        let weight = (run.right - run.x).max(0.1);
        *weights.entry(Look::of(run)).or_default() += weight;
    }
    Found {
        dominant: weights
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(look, _)| look),
        runs,
    }
}

/// The body style of a document: the most common non-bold style, by text width.
fn body_of(pages: &[PageStyles]) -> Option<Body> {
    let mut weights: BTreeMap<Look, f32> = BTreeMap::new();
    let mut widths: BTreeMap<u32, f32> = BTreeMap::new();
    for run in pages.iter().flat_map(|page| &page.runs) {
        *weights.entry(Look::of(run)).or_default() += (run.right - run.x).max(0.0);
        if run.letter_width > 0.0 {
            widths.insert(run.font, run.letter_width);
        }
    }
    let regular = weights
        .iter()
        .filter(|(look, _)| !look.bold)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(look, _)| *look);
    let look = regular.or_else(|| {
        weights
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(look, _)| look)
    })?;
    Some(Body { look, widths })
}

/// A caption or table label, not a heading, however it is set.
fn is_label(text: &str) -> bool {
    let lower = text.trim_start().to_lowercase();
    [
        "table",
        "tabel",
        "figure",
        "figuur",
        "fig.",
        "fig ",
        "afbeelding",
        "box",
        "appendix table",
    ]
    .iter()
    .any(|prefix| {
        lower
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.trim_start().starts_with(|c: char| c.is_ascii_digit()))
    })
}

/// Whether `text` reads like a heading: short, lettered, not a caption, label or sentence.
fn plausible_heading_text(text: &str) -> bool {
    let trimmed = text.trim();
    let letters = trimmed.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 3
        || trimmed.chars().count() > MAX_HEADING_CHARS
        || trimmed.split_whitespace().count() > MAX_HEADING_WORDS
        || is_label(trimmed)
    {
        return false;
    }
    // Headings start with a capital, a digit or a symbol; a lowercase start is a sentence fragment.
    if trimmed
        .chars()
        .find(|c| c.is_alphanumeric())
        .is_some_and(char::is_lowercase)
    {
        return false;
    }
    // Run-in labels ("Encoder:"), contact lines and sentences are not section headings.
    if trimmed.ends_with(':') || trimmed.contains('@') {
        return false;
    }
    let words = trimmed.split_whitespace().count();
    !(trimmed.ends_with('.') && !starts_with_number(trimmed) && words > 3)
}

/// Whether a style stands out from the body: larger, or bold where the body is not.
fn stands_out(look: Look, body: &Body) -> Option<Emphasis> {
    let body_size = body.look.size as f32;
    let size = look.size as f32;
    let larger = size >= body_size * LARGER_THAN_BODY;
    let bold_lead = look.bold && !body.look.bold && size >= body_size * 0.95;
    // Many publishers name their fonts opaquely (`AdvPS9488`); a heading is then the face that
    // is not the body face and sets wider letters at the body size.
    let other_face = look.font != body.look.font
        && !look.italic
        && size >= body_size * 0.95
        && matches!(
            (body.widths.get(&look.font), body.widths.get(&body.look.font)),
            (Some(own), Some(base)) if *own >= *base * WIDER_FACE
        );
    if larger || bold_lead {
        Some(Emphasis::Strong)
    } else {
        other_face.then_some(Emphasis::Face)
    }
}

/// How a heading style stands out from the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Emphasis {
    /// Larger, or bold where the body is not.
    Strong,
    /// Only a different, wider face: short text only, since captions and notes use other faces.
    Face,
}

fn heading_candidate(
    text: &str,
    location: [u16; 4],
    found: &Found<'_>,
    body: &Body,
    page: &PageStyles,
) -> Option<Look> {
    let look = found.dominant?;
    let emphasis = stands_out(look, body)?;
    if !plausible_heading_text(text)
        || (emphasis == Emphasis::Face && text.split_whitespace().count() > MAX_FACE_HEADING_WORDS)
    {
        return None;
    }
    let height = f32::from(location[3].saturating_sub(location[1])) * page.height / GRID;
    let size = look.size as f32 / 2.0;
    // Nearly all of the text must be in the heading style: a bold lead-in is not a heading.
    let total: f32 = found
        .runs
        .iter()
        .map(|run| (run.right - run.x).max(0.1))
        .sum();
    let styled: f32 = found
        .runs
        .iter()
        .filter(|run| same_style(run, look))
        .map(|run| (run.right - run.x).max(0.1))
        .sum();
    (height <= MAX_HEADING_LINES * size.max(1.0) * 1.25 && styled >= 0.85 * total).then_some(look)
}

/// A heading line that reflow joined to the paragraph below it (a full-width heading counts as
/// a full line): the leading lines set in a heading style over a body-style remainder.
fn split_leading_heading(
    text: &str,
    found: &Found<'_>,
    body: &Body,
) -> Option<(String, String, Look)> {
    let mut lines: Vec<(f32, Vec<&Run>)> = Vec::new();
    let mut runs = found.runs.clone();
    runs.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.x.total_cmp(&b.x)));
    for run in runs {
        match lines.last_mut() {
            Some((baseline, line)) if (run.baseline - *baseline).abs() < 1.5 => line.push(run),
            _ => lines.push((run.baseline, vec![run])),
        }
    }
    let look_of = |line: &[&Run]| -> Option<Look> {
        let first = line.first()?;
        let look = Look::of(first);
        line.iter().all(|run| same_style(run, look)).then_some(look)
    };
    let heading_look =
        look_of(&lines.first()?.1).filter(|look| stands_out(*look, body).is_some())?;
    let count = lines
        .iter()
        .take(MAX_HEADING_LINES as usize)
        .take_while(|(_, line)| look_of(line) == Some(heading_look))
        .count();
    if count == 0 || count >= lines.len() {
        return None;
    }
    let mut heading_text = String::new();
    for (_, line) in &lines[..count] {
        for run in line {
            heading_text.push_str(&run.text);
        }
        heading_text.push(' ');
    }
    let wanted: Vec<char> = heading_text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if wanted.is_empty() || !plausible_heading_text(&heading_text) {
        return None;
    }
    // Walk the paragraph text until it has spelled out the heading's characters.
    let mut seen = 0;
    let mut split = None;
    for (index, character) in text.char_indices() {
        if character.is_whitespace() {
            continue;
        }
        if wanted.get(seen) != Some(&character) {
            return None;
        }
        seen += 1;
        if seen == wanted.len() {
            split = Some(index + character.len_utf8());
            break;
        }
    }
    let split = split?;
    let (head, rest) = text.split_at(split);
    let rest = rest.trim_start();
    (!rest.is_empty()).then(|| (head.trim().to_owned(), rest.to_owned(), heading_look))
}

fn starts_with_number(text: &str) -> bool {
    let head: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    !head.is_empty() && head.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// The raised markers of a page: their texts, and whether any marker could not be decoded.
struct PageMarkers {
    texts: Vec<String>,
    undecoded: bool,
    /// Markers whose host text decoded, so they can be found in the paragraph text.
    anchored: Vec<Anchored>,
}

/// A raised marker and the end of the text it follows.
struct Anchored {
    marker: String,
    tail: String,
    used: bool,
}

fn page_markers(page: &PageStyles, body_size: f32) -> PageMarkers {
    let mut markers = PageMarkers {
        texts: Vec::new(),
        undecoded: false,
        anchored: Vec::new(),
    };
    let mut raised: Vec<(&Run, &Run)> = page
        .runs
        .iter()
        .filter(|run| run.size <= body_size * MARKER_SIZE)
        .filter_map(|marker| {
            page.runs
                .iter()
                .find(|host| is_raised_over(marker, host))
                .map(|host| (marker, host))
        })
        .collect();
    raised.sort_by(|a, b| {
        a.0.baseline
            .total_cmp(&b.0.baseline)
            .then(a.0.x.total_cmp(&b.0.x))
    });
    for (marker, host) in raised {
        let text = marker.text.trim();
        if text.is_empty() {
            markers.undecoded = true;
        } else if is_marker_text(text) {
            markers.texts.push(text.to_owned());
            let host_text = host.text.trim_end();
            let tail: String = {
                let chars: Vec<char> = host_text.chars().collect();
                chars[chars.len().saturating_sub(HOST_TAIL_CHARS)..]
                    .iter()
                    .collect()
            };
            if tail.chars().filter(|c| c.is_alphanumeric()).count() >= 3 {
                markers.anchored.push(Anchored {
                    marker: text.to_owned(),
                    tail,
                    used: false,
                });
            }
        }
    }
    markers
}

/// Whether `marker` is set smaller and higher than `host` and starts where `host` ends.
fn is_raised_over(marker: &Run, host: &Run) -> bool {
    host.size > marker.size * 1.15
        && host.right <= marker.x + 2.0
        && host.right >= marker.x - 4.0
        && marker.baseline < host.baseline - 0.12 * host.size
        && marker.baseline > host.baseline - 0.95 * host.size
}

/// Classifies paragraphs and strips citation markers from prose.
///
/// `pages[i]` holds the typography of page `i + 1`. Nodes without a page's typography keep
/// [`Role::Body`].
pub(crate) fn classify(nodes: Vec<Node>, pages: &[PageStyles]) -> Vec<(Node, Role)> {
    let Some(body) = body_of(pages) else {
        return nodes.into_iter().map(|node| (node, Role::Body)).collect();
    };
    let mut markers: Vec<PageMarkers> = pages
        .iter()
        .map(|page| page_markers(page, body.look.size as f32 / 2.0))
        .collect();
    // Pass 1: every paragraph's page, text, location and found style.
    struct Slot {
        index: usize,
        heading: Option<Look>,
        footnote: bool,
        /// A heading line and the body text that followed it in the same paragraph.
        lead: Option<(String, String, Look)>,
        location: [u16; 4],
    }
    let mut page = 0_usize;
    let mut slots: Vec<Slot> = Vec::new();
    let mut nodes = nodes;
    // Whether the previous paragraph was a footnote: its wrapped lines carry no marker.
    let mut after_footnote = false;
    for (index, node) in nodes.iter_mut().enumerate() {
        match node {
            Node::PageInfo { page_no, .. } => {
                page = page_no.saturating_sub(1);
                after_footnote = false;
            }
            Node::Located { location, inner } => {
                let Node::Paragraph { text } = &mut **inner else {
                    continue;
                };
                let Some(styles) = pages.get(page) else {
                    continue;
                };
                let found = runs_in(styles, *location);
                let heading = heading_candidate(text, *location, &found, &body, styles);
                let Some(page_notes) = markers.get_mut(page) else {
                    continue;
                };
                let stripped = strip_markers(text, page_notes);
                let lead = if heading.is_none() {
                    split_leading_heading(&stripped, &found, &body)
                } else {
                    None
                };
                let note_page = &*page_notes;
                let footnote = heading.is_none()
                    && lead.is_none()
                    && (is_footnote(&stripped, *location, &found, &body, styles, note_page)
                        || (after_footnote && is_small_low_text(*location, &found, &body)));
                after_footnote = footnote;
                let mut stripped = stripped;
                if footnote
                    && let Some(marker) = leading_marker(&found)
                    && let Some(rest) = stripped.trim_start().strip_prefix(marker.as_str())
                    && !rest.starts_with(char::is_whitespace)
                {
                    stripped = format!("{marker} {rest}");
                }
                *text = stripped;
                slots.push(Slot {
                    index,
                    heading,
                    footnote,
                    lead,
                    location: *location,
                });
            }
            _ => {}
        }
    }
    // Pass 2: heading levels from the distinct looks, largest first.
    let mut looks: BTreeMap<Look, usize> = BTreeMap::new();
    for slot in &slots {
        if let Some(look) = slot.heading.or(slot.lead.as_ref().map(|lead| lead.2)) {
            *looks.entry(look).or_default() += 1;
        }
    }
    let mut ranked: Vec<Look> = looks
        .iter()
        .filter(|(_, count)| **count >= 2)
        .map(|(look, _)| *look)
        .collect();
    ranked.sort_by(|a, b| b.cmp(a));
    let level_of = |look: Look| -> u8 {
        let rank = ranked
            .iter()
            .position(|other| *other <= look)
            .unwrap_or(ranked.len());
        u8::try_from(rank + 1).unwrap_or(MAX_LEVEL).min(MAX_LEVEL)
    };
    let mut output: Vec<(Node, Role)> = Vec::with_capacity(nodes.len() + 8);
    let mut by_index: BTreeMap<usize, &Slot> =
        slots.iter().map(|slot| (slot.index, slot)).collect();
    let mut previous_heading: Option<(Look, [u16; 4])> = None;
    for (index, node) in nodes.into_iter().enumerate() {
        let Some(slot) = by_index.remove(&index) else {
            previous_heading = None;
            output.push((node, Role::Body));
            continue;
        };
        if let (Some((head, rest, look)), Node::Located { location, .. }) = (&slot.lead, &node) {
            output.push((
                paragraph(*location, head.clone()),
                Role::Heading(level_of(*look)),
            ));
            output.push((paragraph(*location, rest.clone()), Role::Body));
            previous_heading = None;
            continue;
        }
        if let Some(look) = slot.heading {
            // A heading wrapped over two boxes: one heading.
            if let Some((previous_look, previous_box)) = previous_heading
                && previous_look == look
                && slot.location[1].saturating_sub(previous_box[3]) <= WRAPPED_HEADING_GAP
                && slot.location[0] < previous_box[2]
                && let Some((
                    Node::Located {
                        inner: previous, ..
                    },
                    _,
                )) = output.last_mut()
                && let (Node::Paragraph { text: first }, Node::Located { inner, .. }) =
                    (&mut **previous, &node)
                && let Node::Paragraph { text: second } = &**inner
            {
                first.push(' ');
                first.push_str(second);
                previous_heading = Some((look, slot.location));
                continue;
            }
            previous_heading = Some((look, slot.location));
            output.push((node, Role::Heading(level_of(look))));
            continue;
        }
        previous_heading = None;
        output.push((
            node,
            if slot.footnote {
                Role::Footnote
            } else {
                Role::Body
            },
        ));
    }
    demote_heading_blocks(&mut output);
    output
}

/// Title pages (journal banner, author and affiliation lists) set whole blocks in a heading
/// style. Sections do not follow each other without a paragraph between them, so a run of
/// consecutive headings is front matter, not structure.
fn demote_heading_blocks(output: &mut [(Node, Role)]) {
    let mut start = 0;
    while start < output.len() {
        if !matches!(output[start].1, Role::Heading(_)) {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < output.len() && matches!(output[end].1, Role::Heading(_)) {
            end += 1;
        }
        if end - start >= MAX_HEADING_RUN {
            for entry in &mut output[start..end] {
                entry.1 = Role::Body;
            }
        }
        start = end;
    }
}

fn paragraph(location: [u16; 4], text: String) -> Node {
    Node::Located {
        location,
        inner: Box::new(Node::Paragraph { text }),
    }
}

/// The raised number or symbol a footnote line starts with, when it is set as a small run of
/// its own (`³L. Tribe, ...`), and so carries no space in the extracted text.
fn leading_marker(found: &Found<'_>) -> Option<String> {
    let first = found
        .runs
        .iter()
        .min_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.x.total_cmp(&b.x)))?;
    let rest_size = found
        .runs
        .iter()
        .filter(|run| run.size > first.size * 1.15)
        .map(|run| run.size)
        .fold(0.0_f32, f32::max);
    (rest_size > 0.0 && is_marker_text(&first.text)).then(|| first.text.trim().to_owned())
}

/// Whether a run is set in `look`. Bold is a style of its own (a bold number and an italic
/// title still make one heading); other faces must match exactly.
fn same_style(run: &Run, look: Look) -> bool {
    if look.bold {
        size_key(run.size) == look.size && run.bold
    } else {
        Look::of(run) == look
    }
}

/// Whether the text starts like a footnote: a number or symbol, then the note.
fn starts_like_note(text: &str) -> bool {
    let trimmed = text.trim_start();
    let mut chars = trimmed.chars();
    match chars.next() {
        Some('*' | '∗' | '†' | '‡' | '§' | '¶' | '•') => true,
        Some(first) if first.is_ascii_digit() => {
            let rest = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
            trimmed.len() - rest.len() <= 3 && rest.starts_with([' ', '.', ')']) || rest.is_empty()
        }
        _ => false,
    }
}

/// Small text in the footnote zone, whatever it starts with.
fn is_small_low_text(location: [u16; 4], found: &Found<'_>, body: &Body) -> bool {
    f32::from(location[1]) / GRID >= FOOTNOTE_ZONE
        && found
            .dominant
            .is_some_and(|look| (look.size as f32) <= body.look.size as f32 * FOOTNOTE_SIZE)
}

fn is_footnote(
    text: &str,
    location: [u16; 4],
    found: &Found<'_>,
    body: &Body,
    page: &PageStyles,
    markers: &PageMarkers,
) -> bool {
    let Some(look) = found.dominant else {
        return false;
    };
    let trimmed = text.trim();
    let top = f32::from(location[1]) / GRID;
    let raised = leading_marker(found).filter(|marker| trimmed.starts_with(marker.as_str()));
    if top < FOOTNOTE_ZONE
        || (look.size as f32) > body.look.size as f32 * FOOTNOTE_SIZE
        || trimmed.chars().count() < MIN_FOOTNOTE_CHARS
        || trimmed.split_whitespace().count() < MIN_FOOTNOTE_WORDS
        || !(raised.is_some() || starts_like_note(trimmed))
    {
        return false;
    }
    // Reference lists and affiliations are small too; a footnote has body text above it.
    let note_top = f32::from(location[1]) * page.height / GRID;
    let body_above: f32 = page
        .runs
        .iter()
        .filter(|run| {
            run.baseline < note_top - 8.0 && run.size >= body.look.size as f32 / 2.0 * 0.97
        })
        .map(|run| run.right - run.x)
        .sum();
    if body_above < MIN_BODY_ABOVE {
        return false;
    }
    // A number must be one of the page's raised markers; symbols mark footnotes on their own.
    let marker: String = raised.unwrap_or_else(|| {
        trimmed
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect::<String>()
            .trim_end_matches(['.', ')'])
            .to_owned()
    });
    marker.chars().all(|c| !c.is_ascii_digit())
        || markers.undecoded
        || markers
            .texts
            .iter()
            .any(|text| text.trim_end_matches(',') == marker)
}

/// Removes the page's raised citation markers (`lifespan.5`, `results, 11`) from a paragraph.
/// Each marker is found by the end of the text it follows and removed once.
fn strip_markers(text: &str, markers: &mut PageMarkers) -> String {
    let mut result = text.to_owned();
    for anchored in markers
        .anchored
        .iter_mut()
        .filter(|anchored| !anchored.used)
    {
        if let Some((start, end)) = find_marker(&result, 0, &anchored.marker, &anchored.tail) {
            result.replace_range(start..end, "");
            anchored.used = true;
        }
    }
    result
}

fn is_marker_text(text: &str) -> bool {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    (1..=6).contains(&count)
        && trimmed.chars().all(|c| {
            c.is_ascii_digit() || matches!(c, ',' | '-' | '–' | '*' | '†' | '‡' | '§' | '¶' | '#')
        })
        || (count == 1 && trimmed.chars().all(|c| c.is_ascii_lowercase()))
}

/// Quotes and dashes as docling writes them in paragraph text, so they compare equal to the
/// characters the font decodes to.
fn fold(character: char) -> char {
    match character {
        '“' | '”' | '"' | '‘' | '’' | '\'' => '\'',
        '‐' | '‑' | '–' | '—' | '−' => '-',
        other => other,
    }
}

fn ends_with_folded(text: &str, tail: &str) -> bool {
    let wanted: Vec<char> = tail.chars().map(fold).collect();
    let actual: Vec<char> = text.chars().rev().take(wanted.len()).map(fold).collect();
    actual.len() == wanted.len() && actual.iter().rev().eq(wanted.iter())
}

/// The next place at or after `from` where `marker` follows the end of its host text (with at
/// most a space between) and is followed by a space, punctuation or the end. Returns the span
/// to delete: the marker and the space before it.
fn find_marker(text: &str, from: usize, marker: &str, host: &str) -> Option<(usize, usize)> {
    let tail: String = {
        let chars: Vec<char> = host.chars().collect();
        chars[chars.len().saturating_sub(HOST_TAIL_CHARS)..]
            .iter()
            .collect()
    };
    let closes = |end: usize| {
        text[end..].chars().next().is_none_or(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '.' | ',' | ';' | ':' | ')' | ']' | '"' | '”' | '’' | '\''
                )
        })
    };
    let mut offset = from;
    while let Some(found) = text.get(offset..)?.find(marker) {
        let start = offset + found;
        let end = start + marker.len();
        offset = end;
        if !closes(end) {
            continue;
        }
        let before = &text[..start];
        let spaced = before.trim_end_matches(' ');
        let gap = before.len() - spaced.len();
        // Anchored on the host's own text when the font decoded it; otherwise the marker must
        // sit glued to a word or follow closing punctuation.
        let anchored = !tail.trim().is_empty() && ends_with_folded(spaced, tail.trim_end());
        if anchored && gap <= 1 {
            return Some((start - gap, end));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(x: f32, right: f32, baseline: f32, size: f32, text: &str) -> Run {
        Run {
            x,
            right,
            baseline,
            size,
            bold: false,
            italic: false,
            font: 1,
            letter_width: 500.0,
            text: text.to_owned(),
        }
    }

    #[test]
    fn glued_markers_are_found_after_words_and_punctuation() {
        assert_eq!(
            find_marker("the lifespan.5 As many", 0, "5", "the lifespan."),
            Some((13, 14))
        );
        assert_eq!(
            find_marker(
                "toxic stress. 11-13 Attachment",
                0,
                "11-13",
                "toxic stress."
            ),
            Some((13, 19))
        );
        assert_eq!(find_marker("in 1998 it", 0, "5", "lifespan."), None);
        assert_eq!(find_marker("see page 12", 0, "12", "lifespan."), None);
        assert_eq!(
            find_marker("potential life,' 1  it", 0, "1", "al life,\u{201d}"),
            Some((16, 18))
        );
    }

    #[test]
    fn raised_small_digits_after_a_word_are_stripped() {
        let host = run(10.0, 90.0, 100.0, 10.0, "the lifespan.");
        let marker = run(90.0, 94.0, 96.0, 6.5, "5");
        let page = PageStyles {
            width: 612.0,
            height: 792.0,
            runs: vec![host, marker],
        };
        let mut markers = page_markers(&page, 10.0);
        assert_eq!(
            strip_markers("the lifespan.5 As many", &mut markers),
            "the lifespan. As many"
        );
        // Each marker is removed once.
        assert_eq!(
            strip_markers("later lifespan.5 text", &mut markers),
            "later lifespan.5 text"
        );
    }

    #[test]
    fn labels_are_not_headings() {
        assert!(is_label("Table 3. Results"));
        assert!(is_label("Figure 12"));
        assert!(!is_label("Tables and figures"));
    }
}
