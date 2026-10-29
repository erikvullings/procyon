//! Whitespace segmentation of a page into a tree of regions.
//!
//! Whitespace is the structure and words are what it encloses. A region is cut where empty space
//! is wide relative to its own line height: vertical corridors that (almost) no word crosses
//! separate columns, wide empty horizontal strips separate bands, and rows that cross a corridor
//! (a title, a wrapped caption, a note) become bands of their own. The tree says where the
//! boundaries are; reading order and table reconstruction are decided by the consumers.

use crate::reflow::{GRID, PageWords, to_units};

/// A word in page units (`0..=511` on both axes).
pub(crate) struct Word<'a> {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub text: &'a str,
}

/// A region of the page and the words (indices into the word list) it encloses.
pub(crate) enum Region {
    /// No boundary strong enough to cut further.
    Leaf(Vec<usize>),
    /// Regions stacked top to bottom.
    Bands(Vec<Region>),
    /// Regions side by side, left to right, separated by `corridors` (`start..end` page units).
    Columns {
        words: Vec<usize>,
        corridors: Vec<(usize, usize)>,
        children: Vec<Region>,
    },
}

/// Fewest words worth cutting.
const MIN_SPLIT_WORDS: usize = 20;
/// Fewest text rows a column cut needs to be believable (justified text leaves rivers).
const MIN_COLUMN_ROWS: usize = 6;
/// Narrowest region, in page units, that can hold columns.
const MIN_COLUMN_WIDTH: f32 = 100.0;
/// Share of a region's words that may touch a corridor.
const CORRIDOR_NOISE: f32 = 0.01;
/// Narrowest corridor, in line heights, and in page units.
const MIN_CORRIDOR_LINES: f32 = 1.0;
const MIN_CORRIDOR: f32 = 4.0;
/// Distance, in page units, a corridor keeps from the edge of the region.
const EDGE: f32 = 3.0;
/// Most columns; more corridors than this are gaps in sparse text, not columns.
const MAX_COLUMNS: usize = 8;
/// Share of the words every column holds: two columns are text columns, more may include a
/// narrow label column.
const MIN_TWO_COLUMN_SHARE: f32 = 0.25;
const MIN_MANY_COLUMN_SHARE: f32 = 0.03;
/// Widest gap, in page units, between words of one run (a header or a phrase).
pub(crate) const RUN_GAP: f32 = 4.0;
/// Largest vertical offset, in line heights, between words of one row.
const SAME_ROW: f32 = 0.6;
/// Narrowest empty strip, in line heights, that cuts a region into bands.
const MIN_BAND_GAP: f32 = 0.6;
/// Recursion limit.
const MAX_DEPTH: usize = 24;

/// The page's words, in page units.
pub(crate) fn words_of(page: &PageWords) -> Vec<Word<'_>> {
    page.words
        .iter()
        .filter(|word| !word.text.trim().is_empty())
        .map(|word| Word {
            left: to_units(word.left, page.width),
            right: to_units(word.right, page.width),
            top: to_units(word.top, page.height),
            text: word.text.as_str(),
        })
        .collect()
}

/// Segments `indices` (all words when `None`) of `words`; `height` is the typical line height.
pub(crate) fn segment(words: &[Word<'_>], indices: Vec<usize>, height: f32) -> Region {
    split(words, indices, height, 0)
}

/// Whether the tree has a column cut anywhere.
pub(crate) fn has_columns(region: &Region) -> bool {
    match region {
        Region::Leaf(_) => false,
        Region::Columns { .. } => true,
        Region::Bands(children) => children.iter().any(has_columns),
    }
}

/// Words grouped into rows by vertical position, top to bottom, each row left to right.
pub(crate) fn rows(words: &[Word<'_>], indices: &[usize], height: f32) -> Vec<Vec<usize>> {
    let mut sorted = indices.to_vec();
    sorted.sort_by(|&a, &b| {
        words[a]
            .top
            .total_cmp(&words[b].top)
            .then(words[a].left.total_cmp(&words[b].left))
    });
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut anchor = f32::MIN;
    for index in sorted {
        match rows.last_mut() {
            Some(row) if words[index].top - anchor <= height * SAME_ROW => row.push(index),
            _ => {
                anchor = words[index].top;
                rows.push(vec![index]);
            }
        }
    }
    for row in &mut rows {
        row.sort_by(|&a, &b| words[a].left.total_cmp(&words[b].left));
    }
    rows
}

/// Runs of words in a row (gaps up to [`RUN_GAP`]) with the column each starts in.
pub(crate) fn runs(
    words: &[Word<'_>],
    row: &[usize],
    corridors: &[(usize, usize)],
) -> Vec<(usize, Vec<usize>)> {
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for &index in row {
        match runs.last_mut() {
            Some(run) if words[index].left - words[run[run.len() - 1]].right <= RUN_GAP => {
                run.push(index);
            }
            _ => runs.push(vec![index]),
        }
    }
    runs.into_iter()
        .map(|run| {
            let column = corridors
                .iter()
                .filter(|&&(start, _)| words[run[0]].left >= start as f32)
                .count();
            (column, run)
        })
        .collect()
}

fn split(words: &[Word<'_>], indices: Vec<usize>, height: f32, depth: usize) -> Region {
    if indices.len() < MIN_SPLIT_WORDS || depth >= MAX_DEPTH {
        return Region::Leaf(indices);
    }
    let rows = rows(words, &indices, height);
    if rows.len() >= MIN_COLUMN_ROWS
        && let Some(columns) = columns(words, &indices, &rows, height, depth)
    {
        return columns;
    }
    // Cut at the widest empty horizontal strip.
    let bottom =
        |row: &Vec<usize>| row.iter().map(|&i| words[i].top).fold(f32::MIN, f32::max) + height;
    let top = |row: &Vec<usize>| row.iter().map(|&i| words[i].top).fold(f32::MAX, f32::min);
    let widest = (0..rows.len().saturating_sub(1))
        .map(|at| (at, top(&rows[at + 1]) - bottom(&rows[at])))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match widest {
        Some((at, gap)) if gap >= MIN_BAND_GAP * height => {
            let (upper, lower) = rows.split_at(at + 1);
            let flat = |rows: &[Vec<usize>]| rows.iter().flatten().copied().collect::<Vec<_>>();
            Region::Bands(vec![
                split(words, flat(upper), height, depth + 1),
                split(words, flat(lower), height, depth + 1),
            ])
        }
        _ => Region::Leaf(indices),
    }
}

fn columns(
    words: &[Word<'_>],
    indices: &[usize],
    rows: &[Vec<usize>],
    height: f32,
    depth: usize,
) -> Option<Region> {
    let left = indices
        .iter()
        .map(|&i| words[i].left)
        .fold(f32::MAX, f32::min);
    let right = indices
        .iter()
        .map(|&i| words[i].right)
        .fold(f32::MIN, f32::max);
    if right - left < MIN_COLUMN_WIDTH {
        return None;
    }
    let mut coverage = vec![0_u32; GRID as usize + 2];
    for &index in indices {
        let last = (words[index].right.max(0.0) as usize).min(GRID as usize + 1);
        for count in &mut coverage[words[index].left.max(0.0) as usize..=last] {
            *count += 1;
        }
    }
    let allowed = (indices.len() as f32 * CORRIDOR_NOISE).max(1.0) as u32;
    let narrowest = (MIN_CORRIDOR_LINES * height).max(MIN_CORRIDOR);
    let from = (left + EDGE) as usize;
    let to = (right - EDGE) as usize;
    let mut corridors: Vec<(usize, usize)> = Vec::new();
    let mut run: Option<usize> = None;
    for (bin, &count) in coverage.iter().enumerate().take(to + 1).skip(from) {
        if count <= allowed {
            run.get_or_insert(bin);
        } else if let Some(start) = run.take()
            && (bin - start) as f32 >= narrowest
        {
            corridors.push((start, bin));
        }
    }
    if corridors.is_empty() || corridors.len() + 1 > MAX_COLUMNS {
        return None;
    }

    // A row with a word covering a whole corridor spans the columns: a title, a wrapped caption,
    // a note. It is a band of its own and the stretches between such rows are cut again.
    let spans = |row: &Vec<usize>| {
        row.iter().any(|&i| {
            corridors
                .iter()
                .any(|&(start, end)| words[i].left < start as f32 && words[i].right > end as f32)
        })
    };
    if rows.iter().any(spans) && rows.iter().any(|row| !spans(row)) {
        let mut bands: Vec<Region> = Vec::new();
        let mut stretch: Vec<usize> = Vec::new();
        let mut spanning = false;
        for row in rows {
            if spans(row) != spanning && !stretch.is_empty() {
                bands.push(split(
                    words,
                    std::mem::take(&mut stretch),
                    height,
                    depth + 1,
                ));
            }
            spanning = spans(row);
            stretch.extend(row);
        }
        bands.push(split(words, stretch, height, depth + 1));
        if bands.len() > 1 {
            return Some(Region::Bands(bands));
        }
    }

    let mut cells: Vec<Vec<usize>> = vec![Vec::new(); corridors.len() + 1];
    for row in rows {
        for (column, run) in runs(words, row, &corridors) {
            cells[column].extend(run);
        }
    }
    let minimum = if corridors.len() == 1 {
        MIN_TWO_COLUMN_SHARE
    } else {
        MIN_MANY_COLUMN_SHARE
    };
    let enough = (indices.len() as f32 * minimum) as usize;
    if cells.iter().any(|cell| cell.len() < enough.max(1)) {
        return None;
    }
    let children = cells
        .into_iter()
        .map(|cell| split(words, cell, height, depth + 1))
        .collect();
    Some(Region::Columns {
        words: indices.to_vec(),
        corridors,
        children,
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

    fn page(words: Vec<WordBox>) -> PageWords {
        PageWords {
            width: 600.0,
            height: 800.0,
            words,
        }
    }

    /// A column of `lines` rows of three words starting at `left`/`top`.
    fn column(words: &mut Vec<WordBox>, tag: &str, left: f32, top: f32, lines: usize) {
        for line in 0..lines {
            for step in 0..3 {
                words.push(word(
                    left + step as f32 * 36.0 + (line % 3) as f32 * 5.0,
                    top + line as f32 * 9.0,
                    &format!("{tag}{line}w{step}xxxxxxxx"),
                ));
            }
        }
    }

    fn texts(region: &Region, words: &[Word<'_>], out: &mut Vec<String>) {
        match region {
            Region::Leaf(indices) => {
                let mut sorted = indices.clone();
                sorted.sort_by(|&a, &b| words[a].top.total_cmp(&words[b].top));
                out.push(words[sorted[0]].text.to_owned());
            }
            Region::Bands(children) | Region::Columns { children, .. } => {
                for child in children {
                    texts(child, words, out);
                }
            }
        }
    }

    #[test]
    fn two_columns_split_left_then_right() {
        let mut raw = Vec::new();
        column(&mut raw, "L", 40.0, 100.0, 12);
        column(&mut raw, "R", 330.0, 100.0, 12);
        let page = page(raw);
        let words = words_of(&page);
        let tree = segment(&words, (0..words.len()).collect(), 6.0);
        assert!(has_columns(&tree));
        let mut order = Vec::new();
        texts(&tree, &words, &mut order);
        assert_eq!(order, vec!["L0w0xxxxxxxx", "R0w0xxxxxxxx"]);
    }

    #[test]
    fn a_spanning_title_stays_above_the_columns() {
        let mut raw = vec![word(
            40.0,
            40.0,
            "A title running across the whole page width and beyond the gutter",
        )];
        raw[0].right = 560.0;
        column(&mut raw, "L", 40.0, 100.0, 12);
        column(&mut raw, "R", 330.0, 100.0, 12);
        let page = page(raw);
        let words = words_of(&page);
        let tree = segment(&words, (0..words.len()).collect(), 6.0);
        let mut order = Vec::new();
        texts(&tree, &words, &mut order);
        assert!(order[0].starts_with("A title"));
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn single_column_text_has_no_column_cut() {
        let mut raw = Vec::new();
        column(&mut raw, "P", 40.0, 100.0, 30);
        let page = page(raw);
        let words = words_of(&page);
        let tree = segment(&words, (0..words.len()).collect(), 6.0);
        assert!(!has_columns(&tree));
    }
}
