//! Deterministic representative-chunk selection for document summaries.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const ALGORITHM_VERSION: &str = "representative-kmeans/1";
const DEFAULT_MIN_CLUSTERS: usize = 2;
const DEFAULT_MAX_CLUSTERS: usize = 12;
const DEFAULT_MAX_ITERATIONS: usize = 32;

/// Structural role used to preserve document openings and conclusions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarySectionRole {
    /// Identified document title.
    Title,
    /// Identified introduction or opening section.
    Introduction,
    /// Ordinary body content.
    Body,
    /// Identified conclusion or closing section.
    Conclusion,
}

/// One extracted, already-embedded source chunk eligible for selection.
#[derive(Debug, Clone, PartialEq)]
pub struct SummarySourceChunk {
    /// Stable source chunk identity.
    pub chunk_id: String,
    /// Complete structurally bounded chunk text.
    pub text: String,
    /// Existing local embedding; selection never embeds again.
    pub embedding: Vec<f32>,
    /// Token count from the active local tokenizer.
    pub token_count: usize,
    /// Original document order.
    pub source_position: u32,
    /// Bounded section hierarchy.
    pub section_path: Vec<String>,
    /// Serialized strongest available source provenance.
    pub provenance: String,
    /// Structural role inferred by conversion.
    pub role: SummarySectionRole,
    /// Generated records are never recursively selected.
    pub generated: bool,
}

/// Resource and input budget for deterministic selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepresentativeSelectionConfig {
    /// Maximum total tokens passed to generation.
    pub input_token_budget: usize,
    /// Lower bound for large-document cluster count.
    pub minimum_clusters: usize,
    /// Upper bound for large-document cluster count.
    pub maximum_clusters: usize,
    /// Hard convergence bound.
    pub maximum_iterations: usize,
}

impl RepresentativeSelectionConfig {
    /// Creates conservative bounded defaults for one input budget.
    #[must_use]
    pub const fn for_budget(input_token_budget: usize) -> Self {
        Self {
            input_token_budget,
            minimum_clusters: DEFAULT_MIN_CLUSTERS,
            maximum_clusters: DEFAULT_MAX_CLUSTERS,
            maximum_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }
}

/// Selection strategy used for this document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepresentativeSelectionMode {
    /// Every extracted chunk fit and was retained.
    AllChunks,
    /// Deterministic k-means selected real nearest-to-centroid chunks.
    ClusterMedoids,
}

/// One real source chunk selected for generation.
#[derive(Debug, Clone, PartialEq)]
pub struct RepresentativeChunk {
    /// Source chunk without clipping or synthesis.
    pub source: SummarySourceChunk,
    /// Assigned cluster population.
    pub cluster_population: usize,
    /// Cluster population divided by all clustered chunks.
    pub cluster_weight: f32,
    /// Squared Euclidean distance from the cluster centroid.
    pub centroid_distance: f32,
    /// Whether structural coverage forced retention.
    pub structural_anchor: bool,
}

/// Stable selection result and reuse fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct RepresentativeSelection {
    /// Algorithm branch used.
    pub mode: RepresentativeSelectionMode,
    /// Selected source chunks in original source order.
    pub representatives: Vec<RepresentativeChunk>,
    /// Exact selected token count.
    pub selected_tokens: usize,
    /// Stable fingerprint for regeneration reuse.
    pub fingerprint: String,
    /// Versioned algorithm identity.
    pub algorithm_version: &'static str,
}

/// Typed representative-selection failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepresentativeSelectionError {
    /// No extracted source chunks were supplied.
    #[error("no source chunks were supplied")]
    EmptyInput,
    /// Budget or clustering limits are invalid.
    #[error("representative selection configuration is invalid")]
    InvalidConfiguration,
    /// Existing embeddings are missing, inconsistent, or non-finite.
    #[error("source embeddings are invalid")]
    InvalidEmbeddings,
    /// Mandatory structural anchors cannot fit without clipping.
    #[error("summary input budget cannot fit structural coverage")]
    StructuralCoverageExceedsBudget,
    /// Selection was cancelled.
    #[error("representative selection was cancelled")]
    Cancelled,
}

/// Selects deterministic, real source chunks without embedding or clipping.
pub fn select_representative_chunks(
    document_generation: u64,
    content_hash: &str,
    embedding_model_id: &str,
    chunks: &[SummarySourceChunk],
    config: RepresentativeSelectionConfig,
    cancellation: &CancellationToken,
) -> Result<RepresentativeSelection, RepresentativeSelectionError> {
    if cancellation.is_cancelled() {
        return Err(RepresentativeSelectionError::Cancelled);
    }
    if config.input_token_budget == 0
        || config.minimum_clusters == 0
        || config.minimum_clusters > config.maximum_clusters
        || config.maximum_iterations == 0
        || config.maximum_iterations > 100
    {
        return Err(RepresentativeSelectionError::InvalidConfiguration);
    }
    let mut eligible = chunks
        .iter()
        .filter(|chunk| !chunk.generated)
        .cloned()
        .collect::<Vec<_>>();
    eligible.sort_by(|left, right| {
        left.source_position
            .cmp(&right.source_position)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    if eligible.is_empty() {
        return Err(RepresentativeSelectionError::EmptyInput);
    }
    let dimensions = eligible[0].embedding.len();
    if dimensions == 0
        || eligible.iter().any(|chunk| {
            chunk.embedding.len() != dimensions
                || chunk.embedding.iter().any(|value| !value.is_finite())
                || chunk.token_count == 0
        })
    {
        return Err(RepresentativeSelectionError::InvalidEmbeddings);
    }

    let fingerprint = representative_selection_fingerprint(
        document_generation,
        content_hash,
        embedding_model_id,
        &eligible,
        config,
    );
    let total_tokens = eligible
        .iter()
        .map(|chunk| chunk.token_count)
        .sum::<usize>();
    if total_tokens <= config.input_token_budget {
        let population = eligible.len();
        return Ok(RepresentativeSelection {
            mode: RepresentativeSelectionMode::AllChunks,
            representatives: eligible
                .into_iter()
                .map(|source| RepresentativeChunk {
                    structural_anchor: source.role != SummarySectionRole::Body,
                    source,
                    cluster_population: 1,
                    cluster_weight: 1.0 / population as f32,
                    centroid_distance: 0.0,
                })
                .collect(),
            selected_tokens: total_tokens,
            fingerprint,
            algorithm_version: ALGORITHM_VERSION,
        });
    }

    let average_tokens = total_tokens.div_ceil(eligible.len()).max(1);
    let cluster_count = (config.input_token_budget / average_tokens)
        .clamp(config.minimum_clusters, config.maximum_clusters)
        .min(eligible.len());
    let seed = deterministic_seed(&fingerprint, eligible.len());
    let mut centroids = initialize_centroids(&eligible, cluster_count, seed);
    let mut assignments = vec![usize::MAX; eligible.len()];
    for _ in 0..config.maximum_iterations {
        if cancellation.is_cancelled() {
            return Err(RepresentativeSelectionError::Cancelled);
        }
        let next = assign_chunks(&eligible, &centroids);
        let converged = next == assignments;
        assignments = next;
        if converged {
            break;
        }
        recompute_centroids(&eligible, &assignments, &mut centroids);
    }

    let populations = (0..cluster_count)
        .map(|cluster| {
            assignments
                .iter()
                .filter(|assignment| **assignment == cluster)
                .count()
        })
        .collect::<Vec<_>>();
    let mut selected = BTreeMap::<usize, RepresentativeChunk>::new();
    for cluster in 0..cluster_count {
        if populations[cluster] == 0 {
            continue;
        }
        let index = assignments
            .iter()
            .enumerate()
            .filter(|(_, assignment)| **assignment == cluster)
            .min_by(|(left, _), (right, _)| {
                squared_distance(&eligible[*left].embedding, &centroids[cluster])
                    .total_cmp(&squared_distance(
                        &eligible[*right].embedding,
                        &centroids[cluster],
                    ))
                    .then_with(|| {
                        eligible[*left]
                            .source_position
                            .cmp(&eligible[*right].source_position)
                    })
                    .then_with(|| eligible[*left].chunk_id.cmp(&eligible[*right].chunk_id))
            })
            .map(|(index, _)| index)
            .expect("non-empty cluster has a representative");
        selected.insert(
            index,
            representative(
                &eligible,
                &assignments,
                &centroids,
                &populations,
                index,
                false,
            ),
        );
    }

    let anchors = structural_anchor_indices(&eligible);
    let anchor_tokens = anchors
        .iter()
        .map(|index| eligible[*index].token_count)
        .sum::<usize>();
    if anchor_tokens > config.input_token_budget {
        return Err(RepresentativeSelectionError::StructuralCoverageExceedsBudget);
    }
    for index in &anchors {
        selected.insert(
            *index,
            representative(
                &eligible,
                &assignments,
                &centroids,
                &populations,
                *index,
                true,
            ),
        );
    }

    let mut packed = anchors
        .iter()
        .filter_map(|index| selected.remove(index))
        .collect::<Vec<_>>();
    let mut selected_tokens = anchor_tokens;
    let mut remaining = selected.into_values().collect::<Vec<_>>();
    remaining.sort_by(|left, right| {
        right
            .cluster_weight
            .total_cmp(&left.cluster_weight)
            .then_with(|| left.centroid_distance.total_cmp(&right.centroid_distance))
            .then_with(|| {
                left.source
                    .source_position
                    .cmp(&right.source.source_position)
            })
            .then_with(|| left.source.chunk_id.cmp(&right.source.chunk_id))
    });
    for representative in remaining {
        if selected_tokens.saturating_add(representative.source.token_count)
            <= config.input_token_budget
        {
            selected_tokens += representative.source.token_count;
            packed.push(representative);
        }
    }
    packed.sort_by(|left, right| {
        left.source
            .source_position
            .cmp(&right.source.source_position)
            .then_with(|| left.source.chunk_id.cmp(&right.source.chunk_id))
    });

    Ok(RepresentativeSelection {
        mode: RepresentativeSelectionMode::ClusterMedoids,
        representatives: packed,
        selected_tokens,
        fingerprint,
        algorithm_version: ALGORITHM_VERSION,
    })
}

/// Computes the stable reuse key without running clustering.
#[must_use]
pub fn representative_selection_fingerprint(
    document_generation: u64,
    content_hash: &str,
    embedding_model_id: &str,
    chunks: &[SummarySourceChunk],
    config: RepresentativeSelectionConfig,
) -> String {
    let mut chunks = chunks
        .iter()
        .filter(|chunk| !chunk.generated)
        .collect::<Vec<_>>();
    chunks.sort_by(|left, right| {
        left.source_position
            .cmp(&right.source_position)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    let mut digest = Sha256::new();
    digest.update(ALGORITHM_VERSION.as_bytes());
    digest.update(document_generation.to_le_bytes());
    digest.update(content_hash.as_bytes());
    digest.update([0]);
    digest.update(embedding_model_id.as_bytes());
    digest.update(config.input_token_budget.to_le_bytes());
    digest.update(config.minimum_clusters.to_le_bytes());
    digest.update(config.maximum_clusters.to_le_bytes());
    digest.update(config.maximum_iterations.to_le_bytes());
    for chunk in chunks {
        digest.update(chunk.chunk_id.as_bytes());
        digest.update([0]);
        digest.update(chunk.source_position.to_le_bytes());
        digest.update(chunk.token_count.to_le_bytes());
        for value in &chunk.embedding {
            digest.update(value.to_le_bytes());
        }
    }
    let mut fingerprint = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to a string cannot fail");
    }
    fingerprint
}

fn deterministic_seed(fingerprint: &str, count: usize) -> usize {
    fingerprint.bytes().take(16).fold(0_usize, |value, byte| {
        value.wrapping_mul(31).wrapping_add(usize::from(byte))
    }) % count
}

fn initialize_centroids(chunks: &[SummarySourceChunk], count: usize, seed: usize) -> Vec<Vec<f32>> {
    let mut selected = vec![seed];
    while selected.len() < count {
        let candidate = (0..chunks.len())
            .filter(|index| !selected.contains(index))
            .max_by(|left, right| {
                let left_distance = selected
                    .iter()
                    .map(|selected| {
                        squared_distance(&chunks[*left].embedding, &chunks[*selected].embedding)
                    })
                    .min_by(f32::total_cmp)
                    .unwrap_or_default();
                let right_distance = selected
                    .iter()
                    .map(|selected| {
                        squared_distance(&chunks[*right].embedding, &chunks[*selected].embedding)
                    })
                    .min_by(f32::total_cmp)
                    .unwrap_or_default();
                left_distance
                    .total_cmp(&right_distance)
                    .then_with(|| {
                        chunks[*right]
                            .source_position
                            .cmp(&chunks[*left].source_position)
                    })
                    .then_with(|| chunks[*right].chunk_id.cmp(&chunks[*left].chunk_id))
            })
            .expect("cluster count never exceeds source count");
        selected.push(candidate);
    }
    selected
        .into_iter()
        .map(|index| chunks[index].embedding.clone())
        .collect()
}

fn assign_chunks(chunks: &[SummarySourceChunk], centroids: &[Vec<f32>]) -> Vec<usize> {
    chunks
        .iter()
        .map(|chunk| {
            centroids
                .iter()
                .enumerate()
                .min_by(|(left_index, left), (right_index, right)| {
                    squared_distance(&chunk.embedding, left)
                        .total_cmp(&squared_distance(&chunk.embedding, right))
                        .then_with(|| left_index.cmp(right_index))
                })
                .map(|(index, _)| index)
                .expect("selection always has at least one centroid")
        })
        .collect()
}

fn recompute_centroids(
    chunks: &[SummarySourceChunk],
    assignments: &[usize],
    centroids: &mut [Vec<f32>],
) {
    for (cluster, centroid) in centroids.iter_mut().enumerate() {
        let members = assignments
            .iter()
            .enumerate()
            .filter_map(|(index, assignment)| (*assignment == cluster).then_some(index))
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        for (dimension, value) in centroid.iter_mut().enumerate() {
            *value = members
                .iter()
                .map(|index| chunks[*index].embedding[dimension])
                .sum::<f32>()
                / members.len() as f32;
        }
    }
}

fn structural_anchor_indices(chunks: &[SummarySourceChunk]) -> BTreeSet<usize> {
    let mut anchors = BTreeSet::new();
    if let Some(index) = chunks
        .iter()
        .position(|chunk| chunk.role == SummarySectionRole::Title)
    {
        anchors.insert(index);
    }
    if let Some(index) = chunks
        .iter()
        .position(|chunk| chunk.role == SummarySectionRole::Introduction)
    {
        anchors.insert(index);
    }
    if let Some(index) = chunks
        .iter()
        .rposition(|chunk| chunk.role == SummarySectionRole::Conclusion)
    {
        anchors.insert(index);
    }
    anchors
}

fn representative(
    chunks: &[SummarySourceChunk],
    assignments: &[usize],
    centroids: &[Vec<f32>],
    populations: &[usize],
    index: usize,
    structural_anchor: bool,
) -> RepresentativeChunk {
    let cluster = assignments[index];
    RepresentativeChunk {
        source: chunks[index].clone(),
        cluster_population: populations[cluster],
        cluster_weight: populations[cluster] as f32 / chunks.len() as f32,
        centroid_distance: squared_distance(&chunks[index].embedding, &centroids[cluster]),
        structural_anchor,
    }
}

fn squared_distance(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| {
            let difference = left - right;
            difference * difference
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(
        id: &str,
        position: u32,
        tokens: usize,
        embedding: [f32; 2],
        role: SummarySectionRole,
    ) -> SummarySourceChunk {
        SummarySourceChunk {
            chunk_id: id.to_owned(),
            text: format!("complete text for {id}"),
            embedding: embedding.to_vec(),
            token_count: tokens,
            source_position: position,
            section_path: vec![format!("Section {position}")],
            provenance: format!("page:{position}"),
            role,
            generated: false,
        }
    }

    #[test]
    fn short_documents_use_every_source_chunk_in_source_order() {
        let chunks = vec![
            chunk(
                "conclusion",
                30,
                10,
                [1.0, 1.0],
                SummarySectionRole::Conclusion,
            ),
            chunk("title", 0, 5, [0.0, 0.0], SummarySectionRole::Title),
            chunk("body", 10, 20, [0.5, 0.5], SummarySectionRole::Body),
        ];

        let selected = select_representative_chunks(
            3,
            "hash",
            "model",
            &chunks,
            RepresentativeSelectionConfig::for_budget(100),
            &CancellationToken::new(),
        )
        .unwrap();

        assert_eq!(selected.mode, RepresentativeSelectionMode::AllChunks);
        assert_eq!(
            selected
                .representatives
                .iter()
                .map(|item| item.source.chunk_id.as_str())
                .collect::<Vec<_>>(),
            ["title", "body", "conclusion"]
        );
        assert_eq!(selected.selected_tokens, 35);
    }

    #[test]
    fn clustered_selection_is_deterministic_for_unsorted_interleaved_themes() {
        let chunks = vec![
            chunk("a-late", 50, 20, [0.0, 0.1], SummarySectionRole::Body),
            chunk("b-early", 10, 20, [10.0, 10.0], SummarySectionRole::Body),
            chunk("a-early", 20, 20, [0.1, 0.0], SummarySectionRole::Body),
            chunk("b-late", 40, 20, [10.1, 10.0], SummarySectionRole::Body),
            chunk("outlier", 30, 20, [50.0, 50.0], SummarySectionRole::Body),
        ];
        let config = RepresentativeSelectionConfig {
            input_token_budget: 60,
            minimum_clusters: 2,
            maximum_clusters: 3,
            maximum_iterations: 16,
        };

        let first = select_representative_chunks(
            7,
            "content",
            "model",
            &chunks,
            config,
            &CancellationToken::new(),
        )
        .unwrap();
        let second = select_representative_chunks(
            7,
            "content",
            "model",
            &chunks,
            config,
            &CancellationToken::new(),
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first
                .representatives
                .iter()
                .map(|item| item.source.source_position)
                .collect::<Vec<_>>(),
            {
                let mut positions = first
                    .representatives
                    .iter()
                    .map(|item| item.source.source_position)
                    .collect::<Vec<_>>();
                positions.sort_unstable();
                positions
            }
        );
    }

    #[test]
    fn duplicate_vectors_and_empty_clusters_still_select_unique_real_chunks() {
        let chunks = (0..6)
            .map(|position| {
                chunk(
                    &format!("same-{position}"),
                    position,
                    10,
                    [1.0, 1.0],
                    SummarySectionRole::Body,
                )
            })
            .collect::<Vec<_>>();

        let selected = select_representative_chunks(
            1,
            "hash",
            "model",
            &chunks,
            RepresentativeSelectionConfig {
                input_token_budget: 30,
                minimum_clusters: 3,
                maximum_clusters: 3,
                maximum_iterations: 8,
            },
            &CancellationToken::new(),
        )
        .unwrap();

        let mut ids = selected
            .representatives
            .iter()
            .map(|item| item.source.chunk_id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), selected.representatives.len());
        assert!(
            selected
                .representatives
                .iter()
                .all(|item| item.source.text.starts_with("complete"))
        );
    }

    #[test]
    fn structural_anchors_are_preserved_and_outlier_weight_is_small() {
        let mut chunks = vec![
            chunk("title", 0, 10, [0.0, 0.0], SummarySectionRole::Title),
            chunk("intro", 1, 10, [0.1, 0.0], SummarySectionRole::Introduction),
            chunk(
                "conclusion",
                99,
                10,
                [0.2, 0.0],
                SummarySectionRole::Conclusion,
            ),
        ];
        chunks.extend((2..9).map(|position| {
            chunk(
                &format!("theme-{position}"),
                position,
                10,
                [1.0, 1.0],
                SummarySectionRole::Body,
            )
        }));
        chunks.push(chunk(
            "outlier",
            50,
            10,
            [100.0, 100.0],
            SummarySectionRole::Body,
        ));

        let selected = select_representative_chunks(
            1,
            "hash",
            "model",
            &chunks,
            RepresentativeSelectionConfig {
                input_token_budget: 60,
                minimum_clusters: 3,
                maximum_clusters: 6,
                maximum_iterations: 16,
            },
            &CancellationToken::new(),
        )
        .unwrap();

        for id in ["title", "intro", "conclusion"] {
            assert!(
                selected
                    .representatives
                    .iter()
                    .any(|item| item.source.chunk_id == id && item.structural_anchor)
            );
        }
        let outlier = selected
            .representatives
            .iter()
            .find(|item| item.source.chunk_id == "outlier")
            .unwrap();
        assert!(outlier.cluster_weight < 0.2);
    }

    #[test]
    fn generated_chunks_are_excluded_and_no_chunk_is_clipped_to_fit() {
        let mut generated = chunk("old-summary", 0, 5, [0.0, 0.0], SummarySectionRole::Body);
        generated.generated = true;
        let chunks = vec![
            generated,
            chunk("large", 1, 40, [1.0, 1.0], SummarySectionRole::Body),
            chunk("small", 2, 10, [2.0, 2.0], SummarySectionRole::Body),
            chunk("small-2", 3, 10, [3.0, 3.0], SummarySectionRole::Body),
        ];

        let selected = select_representative_chunks(
            1,
            "hash",
            "model",
            &chunks,
            RepresentativeSelectionConfig::for_budget(20),
            &CancellationToken::new(),
        )
        .unwrap();

        assert!(selected.selected_tokens <= 20);
        assert!(
            selected
                .representatives
                .iter()
                .all(|item| item.source.chunk_id != "old-summary"
                    && item.source.text.starts_with("complete"))
        );
    }

    #[test]
    fn cancellation_is_checked_before_cpu_work() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        assert_eq!(
            select_representative_chunks(
                1,
                "hash",
                "model",
                &[chunk("body", 0, 10, [0.0, 0.0], SummarySectionRole::Body)],
                RepresentativeSelectionConfig::for_budget(100),
                &cancellation,
            ),
            Err(RepresentativeSelectionError::Cancelled)
        );
    }
}
