import { describe, expect, it } from 'vitest';
import {
  composeNegativePrompt,
  composePrompt,
  IMAGE_APERTURES,
  IMAGE_LENSES,
  IMAGE_PRESETS,
  presetById,
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
});

describe('composePrompt', () => {
  it('starts from what the person typed', () => {
    expect(composePrompt('a lighthouse at dusk', '', '', '')).toBe('a lighthouse at dusk');
  });

  it('appends the style the preset carries', () => {
    const prompt = composePrompt('a fox', 'clipart', '', '');
    expect(prompt.startsWith('a fox, ')).toBe(true);
    expect(prompt).toContain('bold clean outlines');
  });

  it('adds lens wording for a photographic preset', () => {
    const prompt = composePrompt('a fox', 'portrait', '85mm', 'f1.8');
    expect(prompt).toContain('85mm portrait lens');
    expect(prompt).toContain('f/1.8');
  });

  it('leaves lens wording out of a preset a camera does not suit', () => {
    const prompt = composePrompt('a fox', 'clipart', '85mm', 'f1.8');
    expect(prompt).not.toContain('85mm');
    expect(prompt).not.toContain('f/1.8');
  });

  it('does not stack commas when parts are missing', () => {
    expect(composePrompt('  ', 'clipart', '', '')).toBe(IMAGE_PRESETS[0]?.suffix);
    expect(composePrompt('', '', '', '')).toBe('');
  });

  it('stays inside the bound the backend accepts', () => {
    const prompt = composePrompt('a fox '.repeat(100), 'cinematic', '50mm', 'f8');
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
    const negative = composeNegativePrompt('red', 'clipart');
    expect(negative.startsWith('red, ')).toBe(true);
    expect(negative).toContain('watermark');
  });

  it('is empty when neither side has anything', () => {
    expect(composeNegativePrompt('  ', '')).toBe('');
  });
});
