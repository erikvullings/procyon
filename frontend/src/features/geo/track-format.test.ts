import { describe, expect, it } from 'vitest';
import type { GeoProfilePoint } from '../../models';
import {
  availableSeries,
  chartPath,
  formatDistance,
  formatDuration,
  formatSpeed,
  nearestSample,
  seriesText,
} from './track-format';

const point = (distanceMeters: number, extra: Partial<GeoProfilePoint> = {}): GeoProfilePoint => ({
  distanceMeters,
  longitude: 0,
  latitude: 0,
  ...extra,
});

describe('formatting', () => {
  it('formats distances, durations and speeds', () => {
    expect(formatDistance(800)).toBe('800 m');
    expect(formatDistance(12_345)).toBe('12.35 km');
    expect(formatDuration(65)).toBe('1:05');
    expect(formatDuration(3725)).toBe('1:02:05');
    expect(formatSpeed(10)).toBe('36.0 km/h');
    expect(seriesText(72.4, 'heartRate')).toBe('72 bpm');
  });
});

describe('profile helpers', () => {
  const points = [
    point(0, { elevation: 10, heartRate: 100 }),
    point(100, { elevation: 20 }),
    point(200, { elevation: null, heartRate: 120 }),
    point(300, { elevation: 15 }),
  ];

  it('knows which series have data', () => {
    expect(availableSeries(points)).toEqual(['elevation', 'heartRate']);
    expect(availableSeries([point(0)])).toEqual([]);
  });

  it('finds the sample nearest to a distance', () => {
    expect(nearestSample(points, 140)).toBe(1);
    expect(nearestSample(points, 1000)).toBe(3);
    expect(nearestSample([], 5)).toBeUndefined();
  });

  it('breaks the line where values are missing and scales into the box', () => {
    const path = chartPath(points, 'elevation', 300, 100);
    expect(path).toBeDefined();
    expect(path?.min).toBe(10);
    expect(path?.max).toBe(20);
    expect(path?.line).toBe('M0.0 100.0 L100.0 0.0 M300.0 50.0');
    expect(chartPath([point(0)], 'speed', 10, 10)).toBeUndefined();
  });
});
