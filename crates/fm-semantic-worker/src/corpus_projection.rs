//! Deterministic 2D projection of embedding vectors for the theme map.
//!
//! Principal component analysis by power iteration with deflation: no random
//! numbers, a fixed iteration count and a fixed sign convention, so the same
//! ordered input always yields the same coordinates. Only the projected
//! coordinates leave this module; the vectors never do.

const ITERATIONS: usize = 48;
const EPSILON: f64 = 1e-12;

/// Projects `vectors` onto their first two principal axes.
///
/// Coordinates are scaled uniformly so the larger extent spans `[-1, 1]`
/// (aspect ratio is preserved). Degenerate input, such as identical vectors or
/// a single point, yields the origin for every point.
#[must_use]
pub(crate) fn project_2d(vectors: &[&[f32]]) -> Vec<(f32, f32)> {
    let count = vectors.len();
    let Some(first) = vectors.first() else {
        return Vec::new();
    };
    let dimensions = first.len();
    if count < 2 || dimensions == 0 {
        return vec![(0.0, 0.0); count];
    }
    let mean = (0..dimensions)
        .map(|axis| {
            vectors
                .iter()
                .map(|vector| f64::from(vector[axis]))
                .sum::<f64>()
                / count as f64
        })
        .collect::<Vec<_>>();
    let centred = vectors
        .iter()
        .map(|vector| {
            vector
                .iter()
                .zip(&mean)
                .map(|(value, mean)| f64::from(*value) - mean)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let first_axis = principal_axis(&centred, dimensions, &[]);
    let second_axis = principal_axis(&centred, dimensions, &[&first_axis]);
    let coordinates = centred
        .iter()
        .map(|row| (dot(row, &first_axis), dot(row, &second_axis)))
        .collect::<Vec<_>>();
    let extent = coordinates
        .iter()
        .flat_map(|(x, y)| [x.abs(), y.abs()])
        .fold(0.0_f64, f64::max);
    if extent < EPSILON {
        return vec![(0.0, 0.0); count];
    }
    coordinates
        .into_iter()
        .map(|(x, y)| ((x / extent) as f32, (y / extent) as f32))
        .collect()
}

/// Dominant right singular vector of `rows`, orthogonal to `previous`.
fn principal_axis(rows: &[Vec<f64>], dimensions: usize, previous: &[&Vec<f64>]) -> Vec<f64> {
    // A fixed, non-symmetric start avoids being orthogonal to the true axis.
    let mut axis = (0..dimensions)
        .map(|index| 1.0 + (index % 7) as f64 / 10.0)
        .collect::<Vec<_>>();
    orthogonalise(&mut axis, previous);
    if !normalise(&mut axis) {
        return vec![0.0; dimensions];
    }
    for _ in 0..ITERATIONS {
        let mut next = vec![0.0; dimensions];
        for row in rows {
            let weight = dot(row, &axis);
            for (slot, value) in next.iter_mut().zip(row) {
                *slot += weight * value;
            }
        }
        orthogonalise(&mut next, previous);
        if !normalise(&mut next) {
            return vec![0.0; dimensions];
        }
        axis = next;
    }
    // Sign convention: the component with the largest magnitude is positive.
    let pivot = axis.iter().copied().fold(0.0_f64, |best, value| {
        if value.abs() > best.abs() {
            value
        } else {
            best
        }
    });
    if pivot < 0.0 {
        for value in &mut axis {
            *value = -*value;
        }
    }
    axis
}

fn orthogonalise(vector: &mut [f64], previous: &[&Vec<f64>]) {
    for axis in previous {
        let overlap = dot(vector, axis);
        for (value, base) in vector.iter_mut().zip(axis.iter()) {
            *value -= overlap * base;
        }
    }
}

fn normalise(vector: &mut [f64]) -> bool {
    let norm = dot(vector, vector).sqrt();
    if norm < EPSILON || !norm.is_finite() {
        return false;
    }
    for value in vector {
        *value /= norm;
    }
    true
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(center: [f32; 6], count: usize, salt: f32) -> Vec<Vec<f32>> {
        (0..count)
            .map(|index| {
                center
                    .iter()
                    .enumerate()
                    .map(|(axis, value)| {
                        value + ((index * 7 + axis * 3) as f32 * 0.37 + salt).sin() * 0.05
                    })
                    .collect()
            })
            .collect()
    }

    fn views(vectors: &[Vec<f32>]) -> Vec<&[f32]> {
        vectors.iter().map(Vec::as_slice).collect()
    }

    #[test]
    fn well_separated_groups_stay_separated_in_two_dimensions() {
        let mut vectors = group([1.0, 0.0, 0.0, 0.0, 0.0, 0.0], 12, 0.1);
        vectors.extend(group([0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 12, 0.2));
        vectors.extend(group([0.0, 0.0, 1.0, 0.0, 0.0, 0.0], 12, 0.3));
        let points = project_2d(&views(&vectors));
        let centroid = |range: std::ops::Range<usize>| {
            let slice = &points[range.clone()];
            (
                slice.iter().map(|p| p.0).sum::<f32>() / slice.len() as f32,
                slice.iter().map(|p| p.1).sum::<f32>() / slice.len() as f32,
            )
        };
        let spread = |range: std::ops::Range<usize>| {
            let (cx, cy) = centroid(range.clone());
            points[range]
                .iter()
                .map(|p| ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt())
                .fold(0.0_f32, f32::max)
        };
        let groups = [0..12, 12..24, 24..36];
        for left in 0..3 {
            for right in (left + 1)..3 {
                let (ax, ay) = centroid(groups[left].clone());
                let (bx, by) = centroid(groups[right].clone());
                let between = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
                assert!(
                    between > 2.0 * spread(groups[left].clone()).max(spread(groups[right].clone())),
                    "groups {left} and {right} overlap"
                );
            }
        }
    }

    #[test]
    fn coordinates_are_bounded_deterministic_and_order_equivariant() {
        let mut vectors = group([1.0, 0.0, 0.0, 0.0, 0.0, 0.0], 8, 0.4);
        vectors.extend(group([0.0, 0.0, 0.0, 1.0, 0.0, 0.0], 8, 0.5));
        let first = project_2d(&views(&vectors));
        assert_eq!(first, project_2d(&views(&vectors)));
        assert!(first.iter().all(|(x, y)| x.abs() <= 1.0 && y.abs() <= 1.0));
        assert!(first.iter().any(|(x, y)| x.abs().max(y.abs()) > 0.99));

        let mut reversed = vectors.clone();
        reversed.reverse();
        let mut flipped = project_2d(&views(&reversed));
        flipped.reverse();
        for (a, b) in first.iter().zip(&flipped) {
            assert!((a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3);
        }
    }

    #[test]
    fn degenerate_inputs_collapse_to_the_origin() {
        assert!(project_2d(&[]).is_empty());
        let one = [vec![0.5_f32, 0.5]];
        assert_eq!(project_2d(&views(&one)), vec![(0.0, 0.0)]);
        let same = [vec![1.0_f32, 2.0], vec![1.0, 2.0], vec![1.0, 2.0]];
        assert_eq!(project_2d(&views(&same)), vec![(0.0, 0.0); 3]);
    }
}
