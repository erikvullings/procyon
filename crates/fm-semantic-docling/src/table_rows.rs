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
/// Longest cell, in characters, a rebuilt table may contain.
const MAX_CELL_CHARS: usize = 70;
/// Fewest non-stub cells with letters or digits that make a table.
const MIN_JUDGED_CELLS: usize = 4;
/// Longest cell, in characters, that still reads as a datum rather than prose.
const MAX_DATUM_CHARS: usize = 24;
/// Longest lone label, in characters, tolerated between two multi-cell rows.
const MAX_LABEL_CHARS: usize = 40;
/// Share of cells that must be datum-like.
const MIN_DATUM_SHARE: f32 = 0.6;

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

/// Finds every table in `cells`, which must all belong to one page.
pub(crate) fn find_tables(cells: &[Cell<'_>]) -> Vec<TableBlock> {
    let rows = visual_rows(cells);
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < rows.len() {
        if rows[index].cells.len() < 2 {
            index += 1;
            continue;
        }
        let end = block_end(cells, &rows, index);
        if let Some(block) = build(cells, &rows[index..end]) {
            blocks.push(block);
            index = end;
        } else {
            index += 1;
        }
    }
    blocks
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
        let tabular = row.cells.len() >= 2;
        let label = row.cells.len() == 1
            && cells[row.cells[0]].text.trim().chars().count() <= MAX_LABEL_CHARS
            && rows.get(end + 1).is_some_and(|next| next.cells.len() >= 2);
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
    // A running sentence inside the block means prose with a formula or figure beside it.
    if members
        .iter()
        .any(|&index| cells[index].text.chars().count() > MAX_CELL_CHARS)
    {
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
    for row in rows {
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
                "Smith\t.070\t0.145 ( *",
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
        let sentence =
            "We did not include a medium quality category because we did not expect enough";
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
    fn single_column_lines_are_not_a_table() {
        let cells: Vec<Cell<'_>> = (0..6).map(|i| cell(60, 100 + i * 10, 200, "12")).collect();
        assert!(find_tables(&cells).is_empty());
    }
}
