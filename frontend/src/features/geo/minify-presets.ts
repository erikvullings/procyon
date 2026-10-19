import type { GeoMinify, GeoMinifyPreview } from '../../models';

export type MinifyPreset = 'whitespace' | 'web' | 'share' | 'custom';

/** The reductions a preset stands for; `custom` keeps whatever the user set. */
export function presetOptions(preset: Exclude<MinifyPreset, 'custom'>): GeoMinify {
  switch (preset) {
    case 'whitespace':
      return {};
    case 'web':
      return { precision: 5, toleranceMeters: 2, removeDuplicates: true };
    case 'share':
      return { precision: 6, removeDuplicates: true };
  }
}

/** Which preset the options equal, if any. */
export function presetOf(options: GeoMinify): MinifyPreset {
  const same = (candidate: GeoMinify): boolean =>
    JSON.stringify(normalise(candidate)) === JSON.stringify(normalise(options));
  for (const preset of ['whitespace', 'web', 'share'] as const) {
    if (same(presetOptions(preset))) return preset;
  }
  return 'custom';
}

function normalise(options: GeoMinify): GeoMinify {
  return {
    ...(options.precision === undefined ? {} : { precision: options.precision }),
    ...(options.toleranceMeters === undefined ? {} : { toleranceMeters: options.toleranceMeters }),
    ...(options.dropElevation === true ? { dropElevation: true } : {}),
    ...(options.dropTime === true ? { dropTime: true } : {}),
    ...(options.dropExtensions === true ? { dropExtensions: true } : {}),
    ...(options.removeDuplicates === true ? { removeDuplicates: true } : {}),
  };
}

/** The options as sent to the backend: unset and empty values are left out. */
export function cleanMinify(options: GeoMinify): GeoMinify {
  const cleaned = normalise(options);
  if (cleaned.precision !== undefined) {
    cleaned.precision = Math.min(15, Math.max(0, Math.round(cleaned.precision)));
  }
  if (cleaned.toleranceMeters !== undefined && !(cleaned.toleranceMeters > 0)) {
    delete cleaned.toleranceMeters;
  }
  return cleaned;
}

/** "42% smaller", or undefined when the result is not smaller. */
export function percentSmaller(before: number, after: number): number | undefined {
  if (before <= 0 || after >= before) return undefined;
  return Math.round(((before - after) / before) * 100);
}

/** Relative change of a track length, for example "-1.2%". */
export function lengthChange(preview: GeoMinifyPreview): string | undefined {
  if (preview.distanceBeforeMeters <= 0) return undefined;
  const change =
    ((preview.distanceAfterMeters - preview.distanceBeforeMeters) / preview.distanceBeforeMeters) *
    100;
  return `${change > 0 ? '+' : ''}${change.toFixed(1)}%`;
}
