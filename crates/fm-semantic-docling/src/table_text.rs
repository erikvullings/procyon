//! Rebuilds text-heavy tables (study-characteristics tables, comparison matrices) from the column
//! regions of the whitespace segmentation.
//!
//! Their cells hold running text, so the numeric gate of `table_rows` rejects them and the text
//! layer reads them line by line across the columns. A region that splits into three to eight
//! columns is read as a table: every word run lands in the column it starts in, and a row ends
//! where the vertical gap between text lines grows beyond the usual line pitch.

use crate::reflow::{join, unescape};
use crate::regions::{Word, rows, runs};
use docling_core::Table;

/// Fewest columns that make a table (two are text columns).
const MIN_COLUMNS: usize = 3;
/// A gap this many times the usual line pitch separates two rows.
const ROW_GAP: f32 = 1.35;
/// Fewest rows that make a table.
const MIN_ROWS: usize = 3;
/// Most text lines a row may span before the first-column rule is tried.
const MAX_ROW_LINES: usize = 12;
/// Fewest row labels for the first-column rule.
const MIN_STUBS: usize = 4;
/// Fewest words a table holds.
const MIN_WORDS: usize = 40;

/// Reads the words `indices` of `words`, split by `corridors`, as a table, or `None` when they do
/// not form at least [`MIN_ROWS`] rows.
pub(crate) fn table_in_columns(
    words: &[Word<'_>],
    indices: &[usize],
    corridors: &[(usize, usize)],
    height: f32,
) -> Option<Table> {
    let columns = corridors.len() + 1;
    if columns < MIN_COLUMNS || indices.len() < MIN_WORDS {
        return None;
    }
    let lines = rows(words, indices, height);

    // Rows break where the gap between text lines exceeds the usual pitch.
    let tops: Vec<f32> = lines.iter().map(|line| words[line[0]].top).collect();
    let mut gaps: Vec<f32> = tops
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|gap| *gap > height * 0.5)
        .collect();
    gaps.sort_by(f32::total_cmp);
    let pitch = gaps.get(gaps.len() / 2).copied().unwrap_or(height);
    let mut starts = vec![0_usize];
    for index in 1..lines.len() {
        if tops[index] - tops[index - 1] > pitch * ROW_GAP {
            starts.push(index);
        }
    }

    // Records of equal pitch have no gaps between them: a line opening in the first column (and
    // not mid-sentence) starts a row instead, when those labels repeat (`Item 1`, `Item 2`, ...).
    let longest = starts
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .chain(starts.last().map(|last| lines.len() - last))
        .max()
        .unwrap_or(0);
    if starts.len() < MIN_ROWS || longest > MAX_ROW_LINES {
        let stubs: Vec<(usize, String)> = (0..lines.len())
            .filter_map(|index| {
                runs(words, &lines[index], corridors)
                    .into_iter()
                    .find(|(column, run)| {
                        *column == 0
                            && words[run[0]]
                                .text
                                .chars()
                                .next()
                                .is_some_and(|c| !c.is_lowercase())
                    })
                    .map(|(_, run)| {
                        let text = run.iter().map(|&i| words[i].text).collect::<Vec<_>>();
                        (index, text.join(" "))
                    })
            })
            .collect();
        let first_words: Vec<&str> = stubs
            .iter()
            .filter_map(|(_, text)| text.split_whitespace().next())
            .collect();
        let repeated = first_words.iter().any(|word| {
            first_words.iter().filter(|other| *other == word).count() * 10 >= first_words.len() * 6
        });
        if stubs.len() >= MIN_STUBS && repeated && stubs.len() > starts.len() {
            starts = std::iter::once(0)
                .chain(
                    stubs
                        .iter()
                        .map(|(index, _)| *index)
                        .filter(|&index| index > 0),
                )
                .collect();
        }
    }
    if starts.len() < MIN_ROWS {
        return None;
    }
    starts.push(lines.len());

    let mut grid: Vec<Vec<String>> = Vec::with_capacity(starts.len() - 1);
    for span in starts.windows(2) {
        let mut row = vec![String::new(); columns];
        for line in &lines[span[0]..span[1]] {
            let mut pieces = vec![String::new(); columns];
            for (column, run) in runs(words, line, corridors) {
                if !pieces[column].is_empty() {
                    pieces[column].push(' ');
                }
                let text = run
                    .iter()
                    .map(|&i| unescape(words[i].text))
                    .collect::<Vec<_>>();
                pieces[column].push_str(&text.join(" "));
            }
            for (cell, text) in row.iter_mut().zip(pieces) {
                if text.trim().is_empty() {
                    continue;
                }
                if cell.is_empty() {
                    *cell = text;
                } else {
                    join(cell, &text);
                }
            }
        }
        grid.push(row);
    }

    let extent = indices
        .iter()
        .fold([f32::MAX, f32::MAX, 0.0, 0.0], |acc, &i| {
            let w = &words[i];
            [
                acc[0].min(w.left),
                acc[1].min(w.top),
                acc[2].max(w.right),
                acc[3].max(w.top + height),
            ]
        });
    Some(Table {
        rows: grid,
        location: Some(extent.map(|v| v as u16)),
        ..Table::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflow::{PageWords, WordBox};
    use crate::regions::{Region, has_columns, segment, words_of};

    fn word(left: f32, top: f32, text: &str) -> WordBox {
        WordBox {
            left,
            top,
            right: left + text.len() as f32 * 4.0,
            text: text.to_owned(),
        }
    }

    /// Four text columns, three records of three lines each.
    fn page() -> PageWords {
        let mut words = Vec::new();
        let columns = [20.0, 155.0, 290.0, 425.0];
        for (record, base) in [80.0_f32, 140.0, 200.0].into_iter().enumerate() {
            for line in 0..3 {
                for (index, left) in columns.iter().enumerate() {
                    for step in 0..3 {
                        words.push(word(
                            left + step as f32 * 30.0 + line as f32 * 6.0,
                            base + line as f32 * 12.0,
                            &format!("r{record}c{index}l{line}w{step}"),
                        ));
                    }
                }
            }
        }
        PageWords {
            width: 600.0,
            height: 800.0,
            words,
        }
    }

    fn table_of(page: &PageWords) -> Option<Table> {
        let words = words_of(page);
        let tree = segment(&words, (0..words.len()).collect(), 6.0);
        match tree {
            Region::Columns {
                words: indices,
                corridors,
                ..
            } => table_in_columns(&words, &indices, &corridors, 6.0),
            _ => None,
        }
    }

    #[test]
    fn text_columns_with_gapped_records_become_rows_of_cells() {
        let table = table_of(&page()).expect("a table");
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[0].len(), 4);
        assert!(table.rows[0][1].starts_with("r0c1l0w0 r0c1l0w1"));
        assert!(table.rows[0][1].contains("r0c1l2w2"));
        assert!(table.rows[2][3].starts_with("r2c3l0w0"));
    }

    #[test]
    fn two_columns_are_not_a_table() {
        let mut two = page();
        two.words.retain(|w| w.left < 250.0);
        let words = words_of(&two);
        let tree = segment(&words, (0..words.len()).collect(), 6.0);
        assert!(has_columns(&tree));
        assert!(table_of(&two).is_none());
    }
}
