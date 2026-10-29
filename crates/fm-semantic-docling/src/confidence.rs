//! How far a page's text-layer reconstruction can be trusted.
//!
//! The whitespace segmentation says where it found boundaries; this module looks for what it did
//! not resolve. Rows that still hold a wide internal gap after segmentation were read straight
//! across what is probably a column boundary or a table, and column cuts that rest on narrow
//! corridors may be rivers in justified text. Pages with a low score are the ones worth a second
//! opinion from a layout model.

use crate::reflow::{PageWords, median_gap_height, page_words};
use crate::regions::{Region, Word, rows, segment, words_of};

/// A gap inside a row of this many line heights is a column boundary, not a word space.
const WIDE_GAP: f32 = 1.2;
/// A corridor narrower than this many line heights is a weak boundary.
const WEAK_CORRIDOR: f32 = 1.6;
/// Fewest words for a page to carry any text layer worth judging.
const MIN_WORDS: usize = 20;
/// Weight of weak column cuts relative to unresolved rows.
const WEAK_CUT_WEIGHT: f32 = 0.5;

/// What the segmentation of one page left unresolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageConfidence {
    /// `0.0` (not trustworthy, or no text layer) to `1.0` (every boundary resolved).
    pub score: f32,
    /// Text rows left in the page's regions.
    pub rows: usize,
    /// Rows that still hold a wide gap, i.e. were read across a boundary.
    pub unresolved_rows: usize,
    /// Column cuts in the tree.
    pub cuts: usize,
    /// Column cuts resting on a narrow corridor.
    pub weak_cuts: usize,
}

/// Confidence of the geometric reconstruction of each page of `pdf` (index 0 is page 1).
#[must_use]
pub fn page_confidence(pdf: &[u8]) -> Vec<PageConfidence> {
    page_words(pdf).iter().map(assess).collect()
}

/// Judges one page from its word boxes.
pub(crate) fn assess(page: &PageWords) -> PageConfidence {
    let words = words_of(page);
    if words.len() < MIN_WORDS {
        return PageConfidence {
            score: 0.0,
            rows: 0,
            unresolved_rows: 0,
            cuts: 0,
            weak_cuts: 0,
        };
    }
    let height = median_gap_height(&words);
    let root = segment(&words, (0..words.len()).collect(), height);
    let mut tally = Tally::default();
    walk(&root, &words, height, &mut tally);
    let unresolved = tally.unresolved as f32 / tally.rows.max(1) as f32;
    let weak = tally.weak as f32 / tally.cuts.max(1) as f32;
    PageConfidence {
        score: ((1.0 - unresolved) * (1.0 - WEAK_CUT_WEIGHT * weak)).clamp(0.0, 1.0),
        rows: tally.rows,
        unresolved_rows: tally.unresolved,
        cuts: tally.cuts,
        weak_cuts: tally.weak,
    }
}

#[derive(Default)]
struct Tally {
    rows: usize,
    unresolved: usize,
    cuts: usize,
    weak: usize,
}

fn walk(region: &Region, words: &[Word<'_>], height: f32, tally: &mut Tally) {
    match region {
        Region::Leaf(indices) => {
            for row in rows(words, indices, height) {
                tally.rows += 1;
                let wide = row
                    .windows(2)
                    .any(|pair| words[pair[1]].left - words[pair[0]].right >= WIDE_GAP * height);
                tally.unresolved += usize::from(wide);
            }
        }
        Region::Bands(children) => {
            for child in children {
                walk(child, words, height, tally);
            }
        }
        Region::Columns {
            corridors,
            children,
            ..
        } => {
            tally.cuts += 1;
            let narrowest = corridors
                .iter()
                .map(|&(start, end)| (end - start) as f32)
                .fold(f32::MAX, f32::min);
            tally.weak += usize::from(narrowest < WEAK_CORRIDOR * height);
            for child in children {
                walk(child, words, height, tally);
            }
        }
    }
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

    /// `lines` rows of three words from `left`, each word overlapping the next.
    fn column(words: &mut Vec<WordBox>, left: f32, lines: usize) {
        for line in 0..lines {
            for step in 0..3 {
                words.push(word(
                    left + step as f32 * 36.0,
                    100.0 + line as f32 * 12.0,
                    &format!("w{line}x{step}xxxxxxxx"),
                ));
            }
        }
    }

    #[test]
    fn a_cleanly_cut_two_column_page_is_trusted() {
        let mut words = Vec::new();
        column(&mut words, 40.0, 14);
        column(&mut words, 330.0, 14);
        let confidence = assess(&page(words));
        assert_eq!(confidence.cuts, 1);
        assert_eq!(confidence.unresolved_rows, 0);
        assert!(confidence.score > 0.95, "{confidence:?}");
    }

    #[test]
    fn rows_with_a_wide_gap_that_were_not_cut_lower_the_score() {
        // Two short blocks side by side too small to cut: rows read straight across the gap.
        let mut words = Vec::new();
        for line in 0..4 {
            for (left, text) in [(40.0, "alpha"), (330.0, "omega")] {
                for step in 0..3 {
                    words.push(word(
                        left + step as f32 * 24.0,
                        100.0 + line as f32 * 12.0,
                        text,
                    ));
                }
            }
        }
        let confidence = assess(&page(words));
        assert!(confidence.unresolved_rows > 0, "{confidence:?}");
        assert!(confidence.score < 0.5, "{confidence:?}");
    }

    #[test]
    fn a_page_without_text_layer_scores_zero() {
        assert!(assess(&page(Vec::new())).score.abs() < f32::EPSILON);
    }
}
