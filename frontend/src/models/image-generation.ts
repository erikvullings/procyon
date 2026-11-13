import type {
  CancelImageGenerationRequestDto,
  DiscardImageBatchRequestDto,
  GeneratedImageDto,
  GenerateImagesRequestDto,
  ImageBatchDto,
  SaveGeneratedImageRequestDto,
  SaveGeneratedImageResponseDto,
} from '../api/generated/models';

/**
 * Image generation (task 0325): a bounded prompt, a batch the person reviews, and an explicit save.
 * Nothing reaches disk until a save is asked for.
 */
export type {
  CancelImageGenerationRequestDto as CancelImageGenerationRequest,
  DiscardImageBatchRequestDto as DiscardImageBatchRequest,
  GeneratedImageDto as GeneratedImage,
  GenerateImagesRequestDto as GenerateImagesRequest,
  ImageBatchDto as ImageBatch,
  SaveGeneratedImageRequestDto as SaveGeneratedImageRequest,
  SaveGeneratedImageResponseDto as SaveGeneratedImageResponse,
};

/**
 * The bytes of one stored image, fetched by id. An image never travels as JSON, so each host serves
 * these bytes its own way: a route in the browser, a command payload in the desktop app.
 */

/** How the desktop host's IPC command carries the bytes the HTTP host serves from a route. */
export interface GeneratedImageBytesDto {
  bytes: number[];
  mediaType: string;
}

export interface GeneratedImageBytes {
  /** The preview downscale, or the stored original when that was asked for. */
  bytes: Uint8Array;
  /** Their media type, e.g. `image/png`. */
  mediaType: string;
}

/** How a size is described to the person. */
export type ImageSizeLabel = 'square' | 'landscape' | 'portrait' | 'wide' | 'tall';

/** Widths, heights and counts the pane offers; the backend refuses anything outside them. */
export const IMAGE_SIZES: readonly { width: number; height: number; labelKey: ImageSizeLabel }[] = [
  { width: 1_024, height: 1_024, labelKey: 'square' },
  { width: 1_216, height: 832, labelKey: 'landscape' },
  { width: 832, height: 1_216, labelKey: 'portrait' },
  { width: 1_344, height: 768, labelKey: 'wide' },
  { width: 768, height: 1_344, labelKey: 'tall' },
] as const;

/** How many images one batch may ask for. */
export const IMAGE_COUNTS: readonly number[] = [1, 2, 3, 4];

/** Characters no file system in the app's scope accepts in a single name. */
const FORBIDDEN_NAME_CHARACTERS = /[\\/:*?"<>|]/u;

/** The largest seed the backend accepts. */
export const IMAGE_MAX_SEED = 4_294_967_295;

/**
 * Whether a file name may be used for a saved image. The backend checks its own rules too; the pane
 * checks so the person is told before a request round-trips. Surrounding blanks are refused rather
 * than quietly trimmed, because the file that would be written is not the name that was typed.
 */
export function isValidImageFileName(name: string, extension: string): boolean {
  if (name === '' || name !== name.trim()) return false;
  if (name.length > 128) return false;
  if (name === '.' || name === '..') return false;
  if (FORBIDDEN_NAME_CHARACTERS.test(name)) return false;
  // Control characters are refused without spelling them into a pattern.
  for (const character of name) {
    if ((character.codePointAt(0) ?? 0) < 0x20) return false;
  }
  return name.toLowerCase().endsWith(`.${extension.toLowerCase()}`);
}
