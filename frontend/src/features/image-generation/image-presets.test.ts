import { describe, expect, it } from 'vitest';
import {
  composeNegativePrompt,
  composePrompt,
  IMAGE_APERTURES,
  IMAGE_LENSES,
  IMAGE_PRESETS,
  presetById,
  presetsAreCompatible,
  selectedPresets,
  togglePreset,
} from './image-presets';

describe('presets', () => {
  it('carries wording that holds a style for every preset', () => {
    for (const preset of IMAGE_PRESETS) {
      expect(preset.suffix.trim().length).toBeGreaterThan(10);
      expect(preset.negative.trim().length).toBeGreaterThan(0);
      expect(preset.icon).toMatch(/^[a-z_]+$/u);
    }
  });

  it('offers a square or a clearly shaped size for every preset', () => {
    for (const preset of IMAGE_PRESETS) {
      expect(preset.width % 8).toBe(0);
      expect(preset.height % 8).toBe(0);
      expect(preset.width).toBeGreaterThanOrEqual(64);
      expect(preset.height).toBeLessThanOrEqual(2_048);
    }
  });

  it('keeps the ids the pane stores unique', () => {
    const ids = IMAGE_PRESETS.map((preset) => preset.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(presetById('clipart')?.photographic).toBe(false);
    expect(presetById('blackAndWhite')?.photographic).toBe(true);
    expect(presetById('nothing')).toBeUndefined();
  });

  it('names how the picture is made separately from what is done to it', () => {
    // The look of a picture is one choice; a treatment such as monochrome rides on top of it.
    expect(presetById('clipart')?.group).toBe('medium');
    expect(presetById('cinematic')?.group).toBe('medium');
    expect(presetById('portrait')?.group).toBe('medium');
    expect(presetById('blackAndWhite')?.group).toBe('treatment');
  });
});

describe('presetsAreCompatible', () => {
  it('refuses two ways of making the same picture', () => {
    expect(presetsAreCompatible(['clipart', 'cinematic'])).toBe(false);
    expect(presetsAreCompatible(['portrait', 'pattern'])).toBe(false);
  });

  it('allows a treatment alongside a medium', () => {
    expect(presetsAreCompatible(['clipart'])).toBe(true);
    expect(presetsAreCompatible(['clipart', 'blackAndWhite'])).toBe(true);
    expect(presetsAreCompatible(['cinematic', 'blackAndWhite'])).toBe(true);
  });

  it('allows nothing at all, and ignores ids it does not know', () => {
    expect(presetsAreCompatible([])).toBe(true);
    expect(presetsAreCompatible(['clipart', 'nonsense'])).toBe(true);
  });
});

describe('togglePreset', () => {
  it('adds a preset that is not on yet', () => {
    expect(togglePreset([], 'clipart')).toEqual(['clipart']);
  });

  it('takes the same preset off again', () => {
    expect(togglePreset(['clipart'], 'clipart')).toEqual([]);
  });

  it('replaces the medium rather than stacking two of them', () => {
    // Flat clipart and a cinematic still cannot describe one picture; the newer choice wins.
    expect(togglePreset(['clipart', 'blackAndWhite'], 'cinematic')).toEqual([
      'blackAndWhite',
      'cinematic',
    ]);
  });

  it('lists the selection in preset order, so the prompt does not depend on the click order', () => {
    expect(togglePreset(['cinematic'], 'blackAndWhite')).toEqual(['blackAndWhite', 'cinematic']);
    expect(togglePreset(['blackAndWhite'], 'cinematic')).toEqual(['blackAndWhite', 'cinematic']);
  });

  it('leaves the selection alone for an id it does not know', () => {
    expect(togglePreset(['clipart'], 'nonsense')).toEqual(['clipart']);
    expect(togglePreset([], 'nonsense')).toEqual([]);
  });
});

describe('selectedPresets', () => {
  it('returns only the presets that exist, in preset order', () => {
    expect(selectedPresets(['cinematic', 'nonsense', 'blackAndWhite']).map((p) => p.id)).toEqual([
      'blackAndWhite',
      'cinematic',
    ]);
  });
});

describe('composePrompt', () => {
  it('starts from what the person typed', () => {
    expect(composePrompt('a lighthouse at dusk', [], '', '')).toBe('a lighthouse at dusk');
  });

  it('appends the style the preset carries', () => {
    const prompt = composePrompt('a fox', ['clipart'], '', '');
    expect(prompt.startsWith('a fox, ')).toBe(true);
    expect(prompt).toContain('bold clean outlines');
  });

  it('combines a treatment with the medium it is applied to', () => {
    const prompt = composePrompt('a fox', ['watercolour', 'blackAndWhite'], '', '');
    expect(prompt).toContain('loose watercolour illustration');
    expect(prompt).toContain('film grain');
  });

  it('adds lens wording for a photographic preset', () => {
    const prompt = composePrompt('a fox', ['portrait'], '85mm', 'f1.8');
    expect(prompt).toContain('85mm portrait lens');
    expect(prompt).toContain('f/1.8');
  });

  it('adds lens wording when only one of the chosen presets is photographic', () => {
    const prompt = composePrompt('a fox', ['blackAndWhite', 'cinematic'], '85mm', '');
    expect(prompt).toContain('85mm portrait lens');
  });

  it('leaves lens wording out of presets no camera suits', () => {
    const prompt = composePrompt('a fox', ['clipart'], '85mm', 'f1.8');
    expect(prompt).not.toContain('85mm');
    expect(prompt).not.toContain('f/1.8');
  });

  it('does not stack commas when parts are missing', () => {
    expect(composePrompt('  ', ['clipart'], '', '')).toBe(IMAGE_PRESETS[0]?.suffix);
    expect(composePrompt('', [], '', '')).toBe('');
  });

  it('stays inside the bound the backend accepts', () => {
    const prompt = composePrompt('a fox '.repeat(100), ['cinematic'], '50mm', 'f8');
    expect(prompt.length).toBeLessThanOrEqual(4_000 + 400);
  });

  it('knows the lens and aperture ids it offers', () => {
    const lenses = IMAGE_LENSES.map((lens) => lens.id);
    const apertures = IMAGE_APERTURES.map((aperture) => aperture.id);
    expect(lenses).toContain('85mm');
    expect(apertures).toContain('f1.8');
  });
});

describe('composeNegativePrompt', () => {
  it("keeps what the person asked to avoid and adds the preset's", () => {
    const negative = composeNegativePrompt('red', ['clipart']);
    expect(negative.startsWith('red, ')).toBe(true);
    expect(negative).toContain('watermark');
  });

  it('merges several presets without repeating a term', () => {
    const negative = composeNegativePrompt('red', ['clipart', 'blackAndWhite']);
    expect(negative).toContain('photo');
    expect(negative).toContain('colour');
    expect(negative.split(',').filter((term) => term.trim() === 'watermark')).toHaveLength(1);
  });

  it('is empty when neither side has anything', () => {
    expect(composeNegativePrompt('  ', [])).toBe('');
  });
});
