/**
 * Friendly starting points for the image prompt pane (task 0325).
 *
 * A person who has never written a prompt should still get a usable picture: each preset carries the
 * wording that makes the style hold up, the things to ask the model to avoid, a shape that suits the
 * subject, and whether lens wording makes sense for it.
 */

/** The styles the pane offers. */
export type ImagePresetId =
  | 'clipart'
  | 'blackAndWhite'
  | 'portrait'
  | 'product'
  | 'watercolour'
  | 'flatIllustration'
  | 'cinematic'
  | 'isometric'
  | 'pattern'
  | 'sticker';

/** A preset the person can pick with one click. */
export interface ImagePreset {
  readonly id: ImagePresetId;
  /** Material icon name shown on the chip. */
  readonly icon: string;
  /** Wording appended to what the person typed; it carries the style. */
  readonly suffix: string;
  /** What to ask the model to avoid. */
  readonly negative: string;
  /** A shape that suits the subject; the person can still change it. */
  readonly width: number;
  readonly height: number;
  /** Whether lens and aperture wording helps this preset rather than confuses it. */
  readonly photographic: boolean;
}

/** The presets offered in the prompt pane, in the order they are shown. */
export const IMAGE_PRESETS: readonly ImagePreset[] = [
  {
    id: 'clipart',
    icon: 'vector',
    suffix:
      'simple flat clipart, bold clean outlines, solid colours, plain white background, no shading',
    negative: 'photo, realism, gradient, text, watermark',
    width: 1_024,
    height: 1_024,
    photographic: false,
  },
  {
    id: 'blackAndWhite',
    icon: 'camera',
    suffix:
      'black and white photograph, fine film grain, soft natural light, timeless documentary feel',
    negative: 'colour, oversaturated, cartoon, text, watermark',
    width: 1_216,
    height: 832,
    photographic: true,
  },
  {
    id: 'portrait',
    icon: 'portrait',
    suffix: 'portrait photograph, gentle catchlight, blurred background, warm natural skin tones',
    negative: 'deformed hands, extra fingers, text, watermark',
    width: 832,
    height: 1_216,
    photographic: true,
  },
  {
    id: 'product',
    icon: 'shopping_bag',
    suffix:
      'product photograph on a seamless backdrop, soft studio lighting, crisp detail, subtle contact shadow',
    negative: 'clutter, hands, text, watermark',
    width: 1_024,
    height: 1_024,
    photographic: true,
  },
  {
    id: 'watercolour',
    icon: 'brush',
    suffix: 'loose watercolour illustration, soft washes, visible paper texture, gentle palette',
    negative: 'photo, heavy outlines, text, watermark',
    width: 1_024,
    height: 1_024,
    photographic: false,
  },
  {
    id: 'flatIllustration',
    icon: 'category',
    suffix: 'flat vector illustration, simple shapes, limited palette, generous negative space',
    negative: 'photo, gradient, text, watermark',
    width: 1_216,
    height: 832,
    photographic: false,
  },
  {
    id: 'cinematic',
    icon: 'movie',
    suffix: 'cinematic still, golden hour light, shallow depth of field, subtle film grain',
    negative: 'cartoon, text, watermark',
    width: 1_344,
    height: 768,
    photographic: true,
  },
  {
    id: 'isometric',
    icon: 'deployed_code',
    suffix: 'isometric 3D illustration, soft shadows, pastel palette, clean geometry',
    negative: 'photo, text, watermark',
    width: 1_024,
    height: 1_024,
    photographic: false,
  },
  {
    id: 'pattern',
    icon: 'grid_view',
    suffix: 'seamless repeating pattern, balanced motifs, even spacing, flat colours',
    negative: 'photo, text, watermark, vignette',
    width: 1_024,
    height: 1_024,
    photographic: false,
  },
  {
    id: 'sticker',
    icon: 'loyalty',
    suffix: 'die-cut sticker, bold outline, cheerful colours, plain background',
    negative: 'photo, text, watermark, shadow',
    width: 1_024,
    height: 1_024,
    photographic: false,
  },
] as const;

/** The preset id a pane starts on: nothing is chosen until the person picks one. */
export const NO_PRESET = '';

/** Lens wording, offered only for presets where a camera makes sense. */
export interface ImageLens {
  readonly id: string;
  readonly clause: string;
}

/** Focal lengths a non-specialist recognises from a camera. */
export const IMAGE_LENSES: readonly ImageLens[] = [
  { id: '', clause: '' },
  { id: '24mm', clause: 'shot on a 24mm wide-angle lens' },
  { id: '35mm', clause: 'shot on a 35mm lens' },
  { id: '50mm', clause: 'shot on a 50mm lens' },
  { id: '85mm', clause: 'shot on an 85mm portrait lens' },
  { id: '100mm-macro', clause: 'shot on a 100mm macro lens' },
] as const;

/** Apertures, phrased by what they do rather than by the number alone. */
export const IMAGE_APERTURES: readonly ImageLens[] = [
  { id: '', clause: '' },
  { id: 'f1.8', clause: 'wide open at f/1.8 with a blurred background' },
  { id: 'f4', clause: 'at f/4 with the subject sharp' },
  { id: 'f8', clause: 'stopped down to f/8 with everything in focus' },
] as const;

/** The preset with this id, or `undefined`. */
export function presetById(id: string): ImagePreset | undefined {
  return IMAGE_PRESETS.find((preset) => preset.id === id);
}

/** The clause for a lens id; an unknown or empty id contributes nothing. */
export function clauseById(list: readonly ImageLens[], id: string): string {
  return list.find((entry) => entry.id === id)?.clause ?? '';
}

/**
 * The prompt actually sent: what the person typed, then the style the preset carries, then the lens
 * wording when it applies. Empty parts are dropped so no double commas appear.
 */
export function composePrompt(
  subject: string,
  presetId: string,
  lensId: string,
  apertureId: string,
): string {
  const preset = presetById(presetId);
  const photographic = preset?.photographic ?? false;
  const parts = [
    subject.trim(),
    preset?.suffix ?? '',
    photographic ? clauseById(IMAGE_LENSES, lensId) : '',
    photographic ? clauseById(IMAGE_APERTURES, apertureId) : '',
  ].filter((part) => part.trim() !== '');
  return parts.join(', ');
}

/**
 * The negative prompt sent: the person's own wording first, then the preset's, so a preset never
 * silently drops what the person asked to avoid.
 */
export function composeNegativePrompt(own: string, presetId: string): string {
  const preset = presetById(presetId);
  const parts = [own.trim(), preset?.negative ?? ''].filter((part) => part !== '');
  return parts.join(', ');
}
