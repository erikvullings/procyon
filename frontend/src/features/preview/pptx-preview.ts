import type { PptxPreviewResource, PptxPreviewResourceDescriptor } from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { bytesToDataUri } from './content-preview';
import { sanitizeEpubSvg } from './epub-preview';

const SAFE_IMAGE_MEDIA_TYPES = new Set([
  'image/bmp',
  'image/gif',
  'image/jpeg',
  'image/png',
  'image/svg+xml',
  'image/webp',
]);

/** Resolves only backend-declared package images before using the shared Markdown sanitizer. */
export async function preparePptxSlideHtml(
  markdown: string,
  resources: readonly PptxPreviewResourceDescriptor[],
  readResource: (resourceId: string) => Promise<PptxPreviewResource>,
): Promise<string> {
  let resolvedMarkdown = markdown;
  await Promise.all(
    resources.map(async (descriptor) => {
      const token = `pptx-resource:${descriptor.source}`;
      if (!resolvedMarkdown.includes(token)) return;
      const mediaType = descriptor.mediaType.toLowerCase();
      if (!SAFE_IMAGE_MEDIA_TYPES.has(mediaType)) return;
      const resource = await readResource(descriptor.resourceId);
      if (
        resource.mediaType.toLowerCase() !== mediaType ||
        resource.data.length !== descriptor.byteLength
      ) {
        return;
      }
      const bytes = Uint8Array.from(resource.data);
      const safeBytes =
        mediaType === 'image/svg+xml'
          ? new TextEncoder().encode(sanitizeEpubSvg(new TextDecoder().decode(bytes)))
          : bytes;
      resolvedMarkdown = resolvedMarkdown.split(token).join(bytesToDataUri(safeBytes, mediaType));
    }),
  );

  const document = new DOMParser().parseFromString(safeMarkdownHtml(resolvedMarkdown), 'text/html');
  for (const image of Array.from(document.querySelectorAll('img'))) {
    image.removeAttribute('srcset');
    if (
      !/^data:image\/(?:bmp|gif|jpeg|png|svg\+xml|webp);base64,/i.test(
        image.getAttribute('src') ?? '',
      )
    ) {
      image.removeAttribute('src');
    }
  }
  for (const link of Array.from(document.querySelectorAll('a[href]'))) {
    const href = link.getAttribute('href') ?? '';
    if (/^(?:https?:|mailto:)/i.test(href)) {
      link.setAttribute('target', '_blank');
      link.setAttribute('rel', 'noopener noreferrer');
    } else if (!href.startsWith('#')) {
      link.removeAttribute('href');
      link.removeAttribute('target');
      link.removeAttribute('rel');
    }
  }
  return document.body.innerHTML;
}
