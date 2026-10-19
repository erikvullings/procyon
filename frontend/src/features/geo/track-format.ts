import type { GeoProfilePoint } from '../../models';

/** 1234 m -> "1.23 km", 800 m -> "800 m". */
export function formatDistance(meters: number): string {
  return meters >= 1000 ? `${(meters / 1000).toFixed(2)} km` : `${Math.round(meters)} m`;
}

/** 3725 s -> "1:02:05". */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  const pad = (value: number): string => String(value).padStart(2, '0');
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(rest)}` : `${minutes}:${pad(rest)}`;
}

/** Metres per second as km/h. */
export function formatSpeed(metersPerSecond: number): string {
  return `${(metersPerSecond * 3.6).toFixed(1)} km/h`;
}

export function formatElevation(meters: number): string {
  return `${Math.round(meters)} m`;
}

export type ProfileSeries = 'elevation' | 'speed' | 'heartRate';

export function seriesValue(point: GeoProfilePoint, series: ProfileSeries): number | undefined {
  const value =
    series === 'elevation'
      ? point.elevation
      : series === 'speed'
        ? point.speedMetersPerSecond
        : point.heartRate;
  return value === null || value === undefined || !Number.isFinite(value) ? undefined : value;
}

/** Which series a profile can chart, in display order. */
export function availableSeries(points: readonly GeoProfilePoint[]): ProfileSeries[] {
  return (['elevation', 'speed', 'heartRate'] as const).filter((series) =>
    points.some((point) => seriesValue(point, series) !== undefined),
  );
}

export function seriesText(value: number, series: ProfileSeries): string {
  if (series === 'elevation') return formatElevation(value);
  if (series === 'speed') return formatSpeed(value);
  return `${Math.round(value)} bpm`;
}

/** Index of the sample closest to `distance`, or undefined for an empty profile. */
export function nearestSample(
  points: readonly GeoProfilePoint[],
  distance: number,
): number | undefined {
  if (points.length === 0) return undefined;
  let best = 0;
  let bestGap = Number.POSITIVE_INFINITY;
  points.forEach((point, index) => {
    const gap = Math.abs(point.distanceMeters - distance);
    if (gap < bestGap) {
      best = index;
      bestGap = gap;
    }
  });
  return best;
}

export interface ChartPath {
  /** SVG path data; runs of missing values are separate sub-paths. */
  readonly line: string;
  readonly min: number;
  readonly max: number;
  readonly maxDistance: number;
}

/** Builds the path of a series inside a `width` x `height` box. */
export function chartPath(
  points: readonly GeoProfilePoint[],
  series: ProfileSeries,
  width: number,
  height: number,
): ChartPath | undefined {
  const values = points
    .map((point) => seriesValue(point, series))
    .filter((value): value is number => value !== undefined);
  if (values.length === 0) return undefined;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const maxDistance = Math.max(points.at(-1)?.distanceMeters ?? 0, 1);
  const span = max - min || 1;
  let line = '';
  let drawing = false;
  for (const point of points) {
    const value = seriesValue(point, series);
    if (value === undefined) {
      drawing = false;
      continue;
    }
    const x = (point.distanceMeters / maxDistance) * width;
    const y = height - ((value - min) / span) * height;
    line += `${drawing ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)} `;
    drawing = true;
  }
  return { line: line.trim(), min, max, maxDistance };
}
