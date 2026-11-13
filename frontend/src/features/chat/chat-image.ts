/** Pasted images for the chat (task 0329). The backend refuses anything larger than the edge limit. */

/** Longest side of a pasted image after resizing, in pixels. */
export const CHAT_IMAGE_MAX_EDGE = 1024;

/** Most images sent with one message. */
export const CHAT_IMAGE_MAX_COUNT = 4;

/** An image ready to send: `data` is base64 JPEG without a prefix; `previewUrl` is its data URL. */
export interface PastedImage {
  readonly id: string;
  readonly data: string;
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
    const previewUrl = canvas.toDataURL('image/jpeg', 0.9);
    return { id, data: previewUrl.slice(previewUrl.indexOf(',') + 1), previewUrl };
  } finally {
    bitmap.close();
  }
}
