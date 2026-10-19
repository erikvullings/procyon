import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { GeoProfilePoint } from '../../models';
import {
  availableSeries,
  chartPath,
  formatDistance,
  nearestSample,
  type ProfileSeries,
  seriesText,
  seriesValue,
} from './track-format';

export interface ElevationProfileAttrs {
  readonly name: string;
  readonly points: readonly GeoProfilePoint[];
  /** The sample under the pointer or keyboard cursor; undefined when it leaves the chart. */
  readonly onHover: (point: GeoProfilePoint | undefined) => void;
}

const WIDTH = 320;
const HEIGHT = 110;
const SERIES_LABELS = {
  elevation: 'seriesElevation',
  speed: 'seriesSpeed',
  heartRate: 'seriesHeartRate',
} as const;

/**
 * Elevation (and, when recorded, speed or heart-rate) profile of a track over distance. Hovering
 * or moving the keyboard cursor reports the sample so the map can show where it is.
 */
export const ElevationProfile: FactoryComponent<ElevationProfileAttrs> = () => {
  let series: ProfileSeries = 'elevation';
  let index: number | undefined;

  return {
    view: ({ attrs }) => {
      const options = availableSeries(attrs.points);
      if (options.length === 0) return undefined;
      if (!options.includes(series)) series = options[0] ?? 'elevation';
      const path = chartPath(attrs.points, series, WIDTH, HEIGHT);
      if (path === undefined) return undefined;
      const active = index === undefined ? undefined : attrs.points[index];
      const activeValue = active === undefined ? undefined : seriesValue(active, series);
      const moveTo = (next: number | undefined): void => {
        index = next;
        attrs.onHover(next === undefined ? undefined : attrs.points[next]);
      };
      return m('.fm-elevation-profile', [
        options.length > 1
          ? m(
              '.fm-elevation-series',
              { role: 'group', 'aria-label': t('geoTrack', 'profile') },
              options.map((option) =>
                m(
                  'button.btn-flat',
                  {
                    key: option,
                    type: 'button',
                    'aria-pressed': String(option === series),
                    onclick: () => {
                      series = option;
                    },
                  },
                  t('geoTrack', SERIES_LABELS[option]),
                ),
              ),
            )
          : undefined,
        m(
          'svg.fm-elevation-chart',
          {
            viewBox: `0 0 ${WIDTH} ${HEIGHT}`,
            role: 'img',
            tabindex: 0,
            'aria-label': t('geoTrack', 'chartLabel', { name: attrs.name }),
            onpointermove: (event: PointerEvent) => {
              const box = (event.currentTarget as SVGElement).getBoundingClientRect();
              const ratio = box.width === 0 ? 0 : (event.clientX - box.left) / box.width;
              moveTo(
                nearestSample(attrs.points, Math.min(1, Math.max(0, ratio)) * path.maxDistance),
              );
            },
            onpointerleave: () => moveTo(undefined),
            onblur: () => moveTo(undefined),
            onkeydown: (event: KeyboardEvent) => {
              if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
              event.preventDefault();
              const step = event.key === 'ArrowRight' ? 1 : -1;
              moveTo(Math.min(attrs.points.length - 1, Math.max(0, (index ?? 0) + step)));
            },
          },
          [
            m('path.fm-elevation-line', { d: path.line, fill: 'none' }),
            active === undefined
              ? undefined
              : m('line.fm-elevation-guide', {
                  x1: (active.distanceMeters / path.maxDistance) * WIDTH,
                  x2: (active.distanceMeters / path.maxDistance) * WIDTH,
                  y1: 0,
                  y2: HEIGHT,
                }),
          ],
        ),
        m(
          '.fm-elevation-readout',
          { 'aria-live': 'polite' },
          active === undefined || activeValue === undefined
            ? `${seriesText(path.min, series)} - ${seriesText(path.max, series)}`
            : t('geoTrack', 'pointerReadout', {
                distance: formatDistance(active.distanceMeters),
                value: seriesText(activeValue, series),
              }),
        ),
      ]);
    },
  };
};
