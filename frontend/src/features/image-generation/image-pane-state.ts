import type {
  GeneratedImageSource,
  ImageBatch,
  ImageGenerationProgress,
  ImagePrompt,
  ImageQualityId,
  Location,
  PaneId,
  TabId,
} from '../../models';
import {
  IMAGE_COUNTS,
  IMAGE_MAX_SEED,
  IMAGE_MAX_STEPS,
  IMAGE_SIZES,
  IMAGE_STEP_CHOICES,
} from '../../models';

/** One stored image as the preview pane shows it. */
export interface GeneratedImageView {
  readonly imageId: string;
  /** Object URL of the fetched preview bytes; `undefined` until they arrive. */
  previewUrl?: string | undefined;
  /** Whether its bytes are still being fetched. */
  loading: boolean;
  /** Where it was saved, once it has been. */
  savedLocation?: string | undefined;
}

/** One generation this pane has asked for, kept so the crumb bar can go back to it. */
export interface ImageGenerationRecord {
  readonly generationId: string;
  /** What the person typed for it, before presets and lens wording were added; names the crumb. */
  readonly subject?: string | undefined;
  /** What the endpoint returned: its prompt, size and images. */
  readonly batch: ImageBatch;
  /** How each of its images is displayed; held so revisiting costs no fetch. */
  views: GeneratedImageView[];
  /** Which of its images was shown large. */
  selectedImageId?: string | undefined;
}

/** How many previous generations the crumb bar keeps before the oldest falls off. */
export const MAX_GENERATION_CRUMBS = 12;

/** An image tab (task 0325): a prompt at the bottom, the resulting pictures above it. */
export interface ImagePaneEntry {
  key: string;
  paneId: PaneId;
  tabId: TabId;
  readonly workspaceId: string;
  /** What the person wants to see. */
  subject: string;
  /** What the person wants avoided, on top of what the preset already excludes. */
  negativePrompt: string;
  /** Chosen style preset ids; at most one medium, plus any treatment. */
  presetIds: string[];
  /** Chosen lens and aperture ids; only used by photographic presets. */
  lensId: string;
  apertureId: string;
  width: number;
  height: number;
  count: number;
  /** Reproducibility seed; `undefined` lets the provider choose. */
  seed?: number | undefined;
  /** How hard the endpoint should work; `''` leaves the choice to it. */
  qualityId: ImageQualityId | '';
  /** Sampler step count; `undefined` sends none, since not every endpoint has a sampler. */
  steps?: number | undefined;
  /** A picture on disk this one starts from; the backend reads its bytes. */
  source?: GeneratedImageSource | undefined;
  /** The generation on screen, and how each of its images is displayed. */
  batch?: ImageBatch | undefined;
  views: GeneratedImageView[];
  /** Every prompt ever generated from, with its settings, most recently used first. */
  prompts: ImagePrompt[];
  /** The generations this pane has asked for, oldest first; the crumb bar walks them. */
  history: ImageGenerationRecord[];
  /** Which of them the preview shows. */
  activeGenerationId?: string | undefined;
  /** Which stored image the preview pane shows large. */
  selectedImageId?: string | undefined;
  /** Identity of the running request, so Stop can reach it. */
  pendingRequestId?: string | undefined;
  /** How far the running generation has got, as its endpoint reported it; nothing said is `undefined`. */
  progress?: ImageGenerationProgress | undefined;
  /** Why the last generation was refused, as a rejection reason. */
  failure?: ImageFailureKey | undefined;
  /** Anything else that went wrong, already worded for display. */
  error?: string | undefined;
  /** Image whose save is in flight. */
  savingImageId?: string | undefined;
}

/** A freshly opened pane: sensible defaults, nothing generated. */
export function emptyImagePane(
  key: string,
  paneId: PaneId,
  tabId: TabId,
  workspaceId: string,
): ImagePaneEntry {
  return {
    key,
    paneId,
    tabId,
    workspaceId,
    subject: '',
    negativePrompt: '',
    presetIds: [],
    lensId: '',
    apertureId: '',
    width: IMAGE_SIZES[0]?.width ?? 1_024,
    height: IMAGE_SIZES[0]?.height ?? 1_024,
    count: 1,
    qualityId: '',
    views: [],
    prompts: [],
    history: [],
  };
}

/** The step counts worth choosing, beside "any", which sends none at all. */
export const IMAGE_STEP_OPTIONS: readonly (number | '')[] = ['', ...IMAGE_STEP_CHOICES];

/** The name of the picture this pane starts from, or nothing when it does not start from one. */
export function sourceNameOf(source: GeneratedImageSource | null | undefined): string | undefined {
  if (source === undefined || source === null) return undefined;
  const uri = source.location.uri.replace(/\/+$/, '');
  const tail = uri.slice(uri.lastIndexOf('/') + 1);
  return decodeURIComponent(tail) || uri;
}

/** The bounds the backend enforces on a size: each edge a multiple of 8 in range, and a pixel cap. */
export const IMAGE_SIZE_LIMITS = {
  minEdge: 64,
  maxEdge: 2_048,
  step: 8,
  maxPixels: 2_097_152,
} as const;

/** Whether a width and height are inside what the backend accepts; the person may type any pair. */
export function sizeAllowed(width: number, height: number): boolean {
  const { minEdge, maxEdge, step, maxPixels } = IMAGE_SIZE_LIMITS;
  return (
    [width, height].every(
      (edge) => Number.isInteger(edge) && edge >= minEdge && edge <= maxEdge && edge % step === 0,
    ) && width * height <= maxPixels
  );
}

/** Why a request cannot be sent, as an i18n key; `undefined` when it can. */
export type ImageRequestProblem =
  | 'needSubject'
  | 'promptTooLong'
  | 'negativeTooLong'
  | 'sizeNotAllowed'
  | 'countNotAllowed'
  | 'seedTooLarge'
  | 'stepsOutOfRange';

/** The bounds the backend enforces; checking them here saves a round trip and says why. */
export const IMAGE_LIMITS = {
  promptChars: 4_000,
  negativePromptChars: 2_000,
  maxSeed: IMAGE_MAX_SEED,
  maxSteps: IMAGE_MAX_STEPS,
} as const;

/**
 * Whether the person can press Generate now: a subject, nothing already running, and a request inside
 * the bounds the backend would refuse anyway.
 */
export function imageRequestProblem(entry: ImagePaneEntry): ImageRequestProblem | undefined {
  if (entry.pendingRequestId !== undefined) return undefined;
  const subject = entry.subject.trim();
  if (subject === '') return 'needSubject';
  if (subject.length > IMAGE_LIMITS.promptChars) return 'promptTooLong';
  if (entry.negativePrompt.trim().length > IMAGE_LIMITS.negativePromptChars) {
    return 'negativeTooLong';
  }
  if (!sizeAllowed(entry.width, entry.height)) return 'sizeNotAllowed';
  if (!IMAGE_COUNTS.includes(entry.count)) return 'countNotAllowed';
  if (entry.seed !== undefined && (entry.seed < 0 || entry.seed > IMAGE_LIMITS.maxSeed)) {
    return 'seedTooLarge';
  }
  if (entry.steps !== undefined && (entry.steps < 1 || entry.steps > IMAGE_LIMITS.maxSteps)) {
    return 'stepsOutOfRange';
  }
  return undefined;
}

/** Whether Generate may be pressed. */
export function canGenerate(entry: ImagePaneEntry): boolean {
  return entry.pendingRequestId === undefined && imageRequestProblem(entry) === undefined;
}

/** A readable file stem for a prompt: lower case, dashes, at most 40 characters. */
function fileStem(prompt: string): string {
  const stem = prompt
    .toLowerCase()
    .replace(/[^a-z0-9]+/gu, '-')
    .replace(/^-+|-+$/gu, '')
    .slice(0, 40)
    .replace(/-+$/gu, '');
  return stem === '' ? 'image' : stem;
}

/**
 * A file name for the Nth image of a batch: a readable stem, the size, and the format. When `taken`
 * names files already in the folder (compared without regard to case), the index counts up until
 * the name is free, so the suggestion never points at a file that is there.
 */
export function suggestedFileName(
  prompt: string,
  index: number,
  width: number,
  height: number,
  extension: string,
  taken?: ReadonlySet<string>,
): string {
  const base = `${fileStem(prompt)}-${width}x${height}`;
  let next = index + 1;
  while (taken?.has(`${base}-${next}.${extension}`.toLowerCase()) === true && next < index + 1000) {
    next += 1;
  }
  return `${base}-${next}.${extension}`;
}

/** The names in a listing, folded for a case-insensitive "is it taken" test. */
export function takenNames(names: Iterable<string>): ReadonlySet<string> {
  return new Set([...names].map((name) => name.toLowerCase()));
}

/** A folder as the person reads it: a plain path, with the home directory written as `~`. */
export function folderDisplayPath(uri: string, home?: string): string {
  let path = uri;
  if (uri.startsWith('file://')) {
    try {
      path = decodeURIComponent(new URL(uri).pathname);
    } catch {
      path = uri.slice('file://'.length);
    }
    // `file:///C:/Users` is the drive `C:`, not a folder named `C:` under the root.
    if (/^\/[A-Za-z]:/u.test(path)) path = path.slice(1);
  }
  const trimmed = path.length > 1 ? path.replace(/\/+$/u, '') : path;
  const base = home?.replace(/\/+$/u, '');
  if (base !== undefined && base !== '') {
    if (trimmed === base) return '~';
    if (trimmed.startsWith(`${base}/`)) return `~${trimmed.slice(base.length)}`;
  }
  return trimmed;
}

/** The location a save would write to. */
export function destinationFor(folder: Location, fileName: string): Location {
  const separator = folder.uri.endsWith('/') ? '' : '/';
  return { providerId: folder.providerId, uri: `${folder.uri}${separator}${fileName.trim()}` };
}

/** Whether a save failed because the file is already there: the person must agree to replace it. */
/**
 * Which sentence explains a failure. The pane holds this key rather than the backend's reason, so
 * the decisions below - "go to Settings", "offer to replace" - compare one thing and one only.
 */
export type ImageFailureKey =
  | 'roleNotAssigned'
  | 'cloudImageNotAllowed'
  | 'consentRequired'
  | 'endpointInvalid'
  | 'hostNotAllowed'
  | 'timedOut'
  | 'invalidRequest'
  | 'notFound'
  | 'batchExpired'
  | 'imageTooLarge'
  | 'sourceUnsupported'
  | 'sourceUnusable'
  | 'sourceTooLarge'
  | 'destinationExists'
  | 'destinationInvalid'
  | 'cancelled'
  | 'generationFailed'
  | 'failed';

/**
 * Whether the failure is the save refusing a destination that is already taken: the pane then
 * offers to replace it instead of only complaining.
 */
export function isSaveConflict(failure: string | undefined): boolean {
  return failure === 'destinationExists';
}

/** Whether a failure is fixed in Settings rather than by the person retrying. */
export function failureNeedsSettings(failure: string | undefined): boolean {
  return (
    failure === 'roleNotAssigned' ||
    failure === 'cloudImageNotAllowed' ||
    failure === 'consentRequired' ||
    failure === 'endpointInvalid' ||
    // A model that ran out of time is usually a timeout the person can raise in Settings.
    failure === 'timedOut'
  );
}

/** Whether the preview pane has anything to show. */
export function hasPreview(entry: ImagePaneEntry): boolean {
  return entry.batch !== undefined && entry.batch.images.length > 0;
}

/**
 * Where a new image pane opens (the task 0321 rule, as chat uses it): beside the editor when the
 * active pane shows one, in the active pane otherwise.
 */
export function imagePlacementPaneId(
  paneOrder: readonly PaneId[],
  activePaneId: PaneId,
  activePaneShowsEditor: boolean,
): PaneId {
  if (!activePaneShowsEditor) return activePaneId;
  return paneOrder.find((candidate) => candidate !== activePaneId) ?? activePaneId;
}

/** The pane whose folder a save defaults to: the other pane, when there is one. */
export function imageSaveFolderPaneId(
  paneOrder: readonly PaneId[],
  imagePaneId: PaneId,
): PaneId | undefined {
  return paneOrder.find((candidate) => candidate !== imagePaneId);
}

/** The generation the preview shows, when the pane is looking at one. */
export function activeGeneration(entry: ImagePaneEntry): ImageGenerationRecord | undefined {
  return entry.activeGenerationId === undefined
    ? undefined
    : entry.history.find((record) => record.generationId === entry.activeGenerationId);
}

/**
 * The generation bar after remembering one: a generation appears once however often it is
 * revisited, and the oldest falls off once the bar is full.
 */
export function rememberGeneration(
  history: readonly ImageGenerationRecord[],
  record: ImageGenerationRecord,
): ImageGenerationRecord[] {
  const kept = history.filter((item) => item.generationId !== record.generationId);
  return [...kept, record].slice(-MAX_GENERATION_CRUMBS);
}

/** The generation bar without one of its generations, once it has been discarded. */
export function forgetGeneration(
  history: readonly ImageGenerationRecord[],
  generationId: string,
): ImageGenerationRecord[] {
  return history.filter((record) => record.generationId !== generationId);
}

/** How a generation is named in the crumb bar: the start of its prompt, on one line. */
export function crumbLabel(prompt: string): string {
  const collapsed = prompt.replace(/\s+/gu, ' ').trim();
  if (collapsed.length <= CRUMB_MAX) return collapsed;
  const cut = collapsed.slice(0, CRUMB_MAX + 1);
  const space = cut.lastIndexOf(' ');
  return `${space > CRUMB_MAX / 2 ? cut.slice(0, space) : cut}…`;
}

const CRUMB_MAX = 40;

/** Words a short title should not end on: they promise a noun that was cut off. */
const DANGLING_WORDS = new Set([
  'a',
  'an',
  'the',
  'in',
  'on',
  'at',
  'of',
  'to',
  'with',
  'and',
  'or',
  'for',
  'by',
  'from',
  'into',
  'near',
]);

/** The most words a short title keeps. */
const TITLE_WORDS = 5;

/**
 * A short name for a prompt: its first clause, at most five words, never ending on a word like
 * "in" or "the". The whole prompt stays one hover away, so the name only has to say which is which.
 */
export function shortTitle(prompt: string): string {
  const clause =
    prompt
      .replace(/\s+/gu, ' ')
      .split(/[.,;:!?]/u)
      .map((part) => part.trim())
      .find((part) => part !== '') ?? '';
  const words = clause.split(' ').filter((word) => word !== '');
  let kept = words.slice(0, TITLE_WORDS);
  while (kept.length > 1 && DANGLING_WORDS.has((kept.at(-1) ?? '').toLowerCase()))
    kept = kept.slice(0, -1);
  const title = kept.join(' ');
  return title.length > CRUMB_MAX ? `${title.slice(0, CRUMB_MAX - 1).trimEnd()}…` : title;
}

/**
 * Short titles for a list of prompts, each one a new name: when two prompts get the same title the
 * later one is numbered - `A beautiful woman`, `A beautiful woman (2)` - so no two entries read the same.
 */
export function distinctTitles(prompts: readonly string[]): string[] {
  const seen = new Map<string, number>();
  return prompts.map((prompt) => {
    const title = shortTitle(prompt);
    const count = (seen.get(title) ?? 0) + 1;
    seen.set(title, count);
    return count === 1 || title === '' ? title : `${title} (${count})`;
  });
}

/** The image of a batch the preview should select: the first, or the one still held. */
export function firstImageId(batch: ImageBatch | undefined): string | undefined {
  return batch?.images[0]?.id;
}

/** The changes that put a remembered prompt, with every setting it was used with, back in the form. */
export function promptPatch(prompt: ImagePrompt): Partial<ImagePaneEntry> {
  return {
    subject: prompt.subject,
    negativePrompt: prompt.negativePrompt,
    presetIds: [...prompt.presetIds],
    lensId: prompt.lensId,
    apertureId: prompt.apertureId,
    width: prompt.width,
    height: prompt.height,
    count: prompt.count,
    seed: prompt.seed ?? undefined,
    qualityId: prompt.quality ?? '',
    steps: prompt.steps ?? undefined,
    failure: undefined,
    error: undefined,
  };
}

/** The prompt a generation is remembered as: the form as it was when Generate was pressed. */
export function promptOf(entry: ImagePaneEntry): ImagePrompt {
  return {
    id: crypto.randomUUID(),
    title: '',
    updatedAt: '',
    subject: entry.subject.trim(),
    negativePrompt: entry.negativePrompt.trim(),
    presetIds: [...entry.presetIds],
    lensId: entry.lensId,
    apertureId: entry.apertureId,
    width: entry.width,
    height: entry.height,
    count: entry.count,
    seed: entry.seed ?? null,
    quality: entry.qualityId === '' ? null : entry.qualityId,
    steps: entry.steps ?? null,
  };
}
