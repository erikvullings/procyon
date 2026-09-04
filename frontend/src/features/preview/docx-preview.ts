import type { DocxPreviewResource, DocxPreviewResourceDescriptor } from '../../models';
import { bytesToDataUri } from './content-preview';
import { sanitizeEpubChapterHtml, sanitizeEpubSvg } from './epub-preview';
import { type HtmlSearchResult, searchHtml } from './html-search-highlight';

const SAFE_IMAGE_MEDIA_TYPES = new Set([
  'image/bmp',
  'image/gif',
  'image/jpeg',
  'image/png',
  'image/svg+xml',
  'image/webp',
]);

/**
 * Resolves only backend-declared package images, then applies the shared rich-content sanitizer.
 * No package URL survives as a browser-loadable resource.
 */
export async function prepareDocxPreviewHtml(
  rawHtml: string,
  resources: readonly DocxPreviewResourceDescriptor[],
  readResource: (resourceId: string) => Promise<DocxPreviewResource>,
): Promise<string> {
  const document = new DOMParser().parseFromString(rawHtml, 'text/html');
  const bySource = new Map(resources.map((resource) => [resource.source, resource]));
  const pending = new Map<string, Promise<string | undefined>>();
  const replacements: Array<Promise<void>> = [];

  for (const image of Array.from(document.querySelectorAll('img'))) {
    image.removeAttribute('srcset');
    const source = image.getAttribute('src');
    const descriptor = source === null ? undefined : bySource.get(source);
    if (
      descriptor === undefined ||
      !SAFE_IMAGE_MEDIA_TYPES.has(descriptor.mediaType.toLowerCase())
    ) {
      image.removeAttribute('src');
      continue;
    }
    let dataUri = pending.get(descriptor.resourceId);
    if (dataUri === undefined) {
      dataUri = readResource(descriptor.resourceId).then((resource) => {
        const mediaType = resource.mediaType.toLowerCase();
        if (
          mediaType !== descriptor.mediaType.toLowerCase() ||
          resource.data.length !== descriptor.byteLength ||
          !SAFE_IMAGE_MEDIA_TYPES.has(mediaType)
        ) {
          return undefined;
        }
        const bytes = Uint8Array.from(resource.data);
        if (mediaType === 'image/svg+xml') {
          const safeSvg = sanitizeEpubSvg(new TextDecoder().decode(bytes));
          return bytesToDataUri(new TextEncoder().encode(safeSvg), mediaType);
        }
        return bytesToDataUri(bytes, mediaType);
      });
      pending.set(descriptor.resourceId, dataUri);
    }
    replacements.push(
      dataUri.then((resolved) => {
        if (resolved === undefined) image.removeAttribute('src');
        else image.setAttribute('src', resolved);
      }),
    );
  }
  await Promise.all(replacements);

  for (const element of Array.from(document.querySelectorAll('[src], [srcset], [poster]'))) {
    if (element.tagName !== 'IMG') element.removeAttribute('src');
    element.removeAttribute('srcset');
    element.removeAttribute('poster');
  }
  const sanitized = sanitizeEpubChapterHtml(document.body.innerHTML);
  const safeDocument = new DOMParser().parseFromString(sanitized, 'text/html');
  for (const link of Array.from(safeDocument.querySelectorAll('a[href]'))) {
    const href = link.getAttribute('href') ?? '';
    if (/^(?:https?:|mailto:)/i.test(href)) {
      link.setAttribute('target', '_blank');
      link.setAttribute('rel', 'noopener noreferrer');
    } else {
      link.removeAttribute('target');
      link.removeAttribute('rel');
    }
  }
  return safeDocument.body.innerHTML;
}

export type DocxSearchResult = HtmlSearchResult;

/** Highlights matches in the already-sanitized fragment without flattening its semantic markup. */
export function searchDocxHtml(
  safeHtml: string,
  query: string,
  regex: boolean,
  caseSensitive: boolean,
  wholeWord: boolean,
  activeIndex: number | undefined,
): DocxSearchResult {
  return searchHtml(safeHtml, query, regex, caseSensitive, wholeWord, activeIndex);
}
