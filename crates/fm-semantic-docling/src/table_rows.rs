//! Rebuilds borderless statistics tables from the one-paragraph-per-cell text-layer output.
//!
//! The text-layer path has no table model: every cell of a results table arrives as its own
//! located paragraph. Following the document-parsing protocol's rules for whitespace-aligned
//! tables, a block is only a table when several consecutive visual rows each hold more than one
//! cell and the cells are datum-like (short, numeric or marks). Column tracks are read off the
//! widest row, every cell lands in the track it overlaps, and missing positions are padded with
//! empty cells, never with invented values. Running prose and two-column text fail the gate.

use docling_core::Table;

/// Most cells a vertical gap between rows may span before the block is cut, in row heights.
const MAX_ROW_GAP: f32 = 3.0;
/// Fewest multi-cell rows that make a table.
const MIN_TABLE_ROWS: usize = 3;
/// Fewest cells that make a table.
const MIN_TABLE_CELLS: usize = 8;
/// Longest cell, in characters, a table row may contain; a row with a running sentence is prose
/// with a figure or formula beside it and never belongs to a table.
const MAX_CELL_CHARS: usize = 100;
/// Fewest non-stub cells with letters or digits that make a table.
const MIN_JUDGED_CELLS: usize = 4;
/// Longest cell, in characters, that still reads as a datum rather than prose.
const MAX_DATUM_CHARS: usize = 24;
/// Longest lone label, in characters, tolerated between two multi-cell rows.
const MAX_LABEL_CHARS: usize = 40;
/// Share of cells that must be datum-like.
const MIN_DATUM_SHARE: f32 = 0.6;
/// Widest gap, in page units, between a lone sign or bracket and the value it belongs to.
const MARK_GAP: u16 = 12;
/// Most rows a caption may span above its table.
const MAX_CAPTION_ROWS: usize = 3;
/// Widest vertical gap, in row heights, between a caption and the next row.
const MAX_CAPTION_GAP: f32 = 3.5;
/// Widest vertical gap, in row heights, between a label and its wrapped continuation.
const MAX_CONTINUATION_GAP: f32 = 1.8;
/// Words that open a table or figure caption (compared lower-cased, followed by a number).
const CAPTION_WORDS: &[&str] = &[
    "table",
    "tabel",
    "tab.",
    "tableau",
    "tabelle",
    "tabla",
    "tabella",
    "tabela",
    "figure",
    "fig.",
    "figuur",
    "abbildung",
];

/// A located one-line cell as the text layer reports it (`[left, top, right, bottom]`).
pub(crate) struct Cell<'a> {
    pub location: [u16; 4],
    pub text: &'a str,
}

/// A reconstructed table and the indices of the input cells it consumed.
pub(crate) struct TableBlock {
    pub members: Vec<usize>,
    pub table: Table,
}

struct Row {
    cells: Vec<usize>,
    center: f32,
    height: f32,
}

/// A working cell: one or more input cells fused (a lone sign or bracket and its value).
struct Work {
    location: [u16; 4],
    text: String,
    sources: Vec<usize>,
}

/// Finds every table in `cells`, which must all belong to one page.
pub(crate) fn find_tables(cells: &[Cell<'_>]) -> Vec<TableBlock> {
    let work = merge_marks(cells);
    let view: Vec<Cell<'_>> = work
        .iter()
        .map(|cell| Cell {
            location: cell.location,
            text: &cell.text,
        })
        .collect();
    let rows = visual_rows(&view);
    let mut blocks = Vec::new();
    let mut index = 0;
    // First row not yet claimed by an earlier table; captions are searched above `index` only.
    let mut floor = 0;
    while index < rows.len() {
        if rows[index].cells.len() < 2 || has_sentence(&view, &rows[index]) {
            index += 1;
            continue;
        }
        // A caption set on one line with its number (`Table 2` | `Results…`) is itself a
        // multi-cell row; the table starts below it.
        let caption_row = row_text(&view, &rows[index]);
        let (table_start, start_hint) = if is_caption_start(&caption_row) {
            (index + 1, Some(index))
        } else {
            (index, None)
        };
        if table_start >= rows.len() {
            break;
        }
        let end = block_end(&view, &rows, table_start);
        if let Some(mut block) = build(&view, &rows[table_start..end]) {
            let start =
                start_hint.unwrap_or_else(|| caption_start(&view, &rows, table_start, floor));
            if start < table_start {
                let caption = rows[start..table_start]
                    .iter()
                    .flat_map(|row| row.cells.iter().map(|&cell| view[cell].text.trim()))
                    .collect::<Vec<_>>()
                    .join(" ");
                block.table.caption = Some(crate::reflow::unescape(&caption));
                for row in &rows[start..table_start] {
                    block.members.extend(row.cells.iter().copied());
                }
            }
            // Members so far index the working cells; report the input cells they fuse.
            block.members = block
                .members
                .iter()
                .flat_map(|&member| work[member].sources.iter().copied())
                .collect();
            blocks.push(block);
            index = end;
            floor = end;
        } else {
            index += 1;
        }
    }
    blocks
}

fn has_sentence(cells: &[Cell<'_>], row: &Row) -> bool {
    row.cells
        .iter()
        .any(|&cell| cells[cell].text.chars().count() > MAX_CELL_CHARS)
}

fn row_text(cells: &[Cell<'_>], row: &Row) -> String {
    row.cells
        .iter()
        .map(|&cell| cells[cell].text.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_leading_mark(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= 2
        && text
            .chars()
            .all(|c| matches!(c, '-' | '−' | '–' | '+' | '(' | '[' | '$' | '€' | '£'))
}

fn is_trailing_mark(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= 2
        && text
            .chars()
            .all(|c| matches!(c, ')' | ']' | '%' | '*' | '⁎' | '†' | '‡'))
}

/// Fuses a lone sign, currency glyph or bracket with the value it touches on the same line, and a
/// trailing bracket, percent sign or footnote mark with the value before it. The protocol keeps
/// such marks in the cell of their value instead of a track of their own.
fn merge_marks(cells: &[Cell<'_>]) -> Vec<Work> {
    let mut work: Vec<Option<Work>> = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            Some(Work {
                location: cell.location,
                text: cell.text.to_owned(),
                sources: vec![index],
            })
        })
        .collect();
    let same_line = |a: &[u16; 4], b: &[u16; 4]| {
        let height = f32::from(
            a[3].saturating_sub(a[1])
                .max(b[3].saturating_sub(b[1]))
                .max(1),
        );
        let centre = |l: &[u16; 4]| (f32::from(l[1]) + f32::from(l[3])) / 2.0;
        (centre(a) - centre(b)).abs() <= height * 0.6
    };
    // A few passes let `( - 0.5` collapse one mark at a time.
    for _ in 0..3 {
        let mut changed = false;
        for index in 0..work.len() {
            let Some(mark) = work[index].as_ref() else {
                continue;
            };
            let text = mark.text.trim().to_owned();
            let location = mark.location;
            let leading = is_leading_mark(&text);
            let trailing = is_trailing_mark(&text);
            if !leading && !trailing {
                continue;
            }
            let partner = (0..work.len())
                .filter(|&other| other != index)
                .filter_map(|other| work[other].as_ref().map(|cell| (other, cell)))
                .filter(|(_, cell)| same_line(&location, &cell.location))
                .filter(|(_, cell)| {
                    if leading {
                        cell.location[0] >= location[2].saturating_sub(1)
                            && cell.location[0] - location[2].min(cell.location[0]) <= MARK_GAP
                    } else {
                        location[0] >= cell.location[2].saturating_sub(1)
                            && location[0] - cell.location[2].min(location[0]) <= MARK_GAP
                    }
                })
                .min_by_key(|(_, cell)| {
                    if leading {
                        cell.location[0]
                    } else {
                        u16::MAX - cell.location[2]
                    }
                })
                .map(|(other, _)| other);
            let Some(other) = partner else { continue };
            let Some(mark) = work[index].take() else {
                continue;
            };
            let Some(target) = work[other].as_mut() else {
                work[index] = Some(mark);
                continue;
            };
            let separator = if leading && matches!(text.as_str(), "(" | "[") || trailing {
                ""
            } else {
                " "
            };
            target.text = if leading {
                format!("{}{separator}{}", text, target.text.trim())
            } else {
                format!("{}{separator}{}", target.text.trim(), text)
            };
            target.location = [
                target.location[0].min(mark.location[0]),
                target.location[1].min(mark.location[1]),
                target.location[2].max(mark.location[2]),
                target.location[3].max(mark.location[3]),
            ];
            target.sources.extend(mark.sources);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    work.into_iter().flatten().collect()
}

fn is_caption_start(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    CAPTION_WORDS.iter().any(|word| {
        lower
            .strip_prefix(word)
            .is_some_and(|rest| rest.trim_start().starts_with(|c: char| c.is_ascii_digit()))
    })
}

/// First row of the caption sitting directly above the table that starts at `index`, or `index`
/// when there is none. Only rows at or after `floor` are considered.
fn caption_start(cells: &[Cell<'_>], rows: &[Row], index: usize, floor: usize) -> usize {
    for span in 1..=MAX_CAPTION_ROWS {
        let Some(start) = index.checked_sub(span).filter(|&start| start >= floor) else {
            break;
        };
        // Every row from `start` to the table must hug the next one.
        if rows[start..=index].windows(2).any(|pair| {
            pair[1].center - pair[0].center > MAX_CAPTION_GAP * pair[0].height.max(pair[1].height)
        }) {
            break;
        }
        let text = rows[start]
            .cells
            .iter()
            .map(|&cell| cells[cell].text.trim())
            .collect::<Vec<_>>()
            .join(" ");
        if is_caption_start(&text) {
            return start;
        }
    }
    index
}

/// Whether a lone line continues the wrapped label above it rather than opening a new row.
fn continues_label(previous: &str, next: &str) -> bool {
    let next = next.trim();
    let previous = previous.trim_end();
    next.starts_with(|c: char| c.is_lowercase())
        || previous.ends_with(['/', '-', ',', '('])
        || ["of", "and", "or", "in", "for", "to", "with", "the"]
            .iter()
            .any(|word| {
                previous
                    .rsplit(' ')
                    .next()
                    .is_some_and(|last| last.eq_ignore_ascii_case(word))
            })
}

fn visual_rows(cells: &[Cell<'_>]) -> Vec<Row> {
    let center =
        |cell: &Cell<'_>| (f32::from(cell.location[1]) + f32::from(cell.location[3])) / 2.0;
    let mut order: Vec<usize> = (0..cells.len()).collect();
    order.sort_by(|&a, &b| {
        center(&cells[a])
            .total_cmp(&center(&cells[b]))
            .then(cells[a].location[0].cmp(&cells[b].location[0]))
    });
    let mut rows: Vec<Row> = Vec::new();
    for index in order {
        let cell = &cells[index];
        let height = f32::from(cell.location[3].saturating_sub(cell.location[1])).max(1.0);
        let middle = center(cell);
        match rows.last_mut() {
            Some(row) if (middle - row.center).abs() <= (row.height * 0.6).max(2.0) => {
                row.cells.push(index);
                row.height = row.height.max(height);
            }
            _ => rows.push(Row {
                cells: vec![index],
                center: middle,
                height,
            }),
        }
    }
    for row in &mut rows {
        row.cells.sort_by_key(|&index| cells[index].location[0]);
    }
    rows
}

/// End (exclusive) of the run of rows starting at `start` that may belong to one table.
fn block_end(cells: &[Cell<'_>], rows: &[Row], start: usize) -> usize {
    let mut end = start + 1;
    while end < rows.len() {
        let row = &rows[end];
        let gap = row.center - rows[end - 1].center;
        if gap > MAX_ROW_GAP * row.height.max(rows[end - 1].height) {
            break;
        }
        if has_sentence(cells, row) {
            break;
        }
        let tabular = row.cells.len() >= 2;
        let lone = row.cells.len() == 1
            && cells[row.cells[0]].text.trim().chars().count() <= MAX_LABEL_CHARS;
        let label = lone
            && (rows.get(end + 1).is_some_and(|next| next.cells.len() >= 2)
                || continues_label(cells[rows[end - 1].cells[0]].text, cells[row.cells[0]].text));
        if !tabular && !label {
            break;
        }
        end += 1;
    }
    end
}

fn is_datum(text: &str) -> bool {
    let text = text.trim();
    text.chars().count() <= MAX_DATUM_CHARS && text.chars().any(|c| c.is_ascii_digit())
}

fn build(cells: &[Cell<'_>], rows: &[Row]) -> Option<TableBlock> {
    let tabular = rows.iter().filter(|row| row.cells.len() >= 2).count();
    let members: Vec<usize> = rows
        .iter()
        .flat_map(|row| row.cells.iter().copied())
        .collect();
    if tabular < MIN_TABLE_ROWS || members.len() < MIN_TABLE_CELLS {
        return None;
    }
    // Row stubs (the first cell of a row) are labels and mark-only cells (`-`, `–`, `*`) are
    // legitimate content that says nothing about tabularity, so only the remaining cells with
    // letters or digits are judged.
    let judged: Vec<usize> = rows
        .iter()
        .flat_map(|row| row.cells.iter().skip(1).copied())
        .filter(|&index| cells[index].text.chars().any(char::is_alphanumeric))
        .collect();
    let datums = judged
        .iter()
        .filter(|&&index| is_datum(cells[index].text))
        .count();
    if judged.len() < MIN_JUDGED_CELLS || (datums as f32) < MIN_DATUM_SHARE * judged.len() as f32 {
        return None;
    }

    // Column tracks come from the widest, first such row. Stray marks (`*`, a lone bracket) are
    // footnote or bracket remnants of their neighbour, not columns of their own.
    let anchors = |row: &Row| -> Vec<usize> {
        row.cells
            .iter()
            .copied()
            .filter(|&index| cells[index].text.chars().any(char::is_alphanumeric))
            .collect()
    };
    let widest =
        rows.iter().map(anchors).fold(
            Vec::new(),
            |best, row| {
                if row.len() > best.len() { row } else { best }
            },
        );
    let tracks: Vec<(u16, u16)> = widest
        .iter()
        .map(|&index| (cells[index].location[0], cells[index].location[2]))
        .collect();
    if tracks.len() < 2 {
        return None;
    }

    let mut grid: Vec<Vec<String>> = Vec::with_capacity(rows.len());
    let mut previous: Option<&Row> = None;
    for row in rows {
        if let (Some(above), [only]) = (previous, row.cells.as_slice()) {
            let cell = &cells[*only];
            let close = row.center - above.center <= MAX_CONTINUATION_GAP * above.height;
            if close
                && nearest_track(&tracks, cell.location) == 0
                && let Some(last) = grid.last_mut()
                && !last[0].is_empty()
                && continues_label(&last[0], cell.text)
            {
                last[0].push(' ');
                last[0].push_str(crate::reflow::unescape(cell.text).trim());
                previous = Some(row);
                continue;
            }
        }
        previous = Some(row);
        let mut line = vec![String::new(); tracks.len()];
        for &index in &row.cells {
            let cell = &cells[index];
            let text = crate::reflow::unescape(cell.text);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let slot = &mut line[nearest_track(&tracks, cell.location)];
            if !slot.is_empty() {
                slot.push(' ');
            }
            slot.push_str(text);
        }
        if line.iter().any(|slot| !slot.is_empty()) {
            grid.push(line);
        }
    }

    let location = members
        .iter()
        .fold([u16::MAX, u16::MAX, 0, 0], |acc, &index| {
            let l = cells[index].location;
            [
                acc[0].min(l[0]),
                acc[1].min(l[1]),
                acc[2].max(l[2]),
                acc[3].max(l[3]),
            ]
        });
    Some(TableBlock {
        members,
        table: Table {
            rows: grid,
            location: Some(location),
            ..Table::default()
        },
    })
}

/// The track a cell overlaps most, else the closest one.
fn nearest_track(tracks: &[(u16, u16)], location: [u16; 4]) -> usize {
    let [left, _, right, _] = location;
    let score = |&(track_left, track_right): &(u16, u16)| {
        let overlap = i32::from(right.min(track_right)) - i32::from(left.max(track_left));
        if overlap > 0 {
            return -overlap;
        }
        // Distance between the intervals; further is worse.
        i32::from(left.max(track_left)) - i32::from(right.min(track_right))
    };
    tracks
        .iter()
        .enumerate()
        .min_by_key(|(_, track)| score(track))
        .map_or(0, |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(left: u16, top: u16, right: u16, text: &str) -> Cell<'_> {
        Cell {
            location: [left, top, right, top + 6],
            text,
        }
    }

    fn rows_of(blocks: &[TableBlock]) -> Vec<String> {
        blocks
            .iter()
            .flat_map(|block| block.table.rows.iter().map(|row| row.join("\t")))
            .collect()
    }

    #[test]
    fn numeric_cells_on_shared_baselines_become_rows() {
        let cells = vec![
            cell(60, 100, 120, "Study"),
            cell(200, 100, 230, "d"),
            cell(280, 100, 330, "95% CI"),
            cell(60, 110, 120, "Smith"),
            cell(200, 110, 230, ".070"),
            cell(280, 110, 300, "0.145 ("),
            cell(305, 110, 308, "*"),
            cell(60, 120, 120, "Jones"),
            cell(200, 120, 230, ".120"),
            cell(280, 120, 330, "0.174, 0.463)"),
        ];
        let blocks = find_tables(&cells);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            rows_of(&blocks),
            vec![
                "Study\td\t95% CI",
                "Smith\t.070\t0.145 (*",
                "Jones\t.120\t0.174, 0.463)"
            ]
        );
        assert_eq!(blocks[0].members.len(), cells.len());
    }

    #[test]
    fn missing_cells_are_padded_not_invented() {
        let cells = vec![
            cell(60, 100, 120, "A"),
            cell(200, 100, 230, "1.0"),
            cell(300, 100, 330, "2.0"),
            cell(60, 110, 120, "B"),
            cell(300, 110, 330, "4.0"),
            cell(60, 120, 120, "C"),
            cell(200, 120, 230, "5.0"),
            cell(300, 120, 330, "6.0"),
        ];
        let rows = rows_of(&find_tables(&cells));
        assert_eq!(rows[1], "B\t\t4.0");
    }

    #[test]
    fn two_column_prose_is_not_a_table() {
        let cells = vec![
            cell(60, 100, 240, "Prevention approaches have existed since"),
            cell(280, 100, 460, "the early nineteen hundreds in many"),
            cell(60, 110, 240, "forms of public health and education"),
            cell(280, 110, 460, "programmes aimed at young children"),
            cell(60, 120, 240, "that were later studied by researchers"),
            cell(280, 120, 460, "who found small but reliable effects"),
        ];
        assert!(find_tables(&cells).is_empty());
    }

    #[test]
    fn dash_bullets_in_two_column_prose_are_not_a_table() {
        let cells = vec![
            cell(60, 100, 70, "-"),
            cell(280, 100, 290, "-"),
            cell(75, 100, 240, "mixed right wing extremism with"),
            cell(295, 100, 460, "related consequences"),
            cell(60, 110, 70, "-"),
            cell(280, 110, 290, "-"),
            cell(75, 110, 240, "xenophobia and classified"),
            cell(295, 110, 460, "including their different forms"),
            cell(60, 120, 70, "-"),
            cell(280, 120, 290, "-"),
            cell(75, 120, 240, "participants as radicalized"),
            cell(295, 120, 460, "in 2019"),
        ];
        assert!(find_tables(&cells).is_empty());
    }

    #[test]
    fn html_entities_in_cells_are_unescaped() {
        let cells = vec![
            cell(60, 100, 120, "Study &amp; sample"),
            cell(200, 100, 230, "d"),
            cell(60, 110, 120, "Smith"),
            cell(200, 110, 230, "0.1"),
            cell(60, 120, 120, "Jones"),
            cell(200, 120, 230, "0.2"),
            cell(60, 130, 120, "Brown"),
            cell(200, 130, 230, "0.3"),
            cell(60, 140, 120, "Green"),
            cell(200, 140, 230, "0.4"),
        ];
        assert_eq!(rows_of(&find_tables(&cells))[0], "Study & sample\td");
    }

    #[test]
    fn a_sentence_is_never_pulled_into_a_table() {
        let sentence = "We did not include a medium quality category because we did not expect to find enough studies to include three categories in moderator analyses";
        let cells = vec![
            cell(60, 100, 460, sentence),
            cell(60, 110, 120, "a ="),
            cell(200, 110, 230, "2.5"),
            cell(60, 120, 120, "b ="),
            cell(200, 120, 230, "3.5"),
            cell(60, 130, 120, "c ="),
            cell(200, 130, 230, "4.5"),
            cell(60, 140, 120, "d ="),
            cell(200, 140, 230, "5.5"),
            cell(300, 100, 330, "1"),
        ];
        assert!(
            find_tables(&cells)
                .iter()
                .all(|block| !block.members.contains(&0))
        );
    }

    #[test]
    fn a_lone_sign_and_bracket_stay_with_their_value() {
        let cells = vec![
            cell(60, 100, 120, "Domain"),
            cell(200, 100, 260, "Mean d (SE)"),
            cell(60, 110, 120, "Attitude"),
            cell(200, 110, 206, "−"),
            cell(209, 110, 240, "0.018"),
            cell(243, 110, 260, "(0.133)"),
            cell(60, 120, 120, "Health"),
            cell(200, 120, 206, "−"),
            cell(209, 120, 240, "0.325"),
            cell(243, 120, 260, "(0.179)"),
            cell(60, 130, 120, "Family"),
            cell(209, 130, 240, "0.250"),
            cell(243, 130, 260, "(0.032"),
            cell(261, 130, 264, ")"),
        ];
        let rows = rows_of(&find_tables(&cells));
        assert_eq!(rows[1], "Attitude\t− 0.018\t(0.133)");
        assert_eq!(rows[3], "Family\t0.250\t(0.032)");
    }

    #[test]
    fn a_caption_above_the_table_becomes_its_caption_and_is_consumed() {
        let cells = vec![
            cell(60, 86, 130, "Table 2"),
            cell(135, 86, 400, "Results after trim and fill analyses."),
            cell(60, 100, 120, "Domain"),
            cell(200, 100, 230, "d"),
            cell(60, 110, 120, "Attitude"),
            cell(200, 110, 230, "0.1"),
            cell(60, 120, 120, "Health"),
            cell(200, 120, 230, "0.2"),
            cell(60, 130, 120, "Family"),
            cell(200, 130, 230, "0.3"),
            cell(60, 140, 120, "Other"),
            cell(200, 140, 230, "0.4"),
        ];
        let blocks = find_tables(&cells);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0].table.caption.as_deref(),
            Some("Table 2 Results after trim and fill analyses.")
        );
        assert!(blocks[0].members.contains(&0) && blocks[0].members.contains(&1));
    }

    #[test]
    fn a_wrapped_row_label_is_one_row() {
        let cells = vec![
            cell(60, 100, 120, "Domain"),
            cell(200, 100, 230, "d"),
            cell(60, 110, 150, "(10) Neurocognition/"),
            cell(200, 110, 230, "0.243"),
            cell(60, 117, 150, "physiology (static)"),
            cell(60, 127, 120, "(11) Background"),
            cell(200, 127, 230, "0.207"),
            cell(60, 137, 120, "(12) Attitude"),
            cell(200, 137, 230, "0.200"),
        ];
        let rows = rows_of(&find_tables(&cells));
        assert_eq!(rows[1], "(10) Neurocognition/ physiology (static)\t0.243");
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn single_column_lines_are_not_a_table() {
        let cells: Vec<Cell<'_>> = (0..6).map(|i| cell(60, 100 + i * 10, 200, "12")).collect();
        assert!(find_tables(&cells).is_empty());
    }
}
