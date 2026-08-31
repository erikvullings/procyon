import DOMPurify from 'dompurify';

/** Resolves `href` (from an OPF manifest entry) against `baseDir` (the OPF file's own directory,
 * trailing-slash-terminated or empty for the archive root), collapsing `.`/`..` segments - a
 * minimal relative-path resolver so EPUB manifests that live in a subdirectory (the common case:
 * `OEBPS/content.opf` referencing `OEBPS/text/chapter1.xhtml` as `text/chapter1.xhtml`) resolve to
 * the correct in-archive path. */
export function resolveEpubPath(baseDir: string, href: string): string {
  const stack: string[] = [];
  for (const part of `${baseDir}${href}`.split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') stack.pop();
    else stack.push(part);
  }
  return stack.join('/');
}

function dirnameWithTrailingSlash(path: string): string {
  const index = path.lastIndexOf('/');
  return index === -1 ? '' : path.slice(0, index + 1);
}

/** Reads the OPF (package document) path out of an EPUB's `META-INF/container.xml`. */
export function parseEpubContainer(containerXml: string): string | undefined {
  const doc = new DOMParser().parseFromString(containerXml, 'application/xml');
  return doc.querySelector('rootfile')?.getAttribute('full-path') ?? undefined;
}

export interface EpubPackage {
  readonly title: string | undefined;
  /** In-archive paths of the spine's XHTML content documents, in reading order. */
  readonly chapterPaths: readonly string[];
  /** EPUB 2 NCX or EPUB 3 navigation document, when declared by the package. */
  readonly navigationPath: string | undefined;
  /** Browser-renderable image resources (including declared fallbacks), keyed by source path. */
  readonly imageResources: ReadonlyMap<string, EpubImageResource>;
}

export interface EpubImageResource {
  readonly path: string;
  readonly mediaType: string;
}

interface EpubManifestItem {
  readonly id: string;
  readonly path: string;
  readonly mediaType: string | undefined;
  readonly fallbackId: string | undefined;
}

function browserImageFallback(
  item: EpubManifestItem,
  manifest: ReadonlyMap<string, EpubManifestItem>,
): EpubImageResource | undefined {
  const visited = new Set<string>();
  let candidate: EpubManifestItem | undefined = item;
  while (candidate !== undefined && !visited.has(candidate.id)) {
    visited.add(candidate.id);
    if (
      candidate.mediaType !== undefined &&
      BROWSER_IMAGE_MEDIA_TYPES.has(candidate.mediaType.toLowerCase())
    ) {
      return { path: candidate.path, mediaType: candidate.mediaType };
    }
    candidate = candidate.fallbackId === undefined ? undefined : manifest.get(candidate.fallbackId);
  }
  return undefined;
}

/** Parses an EPUB's OPF package document: the manifest (id -> href, filtered to (X)HTML content
 * documents) and the spine (reading order, by manifest id), resolving every href against the
 * OPF's own directory so callers get archive-root-relative paths ready to fetch directly. */
export function parseEpubPackage(opfXml: string, opfPath: string): EpubPackage {
  const doc = new DOMParser().parseFromString(opfXml, 'application/xml');
  const baseDir = dirnameWithTrailingSlash(opfPath);
  const manifest = new Map<string, EpubManifestItem>();
  for (const item of Array.from(doc.querySelectorAll('manifest > item'))) {
    const id = item.getAttribute('id');
    const href = item.getAttribute('href');
    if (id === null || href === null) continue;
    manifest.set(id, {
      id,
      path: resolveEpubPath(baseDir, href),
      mediaType: item.getAttribute('media-type') ?? undefined,
      fallbackId: item.getAttribute('fallback') ?? undefined,
    });
  }
  const imageResources = new Map<string, EpubImageResource>();
  for (const item of manifest.values()) {
    const fallback = browserImageFallback(item, manifest);
    if (fallback !== undefined) imageResources.set(item.path, fallback);
  }
  const chapterPaths: string[] = [];
  for (const itemref of Array.from(doc.querySelectorAll('spine > itemref'))) {
    const idref = itemref.getAttribute('idref');
    const item = idref === null ? undefined : manifest.get(idref);
    if (item !== undefined && (item.mediaType === undefined || item.mediaType.includes('html'))) {
      chapterPaths.push(item.path);
    }
  }
  const titleText = doc.getElementsByTagName('dc:title')[0]?.textContent?.trim();
  const spineTocId = doc.querySelector('spine')?.getAttribute('toc');
  const navigationItem =
    (spineTocId === null || spineTocId === undefined ? undefined : manifest.get(spineTocId)) ??
    Array.from(manifest.values()).find((item) => {
      const element = doc.querySelector(`manifest > item[id="${CSS.escape(item.id)}"]`);
      return element?.getAttribute('properties')?.split(/\s+/).includes('nav') === true;
    });
  return {
    title: titleText === '' ? undefined : titleText,
    chapterPaths,
    navigationPath: navigationItem?.path,
    imageResources,
  };
}

const ENGLISH_CHAPTER_NUMBERS: Readonly<Record<string, number>> = {
  one: 1,
  two: 2,
  three: 3,
  four: 4,
  five: 5,
  six: 6,
  seven: 7,
  eight: 8,
  nine: 9,
  ten: 10,
  eleven: 11,
  twelve: 12,
  thirteen: 13,
  fourteen: 14,
  fifteen: 15,
  sixteen: 16,
  seventeen: 17,
  eighteen: 18,
  nineteen: 19,
  twenty: 20,
  thirty: 30,
  forty: 40,
  fifty: 50,
  sixty: 60,
  seventy: 70,
  eighty: 80,
  ninety: 90,
};

function romanNumeralValue(value: string): number | undefined {
  if (!/^[ivxlcdm]+$/i.test(value)) return undefined;
  const values: Readonly<Record<string, number>> = {
    i: 1,
    v: 5,
    x: 10,
    l: 50,
    c: 100,
    d: 500,
    m: 1000,
  };
  let total = 0;
  let previous = 0;
  for (const character of Array.from(value.toLowerCase()).reverse()) {
    const current = values[character];
    if (current === undefined) return undefined;
    total += current < previous ? -current : current;
    previous = current;
  }
  return total > 0 ? total : undefined;
}

function chapterNumber(label: string): number | undefined {
  const match = /^\s*(?:chapter|chap\.?)\s+([a-z\d]+(?:[-\s][a-z]+)?)/i.exec(label);
  const value = match?.[1]?.toLowerCase();
  if (value === undefined) return undefined;
  if (/^\d+$/.test(value)) return Number(value);
  const words = value.split(/[-\s]+/);
  const wordValues = words.map((word) => ENGLISH_CHAPTER_NUMBERS[word]);
  if (wordValues.every((number): number is number => number !== undefined)) {
    return wordValues.reduce((total, number) => total + number, 0);
  }
  return romanNumeralValue(value);
}

interface NumberedNavigationEntry {
  readonly path: string;
  readonly number: number;
}

function numberedNavigationEntries(
  navigationXml: string,
  navigationPath: string,
): readonly NumberedNavigationEntry[] {
  const doc = new DOMParser().parseFromString(navigationXml, 'application/xml');
  const baseDir = dirnameWithTrailingSlash(navigationPath);
  const entries: NumberedNavigationEntry[] = [];
  for (const point of Array.from(doc.querySelectorAll('navPoint'))) {
    const label = point.querySelector('navLabel > text')?.textContent ?? '';
    const href = point.querySelector('content')?.getAttribute('src');
    const number = chapterNumber(label);
    if (href !== null && href !== undefined && number !== undefined) {
      entries.push({
        path: resolveEpubPath(baseDir, pathWithoutQueryOrFragment(href)),
        number,
      });
    }
  }
  return entries;
}

function commonPathPrefix(paths: readonly string[]): string {
  const first = paths[0];
  if (first === undefined) return '';
  let length = first.length;
  for (const path of paths.slice(1)) {
    length = Math.min(length, path.length);
    let index = 0;
    while (index < length && first[index] === path[index]) index += 1;
    length = index;
  }
  return first.slice(0, length);
}

/**
 * Repairs a malformed spine only when clearly numbered TOC labels are out of order and their
 * target filenames independently confirm the ascending sequence. Valid explicit spine ordering is
 * otherwise preserved.
 */
export function repairEpubChapterOrder(
  chapterPaths: readonly string[],
  navigationXml: string,
  navigationPath: string,
): readonly string[] {
  const entries = numberedNavigationEntries(navigationXml, navigationPath);
  if (entries.length < 2 || new Set(entries.map((entry) => entry.number)).size !== entries.length) {
    return chapterPaths;
  }
  const expected = [...entries].sort((left, right) => left.number - right.number);
  if (entries.every((entry, index) => entry.number === expected[index]?.number))
    return chapterPaths;
  const natural = [...entries].sort((left, right) =>
    left.path.localeCompare(right.path, undefined, { numeric: true }),
  );
  if (!natural.every((entry, index) => entry.number === expected[index]?.number)) {
    return chapterPaths;
  }

  const targetPaths = new Set(entries.map((entry) => entry.path));
  const prefix = commonPathPrefix(entries.map((entry) => entry.path));
  const directoryEnd = prefix.lastIndexOf('/') + 1;
  const hasDistinctiveFilenamePrefix = prefix.length > directoryEnd;
  const repairIndexes = chapterPaths
    .map((path, index) => ({ path, index }))
    .filter(({ path }) =>
      hasDistinctiveFilenamePrefix ? path.startsWith(prefix) : targetPaths.has(path),
    )
    .map(({ index }) => index);
  const repairedPaths = repairIndexes
    .map((index) => chapterPaths[index])
    .filter((path): path is string => path !== undefined)
    .sort((left, right) => left.localeCompare(right, undefined, { numeric: true }));
  const repaired = [...chapterPaths];
  repairIndexes.forEach((index, position) => {
    const path = repairedPaths[position];
    if (path !== undefined) repaired[index] = path;
  });
  return repaired;
}

/** Sanitizes one chapter's raw XHTML for display. Strips scripts/styles/forms/embeds and never
 * executes anything from previewed content (task 0071). */
export function sanitizeEpubChapterHtml(xhtml: string): string {
  const sanitized = DOMPurify.sanitize(xhtml, {
    USE_PROFILES: { html: true, svg: true, svgFilters: true, mathMl: true },
    ADD_TAGS: ['use'],
    ADD_ATTR: ['href', 'xlink:href'],
    FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'form', 'link', 'foreignObject'],
    FORBID_ATTR: ['style'],
    ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.-:]|$))/i,
  });
  return stripExternalSvgReferences(sanitized);
}

function stripExternalSvgReferences(markup: string): string {
  const doc = new DOMParser().parseFromString(markup, 'text/html');
  for (const element of Array.from(doc.querySelectorAll('svg, svg *'))) {
    for (const attributeName of ['href', 'xlink:href', 'src']) {
      const value = element.getAttribute(attributeName);
      const reference = value?.trimStart();
      const safeReference =
        reference?.startsWith('#') === true ||
        /^data:image\/(?:png|jpeg|gif|webp);base64,/i.test(reference ?? '');
      if (value !== null && !safeReference) {
        element.removeAttribute(attributeName);
      }
    }
  }
  return doc.body.innerHTML;
}

/** Sanitizes an external SVG resource before it is embedded as an EPUB image data URI. */
export function sanitizeEpubSvg(svg: string): string {
  const sanitized = DOMPurify.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true },
    ADD_TAGS: ['use'],
    ADD_ATTR: ['href', 'xlink:href'],
    FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'foreignObject'],
    FORBID_ATTR: ['style'],
  });
  return stripExternalSvgReferences(sanitized);
}

const BROWSER_IMAGE_MEDIA_TYPES = new Set([
  'image/avif',
  'image/bmp',
  'image/gif',
  'image/jpeg',
  'image/png',
  'image/svg+xml',
  'image/webp',
  'image/x-icon',
]);

function pathWithoutQueryOrFragment(path: string): string {
  const suffixIndex = path.search(/[?#]/);
  return suffixIndex === -1 ? path : path.slice(0, suffixIndex);
}

/**
 * Replaces archive-relative chapter image references with generated data URIs before sanitizing
 * the result. Unsupported formats such as EPS retain their alt text but no broken external source.
 */
export async function inlineEpubChapterImages(
  xhtml: string,
  chapterPath: string,
  imageResources: ReadonlyMap<string, EpubImageResource>,
  readImage: (path: string, mediaType: string) => Promise<string>,
): Promise<string> {
  const doc = new DOMParser().parseFromString(xhtml, 'text/html');
  const chapterDirectory = dirnameWithTrailingSlash(chapterPath);
  const pendingImages = new Map<string, Promise<string>>();
  const replacements: Array<Promise<void>> = [];

  for (const image of Array.from(doc.querySelectorAll('img[src]'))) {
    const source = image.getAttribute('src');
    if (source === null || /^(?:data:|https?:)/i.test(source)) continue;
    const imagePath = resolveEpubPath(chapterDirectory, pathWithoutQueryOrFragment(source));
    const resource = imageResources.get(imagePath);
    if (resource === undefined) {
      image.removeAttribute('src');
      continue;
    }
    let dataUri = pendingImages.get(resource.path);
    if (dataUri === undefined) {
      dataUri = readImage(resource.path, resource.mediaType);
      pendingImages.set(resource.path, dataUri);
    }
    replacements.push(
      dataUri.then((resolvedSource) => {
        image.setAttribute('src', resolvedSource);
      }),
    );
  }

  await Promise.all(replacements);
  return sanitizeEpubChapterHtml(doc.body.innerHTML);
}
