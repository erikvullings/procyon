/**
 * The mock host's image generation (task 0325).
 *
 * The mock never calls an endpoint: it answers a bounded prompt with solid-colour PNGs so both panes
 * can be exercised, keeps the bytes the way the backend's short-lived store does, and applies the
 * same rules the backend does - consent for a cloud image role, bounded requests, cancellation, and
 * no write to a destination that already exists.
 */

import type {
  GeneratedImageSource,
  ImageBatch,
  ImageQualityId,
  Location,
  SaveGeneratedImageResponse,
} from '../../models';
import { IMAGE_COUNTS, IMAGE_MAX_SEED, isValidImageFileName } from '../../models';

/** The quality levels a person may ask for, named as they are named in the pane. */
const QUALITIES: readonly ImageQualityId[] = ['fast', 'good', 'quality', 'superQuality'];

/** The picture formats both hosts accept as a starting point: what the backend can decode and send. */
const SOURCE_EXTENSIONS: readonly string[] = ['png', 'jpg', 'jpeg', 'webp'];

/** Why a picture cannot be used to start from, in the two ways it can fail. */
export function mockSourceProblem(
  source: GeneratedImageSource | null | undefined,
): string | undefined {
  if (source === undefined || source === null) return undefined;
  const uri = source.location.uri;
  const name = uri.slice(uri.lastIndexOf('/') + 1);
  const extension = name.includes('.')
    ? (name.slice(name.lastIndexOf('.') + 1).toLowerCase() ?? '')
    : '';
  return SOURCE_EXTENSIONS.includes(extension)
    ? undefined
    : 'that file cannot be used as a starting picture';
}

/** One 64×64 solid-colour PNG per slot in a batch. */
const SWATCHES: readonly string[] = [
  'iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAT0lEQVR42u3PQQkAAAgEsEtjHesY3wi+hcEKLNXzWgQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQELgsBdtDThNEICwAAAABJRU5ErkJggg==',
  'iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAT0lEQVR42u3PQQkAAAgEsItmFCMa0Qi+hcEKLNP1WgQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQELgvhvAE8CutMDQAAAABJRU5ErkJggg==',
  'iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAT0lEQVR42u3PQQkAAAgEsItnDMMY3Ai+hcEKLDX9WgQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQELgsy38EPrV6xswAAAABJRU5ErkJggg==',
  'iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAT0lEQVR42u3PQQkAAAgEsEtsBuMY0wi+hcEKLF3zWgQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQELguQw2F4tl20HAAAAABJRU5ErkJggg==',
] as const;

/** One stored image: the bytes a host is served, and what the list DTO said about it. */
export interface MockImage {
  bytes: Uint8Array;
  mediaType: string;
  extension: string;
  width: number;
  height: number;
  downscaled: boolean;
}

/** One held batch, mirroring the backend store's shape. */
export interface MockImageBatch {
  model: string;
  prompt: string;
  negativePrompt: string | null;
  seed: number | null;
  width: number;
  height: number;
  quality: ImageQualityId | null;
  steps: number | null;
  source: GeneratedImageSource | null;
  images: Map<string, MockImage>;
  createdAt: number;
}

/** How long a mock batch is held, matching the backend's short-lived store. */
const BATCH_TTL_MS = 30 * 60 * 1_000;

/** The bounds the backend enforces; the mock refuses the same requests so the pane is tested fairly. */
export const MOCK_IMAGE_BOUNDS = {
  promptChars: 4_000,
  negativePromptChars: 2_000,
  minEdge: 64,
  maxEdge: 2_048,
  maxPixels: 2_097_152,
  maxSteps: 64,
} as const;

/**
 * A request the mock could not honour. It carries the rejection reason the backend would report
 * under both names the hosts use, so the pane reads the same reason from either host.
 */
export class MockImageError extends Error {
  readonly code: string;

  constructor(
    readonly reason: string,
    message: string,
  ) {
    super(message);
    this.name = 'MockImageError';
    this.code = reason;
  }
}

/** Whether a prompt and size fall inside the bounds both hosts enforce. */
export function mockImageRequestProblem(request: {
  prompt: string;
  negativePrompt?: string | null;
  width: number;
  height: number;
  count: number;
  seed?: number | null;
  quality?: ImageQualityId | null;
  steps?: number | null;
}): string | undefined {
  if (request.prompt.trim().length === 0) return 'the prompt is empty';
  if (request.prompt.trim().length > MOCK_IMAGE_BOUNDS.promptChars) {
    return 'the prompt is too long';
  }
  const negative = request.negativePrompt?.trim() ?? '';
  if (negative.length > MOCK_IMAGE_BOUNDS.negativePromptChars) {
    return 'the negative prompt is too long';
  }
  for (const edge of [request.width, request.height]) {
    if (edge < MOCK_IMAGE_BOUNDS.minEdge || edge > MOCK_IMAGE_BOUNDS.maxEdge || edge % 8 !== 0) {
      return 'the size is outside the accepted range';
    }
  }
  if (request.width * request.height > MOCK_IMAGE_BOUNDS.maxPixels) {
    return 'the size asks for too many pixels';
  }
  if (!IMAGE_COUNTS.includes(request.count)) {
    return 'the number of images is outside the accepted range';
  }
  if (request.seed != null && (request.seed < 0 || request.seed > IMAGE_MAX_SEED)) {
    return 'the seed is too large';
  }
  if (request.quality != null && !QUALITIES.includes(request.quality)) {
    return 'the quality level is not one of the accepted levels';
  }
  if (request.steps != null && (request.steps < 1 || request.steps > MOCK_IMAGE_BOUNDS.maxSteps)) {
    return 'the step count is outside the accepted range';
  }
  return undefined;
}

/** Ids the mock hands out; deterministic enough for a mock, unique enough for a session. */
let sequence = 0;

function nextId(): string {
  sequence += 1;
  return `mock-image-${sequence.toString(16)}`;
}

/** Decodes a base64 PNG fixture; the mock's "provider output". */
function swatch(index: number): Uint8Array {
  const encoded = SWATCHES[index % SWATCHES.length] ?? '';
  const binary = atob(encoded);
  const bytes = new Uint8Array(binary.length);
  for (let position = 0; position < binary.length; position += 1) {
    bytes[position] = binary.charCodeAt(position);
  }
  return bytes;
}

/** Builds a batch the way the backend would: one image per requested image, in request order. */
export function mockImageBatch(
  model: string,
  request: {
    prompt: string;
    negativePrompt?: string | null;
    width: number;
    height: number;
    count: number;
    seed?: number | null;
    quality?: ImageQualityId | null;
    steps?: number | null;
    source?: GeneratedImageSource | null;
  },
): { generationId: string; batch: MockImageBatch; dto: ImageBatch } {
  const generationId = nextId();
  const images = new Map<string, MockImage>();
  const listed = [];
  for (let index = 0; index < request.count; index += 1) {
    const id = nextId();
    images.set(id, {
      bytes: swatch(index),
      mediaType: 'image/png',
      extension: 'png',
      width: request.width,
      height: request.height,
      // Anything beyond the preview bound is reported as downscaled, as the backend reports it.
      downscaled: Math.max(request.width, request.height) > 1_024,
    });
    listed.push({
      id,
      mediaType: 'image/png',
      width: request.width,
      height: request.height,
      bytes: 136,
      downscaled: Math.max(request.width, request.height) > 1_024,
      extension: 'png',
    });
  }
  return {
    generationId,
    batch: {
      model,
      prompt: request.prompt.trim(),
      negativePrompt: request.negativePrompt?.trim() || null,
      seed: request.seed ?? null,
      width: request.width,
      height: request.height,
      quality: request.quality ?? null,
      steps: request.steps ?? null,
      source: request.source ?? null,
      images,
      createdAt: Date.now(),
    },
    dto: {
      generationId,
      model,
      prompt: request.prompt.trim(),
      negativePrompt: request.negativePrompt?.trim() || null,
      seed: request.seed ?? null,
      width: request.width,
      height: request.height,
      quality: request.quality ?? null,
      steps: request.steps ?? null,
      source: request.source ?? null,
      images: listed,
    },
  };
}

/** Whether a held batch is still inside the store's lifetime. */
export function batchIsLive(batch: MockImageBatch): boolean {
  return Date.now() - batch.createdAt < BATCH_TTL_MS;
}

/**
 * What a save of `fileName` into `folderUri` would do. The mock has no disk, so a save is recorded
 * in memory; the no-overwrite rule the backend enforces is enforced here too.
 */
export function mockImageSave(
  batches: ReadonlyMap<string, MockImageBatch>,
  saved: ReadonlyMap<string, { bytes: number; promptEmbedded: boolean }>,
  input: {
    generationId: string;
    imageId: string;
    destination: Location;
    overwrite: boolean;
  },
): SaveGeneratedImageResponse {
  const batch = batches.get(input.generationId);
  if (batch === undefined || !batchIsLive(batch)) {
    throw new MockImageError('imageBatchExpired', 'these images are no longer held');
  }
  const image = batch.images.get(input.imageId);
  if (image === undefined) {
    throw new MockImageError('notFound', 'no such generated image');
  }
  const fileName = input.destination.uri.slice(input.destination.uri.lastIndexOf('/') + 1);
  if (!isValidImageFileName(fileName, image.extension)) {
    throw new MockImageError(
      'invalidRequest',
      'the file name is not allowed or does not match the image format',
    );
  }
  if (saved.has(input.destination.uri) && !input.overwrite) {
    throw new MockImageError('destinationAlreadyExists', 'the file already exists');
  }
  return {
    location: { providerId: input.destination.providerId, uri: input.destination.uri },
    bytesWritten: image.bytes.byteLength,
    // The mock's PNG fixtures carry no chunks; the real backend reports true for PNG.
    promptEmbedded: false,
  };
}
