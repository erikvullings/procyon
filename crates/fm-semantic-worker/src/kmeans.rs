//! Deterministic k-means shared by document summaries and corpus themes.
//!
//! Callers sort their points into a stable order first. Every tie is broken by
//! point index, so the same ordered input, `k` and seed always produce the same
//! centroids and assignments.

use tokio_util::sync::CancellationToken;

/// Finished clustering: one centroid per cluster and one assignment per point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KMeans {
    /// Cluster centers. A cluster that lost all members keeps its last center.
    pub(crate) centroids: Vec<Vec<f32>>,
    /// Cluster index for each input point.
    pub(crate) assignments: Vec<usize>,
}

/// Clustering stopped because cancellation was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cancelled;

/// Runs farthest-first-initialized Lloyd iterations.
///
/// `count` must be in `1..=points.len()` and every point must share one
/// dimension; callers validate both before calling.
pub(crate) fn run(
    points: &[&[f32]],
    count: usize,
    seed: usize,
    maximum_iterations: usize,
    cancellation: &CancellationToken,
) -> Result<KMeans, Cancelled> {
    let mut centroids = initialize_centroids(points, count, seed);
    let mut assignments = vec![usize::MAX; points.len()];
    for _ in 0..maximum_iterations {
        if cancellation.is_cancelled() {
            return Err(Cancelled);
        }
        let next = assign_all(points, &centroids);
        let converged = next == assignments;
        assignments = next;
        if converged {
            break;
        }
        recompute_centroids(points, &assignments, &mut centroids);
    }
    Ok(KMeans {
        centroids,
        assignments,
    })
}

/// Nearest centroid, preferring the lower index on exact ties.
pub(crate) fn nearest_centroid(point: &[f32], centroids: &[Vec<f32>]) -> (usize, f32) {
    centroids
        .iter()
        .enumerate()
        .map(|(index, centroid)| (index, squared_distance(point, centroid)))
        .min_by(|(left_index, left), (right_index, right)| {
            left.total_cmp(right)
                .then_with(|| left_index.cmp(right_index))
        })
        .expect("clustering always has at least one centroid")
}

/// Squared Euclidean distance.
pub(crate) fn squared_distance(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| {
            let difference = left - right;
            difference * difference
        })
        .sum()
}

fn initialize_centroids(points: &[&[f32]], count: usize, seed: usize) -> Vec<Vec<f32>> {
    let mut selected = vec![seed];
    while selected.len() < count {
        let candidate = (0..points.len())
            .filter(|index| !selected.contains(index))
            .max_by(|left, right| {
                let distance = |index: usize| {
                    selected
                        .iter()
                        .map(|selected| squared_distance(points[index], points[*selected]))
                        .min_by(f32::total_cmp)
                        .unwrap_or_default()
                };
                distance(*left)
                    .total_cmp(&distance(*right))
                    .then_with(|| right.cmp(left))
            })
            .expect("cluster count never exceeds point count");
        selected.push(candidate);
    }
    selected
        .into_iter()
        .map(|index| points[index].to_vec())
        .collect()
}

fn assign_all(points: &[&[f32]], centroids: &[Vec<f32>]) -> Vec<usize> {
    points
        .iter()
        .map(|point| nearest_centroid(point, centroids).0)
        .collect()
}

fn recompute_centroids(points: &[&[f32]], assignments: &[usize], centroids: &mut [Vec<f32>]) {
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
                .map(|index| points[*index][dimension])
                .sum::<f32>()
                / members.len() as f32;
        }
    }
}
