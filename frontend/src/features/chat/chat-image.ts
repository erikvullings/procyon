/** Pasted images for the chat (task 0329). The backend refuses anything larger than the edge limit. */

/** Longest side of a pasted image after resizing, in pixels. */
export const CHAT_IMAGE_MAX_EDGE = 1024;

/** Most images sent with one message. */
export const CHAT_IMAGE_MAX_COUNT = 4;
export const CHAT_IMAGE_MAX_BYTES = 4 * 1024 * 1024;

/** Read file items synchronously while the clipboard data is available. */
export function pastedImageFiles(
  items: ArrayLike<Pick<DataTransferItem, 'kind' | 'type' | 'getAsFile'>>,
): File[] {
  return Array.from(items).flatMap((item) => {
    if (item.kind !== 'file' || !item.type.startsWith('image/')) return [];
    const file = item.getAsFile();
    return file === null ? [] : [file];
  });
}

/** An image ready to send: `data` is base64 JPEG without a prefix; `previewUrl` is its data URL. */
export interface PastedImage {
  readonly id: string;
  readonly data: string;
  readonly mediaType: 'image/jpeg';
  readonly previewUrl: string;
}

/** Scales a size so its longest side is at most `maxEdge`; smaller images keep their size. */
export function fitWithin(
  width: number,
  height: number,
  maxEdge: number = CHAT_IMAGE_MAX_EDGE,
): { width: number; height: number } {
  const longest = Math.max(width, height);
  if (longest <= maxEdge) return { width, height };
  const scale = maxEdge / longest;
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

/** Decodes a pasted image, shrinks it to fit the edge limit and encodes it as JPEG. Browser only. */
export async function encodePastedImage(blob: Blob, id: string): Promise<PastedImage> {
  if (!['image/png', 'image/jpeg', 'image/webp'].includes(blob.type)) {
    throw new Error('unsupported image format');
  }
  const bitmap = await createImageBitmap(blob);
  try {
    const size = fitWithin(bitmap.width, bitmap.height);
    const canvas = document.createElement('canvas');
    canvas.width = size.width;
    canvas.height = size.height;
    const context = canvas.getContext('2d');
    if (context === null) throw new Error('canvas is unavailable');
    // JPEG has no transparency: paint white first so transparent pixels do not turn black.
    context.fillStyle = 'white';
    context.fillRect(0, 0, size.width, size.height);
    context.drawImage(bitmap, 0, 0, size.width, size.height);
    const mediaType = 'image/jpeg';
    const previewUrl = canvas.toDataURL(mediaType, 0.9);
    const prefix = `data:${mediaType};base64,`;
    if (!previewUrl.startsWith(prefix)) throw new Error('canvas did not encode a JPEG');
    return { id, data: previewUrl.slice(prefix.length), mediaType, previewUrl };
  } finally {
    bitmap.close();
  }
}
