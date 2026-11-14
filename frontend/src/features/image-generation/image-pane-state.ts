import type {
  GeneratedImageSource,
  ImageBatch,
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
  /** The generations this pane has asked for, oldest first; the crumb bar walks them. */
  history: ImageGenerationRecord[];
  /** Which of them the preview shows. */
  activeGenerationId?: string | undefined;
  /** Which stored image the preview pane shows large. */
  selectedImageId?: string | undefined;
  /** Identity of the running request, so Stop can reach it. */
  pendingRequestId?: string | undefined;
  /** Why the last generation was refused, as a rejection reason. */
  failure?: string | undefined;
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
  const known = IMAGE_SIZES.some(
    (size) => size.width === entry.width && size.height === entry.height,
  );
  if (!known) return 'sizeNotAllowed';
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

/** A file name for the Nth image of a batch: a readable stem, the size, and the format. */
export function suggestedFileName(
  prompt: string,
  index: number,
  width: number,
  height: number,
  extension: string,
): string {
  const stem = prompt
    .toLowerCase()
    .replace(/[^a-z0-9]+/gu, '-')
    .replace(/^-+|-+$/gu, '')
    .slice(0, 40)
    .replace(/-+$/gu, '');
  const base = stem === '' ? 'image' : stem;
  return `${base}-${width}x${height}-${index + 1}.${extension}`;
}

/** The location a save would write to. */
export function destinationFor(folder: Location, fileName: string): Location {
  const separator = folder.uri.endsWith('/') ? '' : '/';
  return { providerId: folder.providerId, uri: `${folder.uri}${separator}${fileName.trim()}` };
}

/** Whether a save failed because the file is already there: the person must agree to replace it. */
export function isSaveConflict(failure: string | undefined): boolean {
  return failure === 'destinationAlreadyExists';
}

/** Whether a failure is fixed in Settings rather than by the person retrying. */
export function failureNeedsSettings(failure: string | undefined): boolean {
  return failure === 'roleNotAssigned' || failure === 'cloudImageNotAllowed';
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

/** The image of a batch the preview should select: the first, or the one still held. */
export function firstImageId(batch: ImageBatch | undefined): string | undefined {
  return batch?.images[0]?.id;
}
