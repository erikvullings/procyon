//! Deterministic structural chunking.
//!
//! The chunker packs adjacent structural units up to a token target, never
//! exceeds a hard maximum, and only uses overlap when it has to split a single
//! oversized unit. It never packs across an incompatible top-level boundary
//! (a different page, slide, sheet or section) merely to reach the target,
//! because a chunk that spans two boundaries cannot be cited precisely.
//!
//! # Embedding input
//!
//! Embedding input is exactly *section hierarchy plus chunk content*. File
//! name, absolute or relative path, generated document brief, description and
//! SKOS labels are structurally unable to reach it: none of them exist in
//! [`DocumentMetadata`](crate::DocumentMetadata) or in a
//! [`StructuralUnit`](crate::StructuralUnit). Renaming or moving a file
//! therefore cannot change a fingerprint, which is what makes vector reuse
//! safe.
//!
//! # Fingerprints
//!
//! The fingerprint is BLAKE3 over a domain-separated encoding of the
//! embedding input plus the converter version, the chunker version and the
//! token-estimator version. A local edit changes only the chunks that contain
//! the edited unit; every other chunk keeps its fingerprint and its vector.

use crate::model::{ComponentVersion, ConvertedDocument, FormatKind, Provenance, StructuralUnit};
use crate::tokens::{TOKEN_ESTIMATOR_VERSION, estimate_tokens};

/// Version of the packing rules implemented here.
const CHUNKER_VERSION: ComponentVersion = ComponentVersion::new("structural", 2);

/// Domain separator, so a fingerprint cannot collide with any other BLAKE3 use
/// in the workspace.
const FINGERPRINT_DOMAIN: &[u8] = b"procyon.semantic.chunk.v1";

/// Tuning for [`Chunker`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkerOptions {
    /// Token count a chunk aims for.
    pub target_tokens: u32,
    /// Token count a chunk may never exceed.
    pub max_tokens: u32,
    /// Tokens of context repeated when an oversized unit is split.
    pub overlap_tokens: u32,
    /// Maximum characters of the display excerpt.
    pub max_excerpt_chars: usize,
}

impl Default for ChunkerOptions {
    fn default() -> Self {
        Self {
            target_tokens: 400,
            max_tokens: 512,
            overlap_tokens: 48,
            max_excerpt_chars: 400,
        }
    }
}

/// Why a [`ChunkerOptions`] value cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidChunkerOptions {
    /// The target must be positive and at most the maximum.
    #[error("the token target ({target}) must be between 1 and the maximum ({max})")]
    Target {
        /// Configured target.
        target: u32,
        /// Configured maximum.
        max: u32,
    },
    /// Overlap must leave room for progress when splitting.
    #[error("the overlap ({overlap}) must be less than half the maximum ({max})")]
    Overlap {
        /// Configured overlap.
        overlap: u32,
        /// Configured maximum.
        max: u32,
    },
}

/// Where a chunk's content came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkProvenance {
    /// The chunk covers exactly one unit's provenance.
    Exact(Provenance),
    /// The chunk covers several units; the first and last are recorded rather
    /// than a merged range that the formats cannot all express.
    Span {
        /// Provenance of the first unit in the chunk.
        first: Provenance,
        /// Provenance of the last unit in the chunk.
        last: Provenance,
    },
}

/// Position of a chunk inside a split oversized unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkPart {
    /// 0-based part index.
    pub index: u32,
    /// Number of parts the unit was split into.
    pub total: u32,
    /// Tokens of overlap repeated from the previous part.
    pub overlap_tokens: u32,
}

/// One embeddable chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// 0-based position of the chunk in the document.
    pub index: u32,
    /// Order of the first structural unit the chunk covers.
    pub source_order: u32,
    /// Orders of every unit the chunk covers.
    pub unit_orders: Vec<u32>,
    /// Format the chunk came from.
    pub format: FormatKind,
    /// Section hierarchy of the chunk's first unit.
    pub section_path: Vec<String>,
    /// Whether the section hierarchy had to be shortened to leave room for
    /// content within the hard token maximum.
    pub section_path_truncated: bool,
    /// Exactly what is embedded: section hierarchy plus content.
    pub embedding_input: String,
    /// Bounded text for display in results.
    pub display_excerpt: String,
    /// Best available provenance for citation.
    pub provenance: ChunkProvenance,
    /// Part information when an oversized unit had to be split.
    pub part: Option<ChunkPart>,
    /// Conservative token estimate of the embedding input.
    pub estimated_tokens: u32,
    /// Stable content fingerprint; see the module documentation.
    pub fingerprint: String,
}

/// Deterministic structural chunker.
#[derive(Debug, Clone, Default)]
pub struct Chunker {
    options: ChunkerOptions,
}

impl Chunker {
    /// Creates a chunker, validating the options.
    pub fn new(options: ChunkerOptions) -> Result<Self, InvalidChunkerOptions> {
        if options.target_tokens == 0 || options.target_tokens > options.max_tokens {
            return Err(InvalidChunkerOptions::Target {
                target: options.target_tokens,
                max: options.max_tokens,
            });
        }
        if options.overlap_tokens * 2 >= options.max_tokens {
            return Err(InvalidChunkerOptions::Overlap {
                overlap: options.overlap_tokens,
                max: options.max_tokens,
            });
        }
        Ok(Self { options })
    }

    /// The version of the packing rules, which participates in fingerprints.
    #[must_use]
    pub fn version(&self) -> ComponentVersion {
        CHUNKER_VERSION
    }

    /// The options in force.
    #[must_use]
    pub fn options(&self) -> &ChunkerOptions {
        &self.options
    }

    /// Chunks a converted document.
    #[must_use]
    pub fn chunk(&self, document: &ConvertedDocument) -> Vec<Chunk> {
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut pending: Vec<&StructuralUnit> = Vec::new();

        for unit in document.units() {
            let (section_path, _) =
                bounded_section_path(&unit.section_path, self.options.max_tokens);
            let unit_tokens = estimate_tokens(&embedding_input(&section_path, &unit.text));
            if unit_tokens > self.options.max_tokens {
                self.flush(document, &mut pending, &mut chunks);
                self.split_unit(document, unit, &mut chunks);
                continue;
            }
            let compatible = pending
                .last()
                .is_none_or(|previous| previous.boundary == unit.boundary);
            let exceeds_target = !pending.is_empty() && {
                let (path, _) =
                    bounded_section_path(&pending[0].section_path, self.options.max_tokens);
                let mut content = pending
                    .iter()
                    .map(|pending_unit| pending_unit.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                content.push_str("\n\n");
                content.push_str(&unit.text);
                estimate_tokens(&embedding_input(&path, &content)) > self.options.target_tokens
            };
            if !compatible || exceeds_target {
                self.flush(document, &mut pending, &mut chunks);
            }
            pending.push(unit);
        }
        self.flush(document, &mut pending, &mut chunks);
        chunks
    }

    fn flush(
        &self,
        document: &ConvertedDocument,
        pending: &mut Vec<&StructuralUnit>,
        chunks: &mut Vec<Chunk>,
    ) {
        if pending.is_empty() {
            return;
        }
        let units = std::mem::take(pending);
        let content = units
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let first = units.first().unwrap_or_else(|| unreachable!());
        let last = units.last().unwrap_or_else(|| unreachable!());
        let provenance = if units.len() == 1 {
            ChunkProvenance::Exact(first.provenance.clone())
        } else {
            ChunkProvenance::Span {
                first: first.provenance.clone(),
                last: last.provenance.clone(),
            }
        };
        let (section_path, section_path_truncated) =
            bounded_section_path(&first.section_path, self.options.max_tokens);
        let chunk = self.build(
            document,
            chunks.len() as u32,
            first.order,
            units.iter().map(|unit| unit.order).collect(),
            section_path,
            section_path_truncated,
            &content,
            provenance,
            None,
        );
        chunks.push(chunk);
    }

    fn split_unit(
        &self,
        document: &ConvertedDocument,
        unit: &StructuralUnit,
        chunks: &mut Vec<Chunk>,
    ) {
        let (section_path, section_path_truncated) =
            bounded_section_path(&unit.section_path, self.options.max_tokens);
        let prefix_tokens = estimate_tokens(&embedding_input(&section_path, ""));
        let content_budget = self.options.max_tokens.saturating_sub(prefix_tokens).max(1);
        let windows = self.split_text(&unit.text, content_budget);
        let total = windows.len() as u32;
        for (index, (text, overlap_tokens)) in windows.into_iter().enumerate() {
            let chunk = self.build(
                document,
                chunks.len() as u32,
                unit.order,
                vec![unit.order],
                section_path.clone(),
                section_path_truncated,
                &text,
                ChunkProvenance::Exact(unit.provenance.clone()),
                Some(ChunkPart {
                    index: index as u32,
                    total,
                    overlap_tokens,
                }),
            );
            chunks.push(chunk);
        }
    }

    /// Splits oversized text into windows within `content_budget`, repeating
    /// limited context from the previous window.
    fn split_text(&self, text: &str, content_budget: u32) -> Vec<(String, u32)> {
        let pieces: Vec<&str> = text.split_inclusive(char::is_whitespace).collect();
        let mut windows: Vec<(String, u32)> = Vec::new();
        let mut index = 0_usize;
        let mut overlap: Vec<&str> = Vec::new();
        let overlap_budget = self
            .options
            .overlap_tokens
            .min(content_budget.saturating_sub(1));
        while index < pieces.len() {
            let mut window: Vec<&str> = overlap.clone();
            let overlap_tokens = estimate_tokens(&window.concat());
            let mut used = 0_usize;
            while index + used < pieces.len() {
                let candidate = pieces[index + used];
                let mut trial = window.clone();
                trial.push(candidate);
                if estimate_tokens(&trial.concat()) > content_budget {
                    break;
                }
                window.push(candidate);
                used += 1;
            }
            if used == 0 {
                // A single piece is larger than the maximum on its own: split
                // it by characters so progress is always made.
                let piece = pieces[index];
                let budget = content_budget.saturating_sub(overlap_tokens).max(1);
                let taken = prefix_within_token_budget(piece, budget);
                let remainder = piece[taken.len()..].to_owned();
                let mut window_text = window.concat();
                window_text.push_str(&taken);
                windows.push((window_text, overlap_tokens));
                if remainder.is_empty() {
                    index += 1;
                    overlap = Vec::new();
                    continue;
                }
                // Re-enter the loop with the remainder standing in for the
                // rest of the oversized piece.
                let mut tail = remainder;
                tail.push_str(&pieces[index + 1..].concat());
                windows.extend(split_without_overlap(&tail, content_budget));
                return windows;
            }
            windows.push((window.concat(), overlap_tokens));
            index += used;
            overlap = self.overlap_pieces(&pieces[..index], overlap_budget);
            if index >= pieces.len() {
                break;
            }
        }
        if windows.is_empty() {
            windows.push((text.to_owned(), 0));
        }
        windows
    }

    /// Trailing pieces of `consumed` whose combined estimate stays within the
    /// overlap budget.
    fn overlap_pieces<'a>(&self, consumed: &[&'a str], budget: u32) -> Vec<&'a str> {
        let mut overlap: Vec<&str> = Vec::new();
        for piece in consumed.iter().rev() {
            let mut trial = vec![*piece];
            trial.extend(overlap.iter().copied());
            if estimate_tokens(&trial.concat()) > budget {
                break;
            }
            overlap = trial;
        }
        overlap
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        &self,
        document: &ConvertedDocument,
        index: u32,
        source_order: u32,
        unit_orders: Vec<u32>,
        section_path: Vec<String>,
        section_path_truncated: bool,
        content: &str,
        provenance: ChunkProvenance,
        part: Option<ChunkPart>,
    ) -> Chunk {
        let embedding_input = embedding_input(&section_path, content);
        debug_assert!(estimate_tokens(&embedding_input) <= self.options.max_tokens);
        let fingerprint = fingerprint(document.converter(), self.version(), &embedding_input);
        let display_excerpt = excerpt(content, self.options.max_excerpt_chars);
        Chunk {
            index,
            source_order,
            unit_orders,
            format: document.format(),
            section_path,
            section_path_truncated,
            estimated_tokens: estimate_tokens(&embedding_input),
            embedding_input,
            display_excerpt,
            provenance,
            part,
            fingerprint,
        }
    }
}

fn split_without_overlap(text: &str, max_tokens: u32) -> Vec<(String, u32)> {
    let mut windows = Vec::new();
    let mut pending = text;
    while !pending.is_empty() {
        let taken = prefix_within_token_budget(pending, max_tokens);
        pending = &pending[taken.len()..];
        windows.push((taken, 0));
    }
    windows
}

fn prefix_within_token_budget(text: &str, max_tokens: u32) -> String {
    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let mut low = 1_usize;
    let mut high = boundaries.len();
    while low < high {
        let middle = (low + high).div_ceil(2);
        if estimate_tokens(&text[..boundaries[middle - 1]]) <= max_tokens {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    text[..boundaries[low.saturating_sub(1)]].to_owned()
}

fn bounded_section_path(section_path: &[String], max_tokens: u32) -> (Vec<String>, bool) {
    if section_path.is_empty() || max_tokens <= 3 {
        return (Vec::new(), !section_path.is_empty());
    }
    let joined = section_path.join(" > ");
    let max_path_tokens = max_tokens - 3;
    if estimate_tokens(&joined) <= max_path_tokens {
        return (section_path.to_vec(), false);
    }
    (
        vec![prefix_within_token_budget(&joined, max_path_tokens)],
        true,
    )
}

/// Builds embedding input: section hierarchy, then content. Nothing else.
fn embedding_input(section_path: &[String], content: &str) -> String {
    if section_path.is_empty() {
        return content.to_owned();
    }
    format!("{}\n\n{content}", section_path.join(" > "))
}

fn excerpt(content: &str, limit: usize) -> String {
    if content.chars().count() <= limit {
        return content.to_owned();
    }
    let mut excerpt: String = content.chars().take(limit).collect();
    excerpt.push('…');
    excerpt
}

fn fingerprint(
    converter: ComponentVersion,
    chunker: ComponentVersion,
    embedding_input: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(FINGERPRINT_DOMAIN);
    hasher.update(b"\0");
    hasher.update(converter.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(chunker.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(TOKEN_ESTIMATOR_VERSION.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(embedding_input.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{TopLevelBoundary, UnitKind};
    use crate::text::SourceMap;

    fn unit(order: u32, text: &str, boundary: TopLevelBoundary, path: &[&str]) -> StructuralUnit {
        StructuralUnit {
            order,
            kind: UnitKind::Paragraph,
            format: FormatKind::PlainText,
            section_path: path.iter().map(|value| (*value).to_owned()).collect(),
            text: text.to_owned(),
            provenance: Provenance::TextLines {
                start_line: order + 1,
                end_line: order + 1,
            },
            boundary,
            source_map: SourceMap::default(),
            truncated: false,
        }
    }

    fn document(units: Vec<StructuralUnit>) -> ConvertedDocument {
        ConvertedDocument::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::PlainText,
            units,
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn options_are_validated() {
        assert!(matches!(
            Chunker::new(ChunkerOptions {
                target_tokens: 600,
                max_tokens: 512,
                ..ChunkerOptions::default()
            }),
            Err(InvalidChunkerOptions::Target { .. })
        ));
        assert!(matches!(
            Chunker::new(ChunkerOptions {
                target_tokens: 10,
                max_tokens: 20,
                overlap_tokens: 10,
                ..ChunkerOptions::default()
            }),
            Err(InvalidChunkerOptions::Overlap { .. })
        ));
    }

    #[test]
    fn adjacent_compatible_units_are_packed_to_the_target() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 20,
            max_tokens: 40,
            overlap_tokens: 4,
            max_excerpt_chars: 400,
        })
        .expect("chunker");
        let document = document(vec![
            unit(0, "alpha beta gamma", TopLevelBoundary::Document, &[]),
            unit(1, "delta epsilon", TopLevelBoundary::Document, &[]),
        ]);
        let chunks = chunker.chunk(&document);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].unit_orders, [0, 1]);
        assert_eq!(
            chunks[0].embedding_input,
            "alpha beta gamma\n\ndelta epsilon"
        );
        assert!(matches!(chunks[0].provenance, ChunkProvenance::Span { .. }));
    }

    #[test]
    fn incompatible_boundaries_are_never_merged_to_fill_a_target() {
        let chunker = Chunker::default();
        let document = document(vec![
            unit(0, "slide one text", TopLevelBoundary::Slide(1), &[]),
            unit(1, "slide two text", TopLevelBoundary::Slide(2), &[]),
        ]);
        let chunks = chunker.chunk(&document);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].unit_orders, [0]);
        assert_eq!(chunks[1].unit_orders, [1]);
    }

    #[test]
    fn an_oversized_unit_is_split_with_overlap_and_never_exceeds_the_maximum() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 12,
            max_tokens: 16,
            overlap_tokens: 4,
            max_excerpt_chars: 400,
        })
        .expect("chunker");
        let text = (0..40)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let document = document(vec![unit(0, &text, TopLevelBoundary::Document, &[])]);
        let chunks = chunker.chunk(&document);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                estimate_tokens(&chunk.embedding_input) <= 16,
                "chunk exceeded the maximum: {}",
                chunk.estimated_tokens
            );
            let part = chunk.part.expect("split chunks record their part");
            assert_eq!(part.total as usize, chunks.len());
        }
        assert_eq!(chunks[0].part.expect("part").overlap_tokens, 0);
        assert!(chunks[1].part.expect("part").overlap_tokens > 0);
        let second_start = chunks[1]
            .embedding_input
            .split_whitespace()
            .next()
            .expect("word");
        assert!(chunks[0].embedding_input.contains(second_start));
    }

    #[test]
    fn section_hierarchy_counts_toward_the_hard_maximum() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 8,
            max_tokens: 12,
            overlap_tokens: 2,
            max_excerpt_chars: 400,
        })
        .expect("chunker");
        let document = document(vec![unit(
            0,
            "alpha beta gamma delta epsilon zeta eta theta",
            TopLevelBoundary::Section("Architecture".to_owned()),
            &["Architecture", "Semantic Search"],
        )]);

        let chunks = chunker.chunk(&document);

        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.estimated_tokens <= chunker.options().max_tokens)
        );
    }

    #[test]
    fn an_oversized_section_hierarchy_is_bounded_deterministically() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 4,
            max_tokens: 6,
            overlap_tokens: 1,
            max_excerpt_chars: 400,
        })
        .expect("chunker");
        let document = document(vec![unit(
            0,
            "body",
            TopLevelBoundary::Section("Extremely long section".to_owned()),
            &[
                "Extremely long section title",
                "Another very long nested section title",
            ],
        )]);

        let first = chunker.chunk(&document);
        let second = chunker.chunk(&document);

        assert_eq!(first, second);
        assert!(!first[0].section_path.is_empty());
        assert!(first[0].section_path_truncated);
        assert!(
            first
                .iter()
                .all(|chunk| chunk.estimated_tokens <= chunker.options().max_tokens)
        );
    }

    #[test]
    fn packed_unit_separators_count_toward_the_hard_maximum() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 3,
            max_tokens: 3,
            overlap_tokens: 0,
            max_excerpt_chars: 400,
        })
        .expect("chunker");
        let document = document(vec![
            unit(0, "aaa", TopLevelBoundary::Document, &[]),
            unit(1, "bbb", TopLevelBoundary::Document, &[]),
        ]);

        let chunks = chunker.chunk(&document);

        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|chunk| chunk.estimated_tokens <= 3));
    }

    #[test]
    fn a_single_gigantic_token_is_split_by_characters() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 8,
            max_tokens: 10,
            overlap_tokens: 2,
            max_excerpt_chars: 64,
        })
        .expect("chunker");
        let text = "x".repeat(500);
        let document = document(vec![unit(0, &text, TopLevelBoundary::Document, &[])]);
        let chunks = chunker.chunk(&document);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(estimate_tokens(&chunk.embedding_input) <= 10);
        }
        let rebuilt: String = chunks
            .iter()
            .map(|chunk| chunk.embedding_input.clone())
            .collect();
        assert_eq!(rebuilt, text);
    }

    #[test]
    fn text_after_a_gigantic_token_is_also_split_to_the_maximum() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 8,
            max_tokens: 10,
            overlap_tokens: 2,
            max_excerpt_chars: 400,
        })
        .expect("chunker");
        let text = format!(
            "{} {}",
            "x".repeat(100),
            (0..40)
                .map(|index| format!("tail{index}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let document = document(vec![unit(0, &text, TopLevelBoundary::Document, &[])]);

        let chunks = chunker.chunk(&document);

        assert!(chunks.iter().all(|chunk| chunk.estimated_tokens <= 10));
        let rebuilt: String = chunks
            .iter()
            .map(|chunk| chunk.embedding_input.clone())
            .collect();
        assert_eq!(rebuilt, text);
    }

    #[test]
    fn embedding_input_is_the_section_hierarchy_plus_content_only() {
        let chunker = Chunker::default();
        let document = document(vec![unit(
            0,
            "body text",
            TopLevelBoundary::Section("Chapter".to_owned()),
            &["Chapter", "Section"],
        )]);
        let chunks = chunker.chunk(&document);
        assert_eq!(chunks[0].embedding_input, "Chapter > Section\n\nbody text");
    }

    #[test]
    fn a_local_edit_only_changes_the_fingerprints_of_affected_chunks() {
        let chunker = Chunker::new(ChunkerOptions {
            target_tokens: 8,
            max_tokens: 16,
            overlap_tokens: 2,
            max_excerpt_chars: 400,
        })
        .expect("chunker");
        let before = document(vec![
            unit(0, "first paragraph body", TopLevelBoundary::Document, &[]),
            unit(1, "second paragraph body", TopLevelBoundary::Document, &[]),
            unit(2, "third paragraph body", TopLevelBoundary::Document, &[]),
        ]);
        let after = document(vec![
            unit(0, "first paragraph body", TopLevelBoundary::Document, &[]),
            unit(
                1,
                "second paragraph EDITED",
                TopLevelBoundary::Document,
                &[],
            ),
            unit(2, "third paragraph body", TopLevelBoundary::Document, &[]),
        ]);
        let old_chunks = chunker.chunk(&before);
        let new_chunks = chunker.chunk(&after);
        assert_eq!(old_chunks.len(), new_chunks.len());
        assert_eq!(old_chunks[0].fingerprint, new_chunks[0].fingerprint);
        assert_ne!(old_chunks[1].fingerprint, new_chunks[1].fingerprint);
        assert_eq!(old_chunks[2].fingerprint, new_chunks[2].fingerprint);
    }

    #[test]
    fn fingerprints_are_stable_and_versioned() {
        let chunker = Chunker::default();
        let document = document(vec![unit(0, "content", TopLevelBoundary::Document, &[])]);
        let first = chunker.chunk(&document);
        let second = chunker.chunk(&document);
        assert_eq!(first, second);

        let other_converter = ConvertedDocument::new(
            ComponentVersion::new("baseline", 2),
            FormatKind::PlainText,
            vec![unit(0, "content", TopLevelBoundary::Document, &[])],
            Vec::new(),
            Vec::new(),
        );
        let bumped = chunker.chunk(&other_converter);
        assert_ne!(first[0].fingerprint, bumped[0].fingerprint);
    }

    #[test]
    fn the_display_excerpt_is_bounded_but_the_embedding_input_is_not_truncated() {
        let chunker = Chunker::new(ChunkerOptions {
            max_excerpt_chars: 10,
            ..ChunkerOptions::default()
        })
        .expect("chunker");
        let document = document(vec![unit(
            0,
            "0123456789abcdef",
            TopLevelBoundary::Document,
            &[],
        )]);
        let chunks = chunker.chunk(&document);
        assert_eq!(chunks[0].display_excerpt, "0123456789…");
        assert_eq!(chunks[0].embedding_input, "0123456789abcdef");
    }
}
