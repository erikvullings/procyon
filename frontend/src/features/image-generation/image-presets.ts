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

/**
 * What a preset describes. A `medium` is how the picture is made - flat clipart, a watercolour, a
 * photograph, a film still - so only one of them can describe a given picture. A `treatment` is
 * something done to a picture, such as taking the colour out of it, and sits on top of a medium.
 */
export type ImagePresetGroup = 'medium' | 'treatment';

/** A preset the person can switch on with one click. */
export interface ImagePreset {
  readonly id: ImagePresetId;
  /** Material icon name; only the label is shown in the list, the icon belongs on the chip. */
  readonly icon: string;
  /** Whether this preset is the look of the picture, or something done to it. */
  readonly group: ImagePresetGroup;
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
    group: 'medium',
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
    group: 'treatment',
    photographic: true,
  },
  {
    id: 'portrait',
    icon: 'portrait',
    suffix: 'portrait photograph, gentle catchlight, blurred background, warm natural skin tones',
    negative: 'deformed hands, extra fingers, text, watermark',
    width: 832,
    height: 1_216,
    group: 'medium',
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
    group: 'medium',
    photographic: true,
  },
  {
    id: 'watercolour',
    icon: 'brush',
    suffix: 'loose watercolour illustration, soft washes, visible paper texture, gentle palette',
    negative: 'photo, heavy outlines, text, watermark',
    width: 1_024,
    height: 1_024,
    group: 'medium',
    photographic: false,
  },
  {
    id: 'flatIllustration',
    icon: 'category',
    suffix: 'flat vector illustration, simple shapes, limited palette, generous negative space',
    negative: 'photo, gradient, text, watermark',
    width: 1_216,
    height: 832,
    group: 'medium',
    photographic: false,
  },
  {
    id: 'cinematic',
    icon: 'movie',
    suffix: 'cinematic still, golden hour light, shallow depth of field, subtle film grain',
    negative: 'cartoon, text, watermark',
    width: 1_344,
    height: 768,
    group: 'medium',
    photographic: true,
  },
  {
    id: 'isometric',
    icon: 'deployed_code',
    suffix: 'isometric 3D illustration, soft shadows, pastel palette, clean geometry',
    negative: 'photo, text, watermark',
    width: 1_024,
    height: 1_024,
    group: 'medium',
    photographic: false,
  },
  {
    id: 'pattern',
    icon: 'grid_view',
    suffix: 'seamless repeating pattern, balanced motifs, even spacing, flat colours',
    negative: 'photo, text, watermark, vignette',
    width: 1_024,
    height: 1_024,
    group: 'medium',
    photographic: false,
  },
  {
    id: 'sticker',
    icon: 'loyalty',
    suffix: 'die-cut sticker, bold outline, cheerful colours, plain background',
    negative: 'photo, text, watermark, shadow',
    width: 1_024,
    height: 1_024,
    group: 'medium',
    photographic: false,
  },
] as const;

/** Nothing is switched on when a pane opens. */
export const NO_PRESETS: readonly string[] = [];

/** The preset id a pane starts on: nothing is chosen until the person picks one. */
export const NO_PRESET = '';

/** Lens wording, offered only for presets where a camera makes sense. */
export interface ImageLens {
  readonly id: string;
  /** What the status bar shows; the id is a slug and not always something to read out. */
  readonly label: string;
  readonly clause: string;
}

/** Focal lengths a non-specialist recognises from a camera. */
export const IMAGE_LENSES: readonly ImageLens[] = [
  { id: '', label: '', clause: '' },
  { id: '24mm', label: '24mm wide', clause: 'shot on a 24mm wide-angle lens' },
  { id: '35mm', label: '35mm', clause: 'shot on a 35mm lens' },
  { id: '50mm', label: '50mm', clause: 'shot on a 50mm lens' },
  { id: '85mm', label: '85mm portrait', clause: 'shot on an 85mm portrait lens' },
  { id: '100mm-macro', label: '100mm macro', clause: 'shot on a 100mm macro lens' },
] as const;

/** Apertures, phrased by what they do rather than by the number alone. */
export const IMAGE_APERTURES: readonly ImageLens[] = [
  { id: '', label: '', clause: '' },
  { id: 'f1.8', label: 'f/1.8 wide open', clause: 'wide open at f/1.8 with a blurred background' },
  { id: 'f4', label: 'f/4', clause: 'at f/4 with the subject sharp' },
  { id: 'f8', label: 'f/8 all in focus', clause: 'stopped down to f/8 with everything in focus' },
] as const;

/** The preset with this id, or `undefined`. */
export function presetById(id: string): ImagePreset | undefined {
  return IMAGE_PRESETS.find((preset) => preset.id === id);
}

/** The clause for a lens id; an unknown or empty id contributes nothing. */
export function clauseById(list: readonly ImageLens[], id: string): string {
  return list.find((entry) => entry.id === id)?.clause ?? '';
}

/** The presets behind these ids, in the order they are listed; unknown ids contribute nothing. */
export function selectedPresets(ids: readonly string[]): ImagePreset[] {
  return IMAGE_PRESETS.filter((preset) => ids.includes(preset.id));
}

/**
 * Whether a selection can describe one picture: the way it is made is a single choice, so two
 * mediums such as flat clipart and a cinematic still contradict each other. A treatment - the
 * colour taken out of a photograph - is free to join any of them.
 */
export function presetsAreCompatible(ids: readonly string[]): boolean {
  return selectedPresets(ids).filter((preset) => preset.group === 'medium').length <= 1;
}

/**
 * The selection after the person clicks a preset: it comes off when it was on, and goes on beside
 * the others otherwise - replacing whatever medium was on, since only one picture can be made one
 * way. The result is in preset order, so the prompt does not depend on the order of clicks.
 */
export function togglePreset(ids: readonly string[], id: string): string[] {
  if (presetById(id) === undefined) return [...ids];
  const without = ids.filter((candidate) => candidate !== id);
  if (ids.includes(id)) return selectedPresets(without).map((preset) => preset.id);
  const chosen = presetById(id) as ImagePreset;
  const kept =
    chosen.group === 'medium'
      ? without.filter((candidate) => presetById(candidate)?.group !== 'medium')
      : without;
  return selectedPresets([...kept, id]).map((preset) => preset.id);
}

/** Whether a camera makes sense for this selection: any photographic preset is enough. */
export function selectionIsPhotographic(ids: readonly string[]): boolean {
  return selectedPresets(ids).some((preset) => preset.photographic);
}

/**
 * The prompt actually sent: what the person typed, then the wording each chosen preset carries,
 * then the lens wording when a photographic preset is on. Empty parts are dropped so no double
 * commas appear.
 */
export function composePrompt(
  subject: string,
  presetIds: readonly string[],
  lensId: string,
  apertureId: string,
): string {
  const presets = selectedPresets(presetIds);
  const photographic = presets.some((preset) => preset.photographic);
  const parts = [
    subject.trim(),
    ...presets.map((preset) => preset.suffix),
    photographic ? clauseById(IMAGE_LENSES, lensId) : '',
    photographic ? clauseById(IMAGE_APERTURES, apertureId) : '',
  ].filter((part) => part.trim() !== '');
  return parts.join(', ');
}

/**
 * The negative prompt sent: the person's own wording first, then what each chosen preset asks to
 * avoid, with a term that two presets both dislike listed once.
 */
export function composeNegativePrompt(own: string, presetIds: readonly string[]): string {
  const terms = [own, ...selectedPresets(presetIds).map((preset) => preset.negative)]
    .join(',')
    .split(',')
    .map((term) => term.trim())
    .filter((term) => term !== '');
  return [...new Set(terms)].join(', ');
}
