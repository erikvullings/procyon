import type {
  DocxPreviewResource,
  DocxPreviewResourceDescriptor,
  SearchInFileMatch,
} from '../../models';
import { bytesToDataUri } from './content-preview';
import { sanitizeEpubChapterHtml, sanitizeEpubSvg } from './epub-preview';

const SAFE_IMAGE_MEDIA_TYPES = new Set([
  'image/bmp',
  'image/gif',
  'image/jpeg',
  'image/png',
  'image/svg+xml',
  'image/webp',
]);
const MAX_DOCX_SEARCH_MATCHES = 5_000;

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

export interface DocxSearchResult {
  readonly html: string;
  readonly matches: readonly SearchInFileMatch[];
  readonly truncated: boolean;
}

/** Highlights matches in the already-sanitized fragment without flattening its semantic markup. */
export function searchDocxHtml(
  safeHtml: string,
  query: string,
  regex: boolean,
  caseSensitive: boolean,
  wholeWord: boolean,
  activeIndex: number | undefined,
): DocxSearchResult {
  if (query === '') return { html: safeHtml, matches: [], truncated: false };
  const source = regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const expression = new RegExp(
    wholeWord ? `\\b(?:${source})\\b` : source,
    caseSensitive ? 'gu' : 'giu',
  );
  const document = new DOMParser().parseFromString(safeHtml, 'text/html');
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const textNodes: Text[] = [];
  let node = walker.nextNode();
  while (node !== null) {
    textNodes.push(node as Text);
    node = walker.nextNode();
  }
  const fullText = textNodes.map((textNode) => textNode.data).join('');
  const matches: SearchInFileMatch[] = [];
  let truncated = false;
  for (const match of fullText.matchAll(expression)) {
    const value = match[0];
    if (value.length === 0) continue;
    if (matches.length === MAX_DOCX_SEARCH_MATCHES) {
      truncated = true;
      break;
    }
    matches.push({
      offset: match.index,
      length: value.length,
      lineNumber: 1,
    });
  }

  let nodeOffset = 0;
  for (const textNode of textNodes) {
    const text = textNode.data;
    const fragments: Array<Node | string> = [];
    let cursor = 0;
    const nodeEnd = nodeOffset + text.length;
    for (const [matchIndex, match] of matches.entries()) {
      const matchEnd = match.offset + match.length;
      if (matchEnd <= nodeOffset) continue;
      if (match.offset >= nodeEnd) break;
      const start = Math.max(0, match.offset - nodeOffset);
      const end = Math.min(text.length, matchEnd - nodeOffset);
      fragments.push(text.slice(cursor, start));
      const mark = document.createElement('mark');
      mark.className =
        matchIndex === activeIndex ? 'fm-docx-search-match-active' : 'fm-docx-search-match';
      mark.textContent = text.slice(start, end);
      fragments.push(mark);
      cursor = end;
    }
    if (fragments.length > 0) {
      fragments.push(text.slice(cursor));
      textNode.replaceWith(...fragments);
    }
    nodeOffset = nodeEnd;
  }
  return { html: document.body.innerHTML, matches, truncated };
}
