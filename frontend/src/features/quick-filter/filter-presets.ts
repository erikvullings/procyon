import type { FilterPreset } from '../../models/settings';
import type { AdvancedFilterCriteria } from './advanced-filter';

export const MAX_FILTER_PRESET_NAME_CHARS = 80;

export function normalizePresetName(name: string): string {
  return name.trim().slice(0, MAX_FILTER_PRESET_NAME_CHARS).trim();
}

function sameName(left: string, right: string): boolean {
  return left.toLocaleLowerCase() === right.toLocaleLowerCase();
}

export function findFilterPreset(
  presets: readonly FilterPreset[],
  name: string,
): FilterPreset | undefined {
  return presets.find((preset) => sameName(preset.name, name));
}

export type PresetEditResult =
  | { readonly ok: true; readonly presets: readonly FilterPreset[]; readonly name: string }
  | { readonly ok: false; readonly reason: 'blank' | 'missing' | 'duplicate' };

/** Saves `criteria` under `name`, replacing a preset of the same (case-insensitive) name. */
export function saveFilterPreset(
  presets: readonly FilterPreset[],
  name: string,
  criteria: AdvancedFilterCriteria,
): PresetEditResult {
  const normalized = normalizePresetName(name);
  if (normalized === '') return { ok: false, reason: 'blank' };
  const existing = findFilterPreset(presets, normalized);
  // Overwriting keeps the stored spelling so a save never silently renames a preset.
  const storedName = existing?.name ?? normalized;
  const preset: FilterPreset = { name: storedName, criteria: { ...criteria } };
  return {
    ok: true,
    name: storedName,
    presets:
      existing === undefined
        ? [...presets, preset]
        : presets.map((current) => (current === existing ? preset : current)),
  };
}

/** Renames a preset; the new name must not collide with a different preset. */
export function renameFilterPreset(
  presets: readonly FilterPreset[],
  from: string,
  to: string,
): PresetEditResult {
  const normalized = normalizePresetName(to);
  if (normalized === '') return { ok: false, reason: 'blank' };
  const source = findFilterPreset(presets, from);
  if (source === undefined) return { ok: false, reason: 'missing' };
  const clash = findFilterPreset(presets, normalized);
  if (clash !== undefined && clash !== source) return { ok: false, reason: 'duplicate' };
  return {
    ok: true,
    name: normalized,
    presets: presets.map((current) =>
      current === source ? { ...current, name: normalized } : current,
    ),
  };
}

export function deleteFilterPreset(
  presets: readonly FilterPreset[],
  name: string,
): readonly FilterPreset[] {
  return presets.filter((preset) => !sameName(preset.name, name));
}
