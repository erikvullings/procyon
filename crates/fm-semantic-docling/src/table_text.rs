//! Rebuilds text-heavy tables (study-characteristics tables, comparison matrices) from word boxes.
//!
//! Their cells hold running text, so the numeric gate of `table_rows` rejects them and the text
//! layer reads them line by line across the columns. A table is recognised by at least three
//! columns separated by vertical corridors that (almost) no word crosses, below its caption when
//! the page has one, else over the page body. Every word
//! lands in the column it sits in; a row ends where the vertical gap between text lines grows
//! beyond the usual line pitch. Lines that cross a corridor at the top (a wrapped caption) or at
//! the bottom (a note) are not table content.

use crate::reflow::{GRID, MARGIN_BAND, PageWords, join, to_units, unescape};
use crate::table_rows::{Cell, TableBlock, is_table_caption};
use docling_core::Table;

/// Narrowest corridor, in page units, that separates two columns.
const MIN_CORRIDOR: usize = 6;
/// Most columns; more corridors than this are gaps in sparse text, not columns.
const MAX_COLUMNS: usize = 8;
/// Share of the region's words that may touch a corridor.
const CORRIDOR_NOISE: f32 = 0.01;
/// Fewest words a table region holds.
const MIN_WORDS: usize = 60;
/// Fewest columns (corridors + 1).
const MIN_COLUMNS: usize = 3;
/// Share of the region's words every column must hold.
const MIN_COLUMN_SHARE: f32 = 0.03;
/// Largest vertical offset, in line heights, between words of one text line.
const SAME_LINE: f32 = 0.6;
/// A gap this many times the usual line pitch separates two rows.
const ROW_GAP: f32 = 1.35;
/// Widest gap, in page units, between words of one run.
const RUN_GAP: f32 = 4.0;
/// Most text lines a row may span before the first-column rule is tried.
const MAX_ROW_LINES: usize = 12;
/// Fewest row labels for the first-column rule.
const MIN_STUBS: usize = 4;
/// Fewest rows that make a table.
const MIN_ROWS: usize = 3;

struct Word<'a> {
    left: f32,
    right: f32,
    top: f32,
    text: &'a str,
}

fn centre_inside(location: [u16; 4], area: &[u16; 4]) -> bool {
    let x = (f32::from(location[0]) + f32::from(location[2])) / 2.0;
    let y = (f32::from(location[1]) + f32::from(location[3])) / 2.0;
    x >= f32::from(area[0]) - 1.0
        && x <= f32::from(area[2]) + 1.0
        && y >= f32::from(area[1]) - 1.0
        && y <= f32::from(area[3]) + 1.0
}

/// Finds the text-heavy tables of one page. `claimed` are regions of tables already found;
/// `height` is the page's typical line height in page units.
pub(crate) fn find_text_tables(
    page: &PageWords,
    cells: &[Cell<'_>],
    claimed: &[[u16; 4]],
    height: f32,
) -> Vec<TableBlock> {
    let mut captions: Vec<usize> = (0..cells.len())
        .filter(|&index| {
            is_table_caption(cells[index].text)
                && !claimed
                    .iter()
                    .any(|area| centre_inside(cells[index].location, area))
        })
        .collect();
    captions.sort_by_key(|&index| cells[index].location[1]);

    let words: Vec<Word<'_>> = page
        .words
        .iter()
        .filter(|word| !word.text.trim().is_empty())
        .map(|word| Word {
            left: to_units(word.left, page.width),
            right: to_units(word.right, page.width),
            top: to_units(word.top, page.height),
            text: word.text.as_str(),
        })
        .collect();

    // Each caption opens a region down to the next caption; a page without caption is one region
    // (running headers drop repeated captions, so continuation pages have none).
    let floor = GRID - f32::from(MARGIN_BAND);
    let regions: Vec<(Option<usize>, f32, f32)> = if captions.is_empty() {
        vec![(None, f32::from(MARGIN_BAND), floor)]
    } else {
        captions
            .iter()
            .enumerate()
            .map(|(position, &caption)| {
                let bottom = captions
                    .get(position + 1)
                    .map_or(floor, |&next| f32::from(cells[next].location[1]));
                (
                    Some(caption),
                    f32::from(cells[caption].location[3]) - 1.0,
                    bottom,
                )
            })
            .collect()
    };
    let mut blocks: Vec<TableBlock> = Vec::new();
    for (caption, top, bottom) in regions {
        let region: Vec<&Word<'_>> = words
            .iter()
            .filter(|word| word.top >= top && word.top < bottom)
            .filter(|word| {
                let location = [
                    word.left as u16,
                    word.top as u16,
                    word.right as u16,
                    (word.top + height) as u16,
                ];
                !claimed.iter().any(|area| centre_inside(location, area))
            })
            .collect();
        let Some(mut block) = build(&region, height) else {
            continue;
        };
        let [left, first, right, last] = block.table.location.unwrap_or_default();
        let head = caption.map(|index| unescape(cells[index].text).trim().to_owned());
        block.table.caption = match (head, block.table.caption.take()) {
            (Some(head), Some(rest)) => Some(format!("{head} {rest}")),
            (head, rest) => head.or(rest),
        };
        // The table consumes the cells sitting in it and the caption line above it.
        let area = [left, first, right, last];
        block.members = (0..cells.len())
            .filter(|&index| Some(index) == caption || centre_inside(cells[index].location, &area))
            .collect();
        let first = caption.map_or(first, |index| first.min(cells[index].location[1]));
        block.table.location = Some([left, first, right, last]);
        blocks.push(block);
    }
    blocks
}

/// Corridors (`(start, end)` in page units) between the columns of `words`.
fn corridors(words: &[&Word<'_>]) -> Vec<(usize, usize)> {
    let left = words.iter().map(|w| w.left).fold(f32::MAX, f32::min);
    let right = words.iter().map(|w| w.right).fold(f32::MIN, f32::max);
    let mut coverage = vec![0_u32; GRID as usize + 2];
    for word in words {
        let last = (word.right.max(0.0) as usize).min(GRID as usize + 1);
        for count in &mut coverage[word.left.max(0.0) as usize..=last] {
            *count += 1;
        }
    }
    let allowed = (words.len() as f32 * CORRIDOR_NOISE).max(1.0) as u32;
    let from = left as usize + MIN_CORRIDOR;
    let to = right as usize;
    let mut found = Vec::new();
    let mut run: Option<usize> = None;
    for (bin, &count) in coverage.iter().enumerate().take(to).skip(from) {
        if count <= allowed {
            run.get_or_insert(bin);
        } else if let Some(start) = run.take()
            && bin - start >= MIN_CORRIDOR
        {
            found.push((start, bin));
        }
    }
    found
}

/// Words grouped into text lines by vertical position, top to bottom, each left to right.
fn text_lines<'a>(words: &[&'a Word<'a>], height: f32) -> Vec<Vec<&'a Word<'a>>> {
    let mut sorted = words.to_vec();
    sorted.sort_by(|a, b| a.top.total_cmp(&b.top).then(a.left.total_cmp(&b.left)));
    let mut lines: Vec<Vec<&Word<'_>>> = Vec::new();
    let mut anchor = f32::MIN;
    for word in sorted {
        match lines.last_mut() {
            Some(line) if word.top - anchor <= height * SAME_LINE => line.push(word),
            _ => {
                anchor = word.top;
                lines.push(vec![word]);
            }
        }
    }
    for line in &mut lines {
        line.sort_by(|a, b| a.left.total_cmp(&b.left));
    }
    lines
}

fn crosses(line: &[&Word<'_>], corridors: &[(usize, usize)]) -> bool {
    line.iter().any(|word| {
        corridors
            .iter()
            .any(|&(start, end)| word.left < start as f32 && word.right > end as f32)
    })
}

fn build(region: &[&Word<'_>], height: f32) -> Option<TableBlock> {
    if region.len() < MIN_WORDS {
        return None;
    }
    let corridors = corridors(region);
    if !(MIN_COLUMNS..=MAX_COLUMNS).contains(&(corridors.len() + 1)) {
        return None;
    }
    let mut lines = text_lines(region, height);
    // A wrapped caption crosses the corridors above the table, a note below it.
    let mut caption_rest = Vec::new();
    while lines.first().is_some_and(|line| crosses(line, &corridors)) {
        caption_rest.push(lines.remove(0));
    }
    while lines.last().is_some_and(|line| crosses(line, &corridors)) {
        lines.pop();
    }
    let body: Vec<&Word<'_>> = lines.iter().flatten().copied().collect();
    // A run of words (gaps below `RUN_GAP`) belongs to the column its first word starts in, so a
    // centred header that overlaps a corridor stays whole.
    let column_of = |run: &[&Word<'_>]| {
        corridors
            .iter()
            .filter(|&&(start, _)| run[0].left >= start as f32)
            .count()
    };
    let runs_of = |line: &[&Word<'_>]| -> Vec<(usize, String)> {
        let mut runs: Vec<Vec<&Word<'_>>> = Vec::new();
        for &word in line {
            match runs.last_mut() {
                Some(run) if word.left - run[run.len() - 1].right <= RUN_GAP => run.push(word),
                _ => runs.push(vec![word]),
            }
        }
        runs.iter()
            .map(|run| {
                let text = run.iter().map(|w| unescape(w.text)).collect::<Vec<_>>();
                (column_of(run), text.join(" "))
            })
            .collect()
    };
    let columns = corridors.len() + 1;
    let mut share = vec![0_usize; columns];
    for line in &lines {
        for run in line.chunk_by(|a, b| b.left - a.right <= RUN_GAP) {
            share[column_of(run)] += run.len();
        }
    }
    let minimum = (body.len() as f32 * MIN_COLUMN_SHARE) as usize;
    if body.len() < MIN_WORDS || share.iter().any(|&count| count < minimum) {
        return None;
    }

    // Rows break where the gap between text lines exceeds the usual pitch.
    let tops: Vec<f32> = lines.iter().map(|line| line[0].top).collect();
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
    // not mid-sentence) starts a row instead.
    let longest = starts
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .chain(starts.last().map(|last| lines.len() - last))
        .max()
        .unwrap_or(0);
    if starts.len() < MIN_ROWS || longest > MAX_ROW_LINES {
        let stubs: Vec<(usize, String)> = (0..lines.len())
            .filter_map(|index| {
                runs_of(&lines[index])
                    .into_iter()
                    .find(|(column, text)| {
                        *column == 0 && text.chars().next().is_some_and(|c| !c.is_lowercase())
                    })
                    .map(|(_, text)| (index, text))
            })
            .collect();
        // Only a numbered or repeated label (`Item 1`, `Item 2`, ...) is trusted to open rows.
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
            for (column, text) in runs_of(line) {
                if !pieces[column].is_empty() {
                    pieces[column].push(' ');
                }
                pieces[column].push_str(&text);
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

    let extent = body.iter().fold([f32::MAX, f32::MAX, 0.0, 0.0], |acc, w| {
        [
            acc[0].min(w.left),
            acc[1].min(w.top),
            acc[2].max(w.right),
            acc[3].max(w.top + height),
        ]
    });
    let caption = caption_rest
        .iter()
        .map(|line| {
            line.iter()
                .map(|w| unescape(w.text))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(TableBlock {
        members: Vec::new(),
        table: Table {
            rows: grid,
            caption: (!caption.is_empty()).then_some(caption),
            location: Some(extent.map(|v| v as u16)),
            ..Table::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflow::WordBox;

    fn word(left: f32, top: f32, text: &str) -> WordBox {
        WordBox {
            left,
            top,
            right: left + text.len() as f32 * 4.0,
            text: text.to_owned(),
        }
    }

    /// Four text columns, two records of three lines each, headed by a caption.
    fn page() -> PageWords {
        let mut words = vec![word(30.0, 40.0, "TABLE"), word(60.0, 40.0, "3")];
        let columns = [30.0, 170.0, 310.0, 450.0];
        for (record, base) in [80.0_f32, 140.0, 200.0].into_iter().enumerate() {
            for line in 0..3 {
                for (index, left) in columns.iter().enumerate() {
                    for step in 0..3 {
                        words.push(word(
                            left + step as f32 * 36.0 + line as f32 * 9.0,
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

    fn caption() -> Cell<'static> {
        Cell {
            location: [25, 30, 90, 36],
            text: "TABLE 3",
        }
    }

    #[test]
    fn captioned_text_columns_become_rows_of_cells() {
        let tables = find_text_tables(&page(), &[caption()], &[], 6.0);
        assert_eq!(tables.len(), 1);
        let table = &tables[0].table;
        assert_eq!(table.caption.as_deref(), Some("TABLE 3"));
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[0].len(), 4);
        assert!(table.rows[0][1].starts_with("r0c1l0w0 r0c1l0w1"));
        assert!(table.rows[0][1].contains("r0c1l2w2"));
        assert!(table.rows[1][3].starts_with("r1c3l0w0"));
        assert_eq!(tables[0].members, vec![0]);
    }

    #[test]
    fn two_columns_are_not_a_table() {
        let mut two = page();
        two.words.retain(|w| w.left < 300.0);
        assert!(find_text_tables(&two, &[caption()], &[], 6.0).is_empty());
    }

    #[test]
    fn a_page_without_caption_is_one_table_region() {
        let tables = find_text_tables(&page(), &[], &[], 6.0);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].table.caption, None);
    }
}
