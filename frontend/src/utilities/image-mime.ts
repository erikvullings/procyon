const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47] as const;

/**
 * The MIME type of backend-generated thumbnail bytes. The backend encodes photos as JPEG but icon
 * (`.ico`) thumbnails as PNG so their transparency survives; the transport carries only the bytes,
 * so the type is read from the PNG signature.
 */
export function thumbnailMimeType(bytes: Uint8Array): 'image/png' | 'image/jpeg' {
  return PNG_SIGNATURE.every((value, index) => bytes[index] === value) ? 'image/png' : 'image/jpeg';
}
