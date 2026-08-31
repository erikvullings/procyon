/** Copies text to the system clipboard, with the same `execCommand` fallback used elsewhere in
 * the app (see `frontend/src/features/diagnostics/diagnostics-view.ts`'s `copyDiagnosticsToClipboard`). */
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement('textarea');
  textarea.value = text;
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand('copy');
  document.body.removeChild(textarea);
}

/** Copies an image `data:` URI (as produced by `bytesToDataUri`) to the system clipboard as image
 * bytes, decoding the base64 payload directly rather than round-tripping through `fetch` (which
 * jsdom's test environment does not support for `data:` URIs). */
export async function copyImageDataUri(dataUri: string): Promise<void> {
  const match = /^data:([^;]+);base64,(.*)$/.exec(dataUri);
  const mimeType = match?.[1];
  const base64 = match?.[2];
  if (mimeType === undefined || base64 === undefined) {
    throw new Error(t('clipboard', 'unsupportedImage'));
  }
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  if (!navigator.clipboard?.write) {
    throw new Error(t('clipboard', 'imageCopyUnsupported'));
  }
  await navigator.clipboard.write([
    new ClipboardItem({ [mimeType]: new Blob([bytes], { type: mimeType }) }),
  ]);
}

import { t } from '../../i18n';
