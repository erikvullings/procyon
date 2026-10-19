import { describe, expect, it } from 'vitest';
import { applyPreset, MAP_PRESETS, presetFor } from './map-presets';
import { DEFAULT_MAP_SETTINGS, validateMapSettings } from './map-settings';

describe('map presets', () => {
  it('only offers settings the editor and backend accept', () => {
    for (const preset of MAP_PRESETS) {
      expect(
        validateMapSettings(applyPreset(DEFAULT_MAP_SETTINGS, preset)),
        preset.id,
      ).toBeUndefined();
    }
  });

  it('has unique ids and includes the Dutch PDOK services', () => {
    expect(new Set(MAP_PRESETS.map((preset) => preset.id)).size).toBe(MAP_PRESETS.length);
    expect(MAP_PRESETS.filter((preset) => preset.id.startsWith('pdok')).length).toBeGreaterThan(1);
  });

  it('recognises the applied preset and the default', () => {
    const pdok = MAP_PRESETS.find((preset) => preset.id === 'pdok-brt');
    expect(pdok).toBeDefined();
    if (pdok === undefined) return;
    const applied = applyPreset(DEFAULT_MAP_SETTINGS, pdok);
    expect(applied.remoteHost).toBe('service.pdok.nl');
    expect(presetFor(applied)?.id).toBe('pdok-brt');
    expect(presetFor(DEFAULT_MAP_SETTINGS)?.id).toBe('osm');
  });

  it('keeps the online switch when a preset is chosen', () => {
    const offline = { ...DEFAULT_MAP_SETTINGS, onlineTiles: false };
    const preset = MAP_PRESETS[1];
    if (preset === undefined) throw new Error('no preset');
    expect(applyPreset(offline, preset).onlineTiles).toBe(false);
  });
});
