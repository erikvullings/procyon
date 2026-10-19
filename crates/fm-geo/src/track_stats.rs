//! Statistics of a recorded track (task 0279).
//!
//! Anything that needs data the file does not reliably contain is left out instead of guessed:
//! no elevation means no gain or loss, and missing, unzoned or out-of-order timestamps mean no
//! durations or speeds.

use crate::model::{ProfilePoint, TrackSummary};
use crate::time::format_timestamp;

const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
/// Elevation changes smaller than this are treated as GPS noise when summing gain and loss.
pub(crate) const ELEVATION_NOISE_METERS: f64 = 3.0;
/// Below this speed the track counts as stopped.
pub(crate) const MOVING_SPEED_MS: f64 = 0.5;
/// Maximum speed is measured over at least this long, so GPS jitter between fixes does not count.
const MAX_SPEED_WINDOW_SECONDS: f64 = 10.0;
const PROFILE_POINTS: usize = 500;

/// One recorded fix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TrackPoint {
    pub(crate) longitude: f64,
    pub(crate) latitude: f64,
    pub(crate) elevation: Option<f64>,
    /// Seconds since the Unix epoch; `None` when absent or without a time zone.
    pub(crate) time: Option<f64>,
    pub(crate) heart_rate: Option<f64>,
}

/// Great-circle distance in metres.
pub(crate) fn haversine(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let (lat_a, lat_b) = (a.latitude.to_radians(), b.latitude.to_radians());
    let delta_lat = lat_b - lat_a;
    let delta_lon = (b.longitude - a.longitude).to_radians();
    let h = (delta_lat / 2.0).sin().powi(2)
        + lat_a.cos() * lat_b.cos() * (delta_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_METERS * h.sqrt().asin()
}

fn elevation_gain_and_loss(points: &[&TrackPoint]) -> Option<(f64, f64)> {
    let mut elevations = points.iter().filter_map(|point| point.elevation);
    let mut reference = elevations.next()?;
    let (mut gain, mut loss) = (0.0, 0.0);
    for elevation in elevations {
        let change = elevation - reference;
        if change.abs() >= ELEVATION_NOISE_METERS {
            if change > 0.0 {
                gain += change;
            } else {
                loss -= change;
            }
            reference = elevation;
        }
    }
    Some((gain, loss))
}

/// Whether every point has a time and the times never go backwards.
fn times_usable(segments: &[Vec<TrackPoint>]) -> bool {
    let mut previous = f64::NEG_INFINITY;
    for point in segments.iter().flatten() {
        match point.time {
            Some(time) if time >= previous => previous = time,
            _ => return false,
        }
    }
    true
}

fn max_speed(segment: &[TrackPoint]) -> Option<f64> {
    if segment.len() < 2 {
        return None;
    }
    let mut cumulative = vec![0.0];
    for pair in segment.windows(2) {
        let last = cumulative.last().copied().unwrap_or(0.0);
        cumulative.push(last + haversine(&pair[0], &pair[1]));
    }
    let mut best: Option<f64> = None;
    let mut end = 0;
    for start in 0..segment.len() {
        end = end.max(start);
        let start_time = segment[start].time?;
        while end < segment.len() - 1 && segment[end].time? - start_time < MAX_SPEED_WINDOW_SECONDS
        {
            end += 1;
        }
        let elapsed = segment[end].time? - start_time;
        if elapsed >= MAX_SPEED_WINDOW_SECONDS {
            let speed = (cumulative[end] - cumulative[start]) / elapsed;
            best = Some(best.map_or(speed, |current| current.max(speed)));
        }
    }
    best
}

fn profile(segments: &[Vec<TrackPoint>], with_time: bool) -> Vec<ProfilePoint> {
    let total: usize = segments.iter().map(Vec::len).sum();
    let step = total.div_ceil(PROFILE_POINTS).max(1);
    let mut result = Vec::new();
    let mut distance = 0.0;
    let mut index = 0;
    for segment in segments {
        let mut previous: Option<&TrackPoint> = None;
        for point in segment {
            let leg = previous.map(|before| haversine(before, point));
            distance += leg.unwrap_or(0.0);
            let last_of_all = index + 1 == total;
            if index % step == 0 || last_of_all {
                let speed = match (with_time, previous, leg) {
                    (true, Some(before), Some(leg)) => match (before.time, point.time) {
                        (Some(a), Some(b)) if b > a => Some(leg / (b - a)),
                        _ => None,
                    },
                    _ => None,
                };
                result.push(ProfilePoint {
                    distance_meters: distance,
                    longitude: point.longitude,
                    latitude: point.latitude,
                    elevation: point.elevation,
                    speed_meters_per_second: speed,
                    heart_rate: point.heart_rate,
                });
            }
            previous = Some(point);
            index += 1;
        }
    }
    result
}

/// Summarises a track made of one or more segments.
pub(crate) fn summarize(
    name: Option<String>,
    feature_index: usize,
    segments: &[Vec<TrackPoint>],
) -> TrackSummary {
    let points: Vec<&TrackPoint> = segments.iter().flatten().collect();
    let distance: f64 = segments
        .iter()
        .flat_map(|segment| segment.windows(2))
        .map(|pair| haversine(&pair[0], &pair[1]))
        .sum();
    let elevations: Vec<f64> = points.iter().filter_map(|point| point.elevation).collect();
    let gain_loss = elevation_gain_and_loss(&points);
    let timed = !points.is_empty() && times_usable(segments);

    let (mut elapsed, mut moving, mut start, mut end, mut average, mut maximum) =
        (None, None, None, None, None, None);
    if timed {
        let first = points.first().and_then(|point| point.time);
        let last = points.last().and_then(|point| point.time);
        if let (Some(first), Some(last)) = (first, last) {
            elapsed = Some(last - first);
            start = Some(format_timestamp(first));
            end = Some(format_timestamp(last));
        }
        let mut moving_seconds = 0.0;
        let mut moving_distance = 0.0;
        for pair in segments.iter().flat_map(|segment| segment.windows(2)) {
            if let (Some(a), Some(b)) = (pair[0].time, pair[1].time)
                && b > a
            {
                let leg = haversine(&pair[0], &pair[1]);
                if leg / (b - a) >= MOVING_SPEED_MS {
                    moving_seconds += b - a;
                    moving_distance += leg;
                }
            }
        }
        if moving_seconds > 0.0 {
            moving = Some(moving_seconds);
            average = Some(moving_distance / moving_seconds);
        }
        maximum = segments
            .iter()
            .filter_map(|segment| max_speed(segment))
            .fold(None, |best: Option<f64>, speed| {
                Some(best.map_or(speed, |b| b.max(speed)))
            });
    }
    TrackSummary {
        name,
        feature_index,
        points: points.len(),
        distance_meters: distance,
        elapsed_seconds: elapsed,
        moving_seconds: moving,
        ascent_meters: gain_loss.map(|(gain, _)| gain),
        descent_meters: gain_loss.map(|(_, loss)| loss),
        min_elevation: elevations.iter().copied().reduce(f64::min),
        max_elevation: elevations.iter().copied().reduce(f64::max),
        average_speed: average,
        max_speed: maximum,
        start_time: start,
        end_time: end,
        profile: profile(segments, timed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(latitude: f64, elevation: Option<f64>, time: Option<f64>) -> TrackPoint {
        TrackPoint {
            longitude: 0.0,
            latitude,
            elevation,
            time,
            heart_rate: None,
        }
    }

    #[test]
    fn a_degree_of_latitude_is_about_111_kilometres() {
        let distance = haversine(&point(0.0, None, None), &point(1.0, None, None));
        assert!((distance - 111_195.0).abs() < 50.0, "{distance}");
    }

    #[test]
    fn sums_elevation_gain_and_loss_ignoring_noise() {
        let elevations = [100.0, 101.0, 99.5, 110.0, 120.0, 118.0, 100.0];
        let segment: Vec<TrackPoint> = elevations
            .iter()
            .enumerate()
            .map(|(index, elevation)| point(index as f64 * 0.001, Some(*elevation), None))
            .collect();
        let summary = summarize(None, 0, &[segment]);
        // 100 -> 110 -> 120 is +20; 120 -> 100 is -20 (the 118 dip is below the noise floor).
        assert_eq!(summary.ascent_meters, Some(20.0));
        assert_eq!(summary.descent_meters, Some(20.0));
        assert_eq!(summary.min_elevation, Some(99.5));
        assert_eq!(summary.max_elevation, Some(120.0));
        assert_eq!(summary.elapsed_seconds, None);
    }

    #[test]
    fn omits_elevation_statistics_when_the_file_has_no_elevation() {
        let summary = summarize(
            None,
            0,
            &[vec![point(0.0, None, None), point(0.001, None, None)]],
        );
        assert_eq!(summary.ascent_meters, None);
        assert_eq!(summary.min_elevation, None);
        assert!(summary.distance_meters > 100.0);
    }

    #[test]
    fn computes_durations_and_speeds_from_timestamps() {
        // 0.001 degrees ~ 111 m per 10 s while moving, then a 60 s stop.
        let mut points = Vec::new();
        for step in 0..=6 {
            points.push(point(
                f64::from(step) * 0.001,
                None,
                Some(f64::from(step) * 10.0),
            ));
        }
        points.push(point(0.006, None, Some(120.0)));
        let summary = summarize(Some("run".into()), 3, &[points]);
        assert_eq!(summary.feature_index, 3);
        assert_eq!(summary.elapsed_seconds, Some(120.0));
        assert_eq!(summary.moving_seconds, Some(60.0));
        let average = summary.average_speed.expect("average");
        assert!((average - 11.12).abs() < 0.1, "{average}");
        let maximum = summary.max_speed.expect("maximum");
        assert!((maximum - 11.12).abs() < 0.1, "{maximum}");
        assert_eq!(summary.start_time.as_deref(), Some("1970-01-01T00:00:00Z"));
        assert_eq!(summary.end_time.as_deref(), Some("1970-01-01T00:02:00Z"));
    }

    #[test]
    fn omits_time_statistics_for_missing_or_unordered_timestamps() {
        let missing = summarize(
            None,
            0,
            &[vec![point(0.0, None, Some(0.0)), point(0.001, None, None)]],
        );
        assert_eq!(missing.elapsed_seconds, None);
        assert_eq!(missing.average_speed, None);
        assert!(
            missing
                .profile
                .iter()
                .all(|p| p.speed_meters_per_second.is_none())
        );
        let backwards = summarize(
            None,
            0,
            &[vec![
                point(0.0, None, Some(10.0)),
                point(0.001, None, Some(5.0)),
            ]],
        );
        assert_eq!(backwards.elapsed_seconds, None);
        assert_eq!(backwards.max_speed, None);
    }

    #[test]
    fn does_not_connect_segments() {
        let a = vec![point(0.0, None, None), point(0.001, None, None)];
        let b = vec![point(1.0, None, None), point(1.001, None, None)];
        let joined = summarize(None, 0, &[a.clone(), b.clone()]);
        let single =
            summarize(None, 0, &[a]).distance_meters + summarize(None, 0, &[b]).distance_meters;
        assert!((joined.distance_meters - single).abs() < 1e-6);
    }

    #[test]
    fn the_profile_is_bounded_and_keeps_the_end() {
        let points: Vec<TrackPoint> = (0..5_000)
            .map(|index| point(f64::from(index) * 0.0001, Some(f64::from(index)), None))
            .collect();
        let summary = summarize(None, 0, &[points]);
        assert!(summary.profile.len() <= PROFILE_POINTS + 1);
        let last = summary.profile.last().expect("profile");
        assert!((last.distance_meters - summary.distance_meters).abs() < 1e-6);
        assert_eq!(summary.points, 5_000);
    }
}
