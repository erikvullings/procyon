import type { ImageBatch, Location, PaneId, TabId } from '../../models';
import { IMAGE_COUNTS, IMAGE_MAX_SEED, IMAGE_SIZES } from '../../models';

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

/** An image tab (task 0325): a prompt on one side, the resulting pictures on the other. */
export interface ImagePaneEntry {
  key: string;
  paneId: PaneId;
  tabId: TabId;
  readonly workspaceId: string;
  /** What the person wants to see. */
  subject: string;
  /** What the person wants avoided, on top of what the preset already excludes. */
  negativePrompt: string;
  /** Chosen style preset id, or `NO_PRESET`. */
  presetId: string;
  /** Chosen lens and aperture ids; only used by photographic presets. */
  lensId: string;
  apertureId: string;
  width: number;
  height: number;
  count: number;
  /** Reproducibility seed; `undefined` lets the provider choose. */
  seed?: number | undefined;
  /** The batch on screen, and how each of its images is displayed. */
  batch?: ImageBatch | undefined;
  views: GeneratedImageView[];
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
    presetId: '',
    lensId: '',
    apertureId: '',
    width: IMAGE_SIZES[0]?.width ?? 1_024,
    height: IMAGE_SIZES[0]?.height ?? 1_024,
    count: 1,
    views: [],
  };
}

/** Why a request cannot be sent, as an i18n key; `undefined` when it can. */
export type ImageRequestProblem =
  | 'needSubject'
  | 'promptTooLong'
  | 'negativeTooLong'
  | 'sizeNotAllowed'
  | 'countNotAllowed'
  | 'seedTooLarge';

/** The bounds the backend enforces; checking them here saves a round trip and says why. */
export const IMAGE_LIMITS = {
  promptChars: 4_000,
  negativePromptChars: 2_000,
  maxSeed: IMAGE_MAX_SEED,
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

/** The image of a batch the preview should select: the first, or the one still held. */
export function firstImageId(batch: ImageBatch | undefined): string | undefined {
  return batch?.images[0]?.id;
}
