import m, { type FactoryComponent } from 'mithril';
import type { ConnectionClient } from '../../api/client/connection-client.ts';
import type { SemanticClient } from '../../api/client/semantic-client.ts';
import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import { checkIcon, copyIcon, photoIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { KnowledgeEvidence } from '../../models';
import { thumbnailMimeType } from '../../utilities/image-mime';
import { copyImageDataUri, copyText } from '../preview/clipboard';
import { decodeEvidenceTitle } from './evidence-title';
import { mediaKind } from './media-evidence';

export interface KnowledgeImagePreviewAttrs {
  readonly client: ConnectionClient & SemanticClient & WorkspaceClient;
  readonly workspaceId: string;
  readonly evidence: KnowledgeEvidence;
  readonly title: string;
}

export const KnowledgeImagePreview: FactoryComponent<KnowledgeImagePreviewAttrs> = () => {
  let observer: IntersectionObserver | undefined;
  let controller: AbortController | undefined;
  let started = false;
  let dataUri: string | undefined;
  let unavailable = false;
  let removed = false;

  async function load(attrs: KnowledgeImagePreviewAttrs): Promise<void> {
    if (started || removed) return;
    started = true;
    controller = new AbortController();
    const signal = controller.signal;
    try {
      const source = await attrs.client.resolveKnowledgeSource(
        { workspaceId: attrs.workspaceId, sourceId: attrs.evidence.sourceId },
        signal,
      );
      // PDF page thumbnails are not the indexed embedded image; never present
      // one as a preview of that image.
      if (!source.available || !/\.(?:jpe?g|png|gif|webp|ico)$/iu.test(source.location.uri)) {
        unavailable = true;
        return;
      }
      const bytes = await attrs.client.getThumbnail(source.location.uri, 'small', signal);
      if (bytes === undefined || bytes.length === 0 || bytes.length > 128 * 1024) {
        unavailable = true;
        return;
      }
      let binary = '';
      for (const byte of bytes) binary += String.fromCharCode(byte);
      dataUri = `data:${thumbnailMimeType(bytes)};base64,${btoa(binary)}`;
    } catch {
      if (!signal.aborted) unavailable = true;
    } finally {
      if (!signal.aborted) m.redraw();
    }
  }

  return {
    oncreate: ({ dom, attrs }) => {
      if (typeof IntersectionObserver === 'undefined') {
        void load(attrs);
        return;
      }
      observer = new IntersectionObserver(
        (entries) => {
          if (!entries.some((entry) => entry.isIntersecting)) return;
          observer?.disconnect();
          void load(attrs);
        },
        { rootMargin: '96px' },
      );
      observer.observe(dom);
    },
    onremove: () => {
      removed = true;
      observer?.disconnect();
      controller?.abort();
    },
    view: ({ attrs }) =>
      m(
        'span.fm-knowledge-image-preview',
        dataUri === undefined
          ? m(
              'span.fm-knowledge-image-placeholder',
              {
                role: 'img',
                'aria-label': t(
                  'knowledgeSearch',
                  unavailable ? 'imagePreviewUnavailable' : 'imagePreviewLoading',
                ),
              },
              photoIcon(),
            )
          : m('img', {
              src: dataUri,
              alt: t('knowledgeSearch', 'imagePreviewAlt', { title: attrs.title }),
              loading: 'lazy',
              decoding: 'async',
              onerror: () => {
                dataUri = undefined;
                unavailable = true;
                m.redraw();
              },
            }),
      ),
  };
};

export const KnowledgeChunkCopy: FactoryComponent<{
  readonly evidence: KnowledgeEvidence;
  readonly title: string;
}> = () => {
  let copied = false;
  let failed = false;
  let resetTimer: number | undefined;

  return {
    onremove: () => {
      if (resetTimer !== undefined) clearTimeout(resetTimer);
    },
    view: ({ attrs }) =>
      m('span.fm-knowledge-chunk-copy', [
        m(
          'button.fm-knowledge-chunk-copy-button',
          {
            type: 'button',
            'aria-label': t('knowledgeSearch', 'copyChunk'),
            onclick: (event: MouseEvent) => {
              const section = (event.currentTarget as Element).closest('.fm-knowledge-result');
              const content = section
                ?.querySelector('.fm-knowledge-chunk-content')
                ?.textContent?.trim();
              if (content === undefined || content === '') return;
              const position = knowledgeProvenanceLabel(attrs.evidence.provenance);
              const source = t('knowledgeSearch', 'chunkSource', {
                title: `${attrs.title}${position === '' ? '' : `, ${position}`}`,
              });
              void (async () => {
                try {
                  await copyText(`${content}\n\n${source}`);
                  copied = true;
                  failed = false;
                  if (resetTimer !== undefined) clearTimeout(resetTimer);
                  resetTimer = window.setTimeout(() => {
                    copied = false;
                    m.redraw();
                  }, 1500);
                } catch {
                  copied = false;
                  failed = true;
                }
                m.redraw();
              })();
            },
          },
          copied ? checkIcon({ size: 14 }) : copyIcon({ size: 14 }),
        ),
        copied
          ? m('span.fm-visually-hidden', { role: 'status' }, t('knowledgeSearch', 'chunkCopied'))
          : undefined,
        failed
          ? m('span.fm-knowledge-error', { role: 'alert' }, t('knowledgeSearch', 'copyChunkFailed'))
          : undefined,
      ]),
  };
};

export const KNOWLEDGE_COPY_IMAGE_LIMIT = 8 * 1024 * 1024;
export const KNOWLEDGE_COPY_READ_LENGTH = 1024 * 1024;

export const KnowledgeMediaCopy: FactoryComponent<{
  readonly client: ConnectionClient & SemanticClient & WorkspaceClient;
  readonly workspaceId: string;
  readonly evidence: KnowledgeEvidence;
  readonly title: string;
  readonly description: string;
}> = () => {
  let copied = false;
  let error: string | undefined;
  let resetTimer: number | undefined;
  let controller: AbortController | undefined;
  let busy = false;

  async function copy(attrs: {
    readonly client: ConnectionClient & SemanticClient & WorkspaceClient;
    readonly workspaceId: string;
    readonly evidence: KnowledgeEvidence;
    readonly title: string;
    readonly description: string;
  }): Promise<void> {
    if (busy) return;
    busy = true;
    controller = new AbortController();
    const signal = controller.signal;
    const kind = mediaKind(attrs.evidence.mediaType);
    try {
      const source = await attrs.client.resolveKnowledgeSource(
        { workspaceId: attrs.workspaceId, sourceId: attrs.evidence.sourceId },
        signal,
      );
      if (!source.available) throw new Error(t('knowledgeSearch', 'copySourceUnavailable'));
      if (kind === 'image') {
        if (!/\.(?:jpe?g|png|gif|webp|ico)$/iu.test(source.location.uri)) {
          throw new Error(t('knowledgeSearch', 'copyEmbeddedImageUnavailable'));
        }
        const bytes = new Uint8Array(KNOWLEDGE_COPY_IMAGE_LIMIT);
        let offset = 0;
        let eof = false;
        while (!eof && offset < KNOWLEDGE_COPY_IMAGE_LIMIT) {
          const part = await attrs.client.readFileRange(
            { location: source.location, offset, length: KNOWLEDGE_COPY_READ_LENGTH },
            signal,
          );
          if (part.offset !== offset || part.length === 0 || part.length !== part.data.length) {
            throw new Error(t('knowledgeSearch', 'copyMediaFailed'));
          }
          if (offset + part.length > KNOWLEDGE_COPY_IMAGE_LIMIT) {
            throw new Error(t('knowledgeSearch', 'copyImageTooLarge'));
          }
          bytes.set(part.data, offset);
          offset += part.length;
          eof = part.eof;
        }
        if (!eof || offset === 0) {
          throw new Error(t('knowledgeSearch', 'copyImageTooLarge'));
        }
        const png =
          bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47;
        const jpeg = bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff;
        if (!png && !jpeg) throw new Error(t('knowledgeSearch', 'copyImageUnsupported'));
        let binary = '';
        for (const byte of bytes.subarray(0, offset)) binary += String.fromCharCode(byte);
        if (signal.aborted) return;
        await copyImageDataUri(`data:image/${png ? 'png' : 'jpeg'};base64,${btoa(binary)}`);
      } else {
        const uri = source.location.uri;
        if (source.location.providerId !== 'local' || !uri.startsWith('file:///')) {
          throw new Error(t('knowledgeSearch', 'copyLocalLinkUnavailable'));
        }
        if (signal.aborted) return;
        await copyText(
          `${decodeEvidenceText(attrs.description)}\n\n${t('knowledgeSearch', 'chunkSource', { title: attrs.title })}\n${uri}`,
        );
      }
      if (signal.aborted) return;
      error = undefined;
      copied = true;
      if (resetTimer !== undefined) clearTimeout(resetTimer);
      resetTimer = window.setTimeout(() => {
        copied = false;
        m.redraw();
      }, 1500);
    } catch (cause) {
      if (!signal.aborted) {
        copied = false;
        error = cause instanceof Error ? cause.message : t('knowledgeSearch', 'copyMediaFailed');
      }
    } finally {
      busy = false;
      if (!signal.aborted) m.redraw();
    }
  }

  return {
    onremove: () => {
      controller?.abort();
      if (resetTimer !== undefined) clearTimeout(resetTimer);
    },
    view: ({ attrs }) => {
      const image = mediaKind(attrs.evidence.mediaType) === 'image';
      return m('span.fm-knowledge-media-copy', [
        m(
          'button.fm-knowledge-media-copy-button',
          {
            type: 'button',
            disabled: busy || attrs.evidence.unavailable || attrs.evidence.stale === true,
            'aria-label': t('knowledgeSearch', image ? 'copyImage' : 'copyMediaDetails'),
            onclick: () => void copy(attrs),
          },
          copied ? checkIcon({ size: 14 }) : copyIcon({ size: 14 }),
        ),
        copied
          ? m(
              'span.fm-visually-hidden',
              { role: 'status' },
              t('knowledgeSearch', image ? 'imageCopied' : 'mediaDetailsCopied'),
            )
          : undefined,
        error === undefined ? undefined : m('span.fm-knowledge-error', { role: 'alert' }, error),
      ]);
    },
  };
};

export function decodeEvidenceText(text: string): string {
  const textarea = document.createElement('textarea');
  textarea.innerHTML = text.replaceAll('<', '&lt;').replaceAll('>', '&gt;');
  return textarea.value.replace(
    /(https?:\/\/doi\.org\/10\.1016\/j\.)\s+([a-z]+\.\d{4}\.)/giu,
    '$1$2',
  );
}

export function externalEvidenceUrl(value: string): string | undefined {
  if (!/^https?:\/\//iu.test(value)) return undefined;
  try {
    const parsed = new URL(value);
    return (parsed.protocol === 'http:' || parsed.protocol === 'https:') &&
      parsed.hostname !== '' &&
      parsed.username === '' &&
      parsed.password === ''
      ? parsed.href
      : undefined;
  } catch {
    return undefined;
  }
}

export function evidenceTextWithLinks(text: string): m.Children {
  const nodes: m.Children[] = [];
  const urlPattern = /https?:\/\/[^\s<>"']+/giu;
  let last = 0;
  for (const match of text.matchAll(urlPattern)) {
    const start = match.index;
    const raw = match[0];
    let end = raw.length;
    while (
      end > 0 &&
      (/[.,;:!?]/u.test(raw[end - 1] ?? '') ||
        (raw[end - 1] === ')' &&
          (raw.slice(0, end).match(/\)/gu)?.length ?? 0) >
            (raw.slice(0, end).match(/\(/gu)?.length ?? 0)))
    ) {
      end--;
    }
    const label = raw.slice(0, end);
    const href = externalEvidenceUrl(label);
    if (href === undefined) continue;
    nodes.push(
      text.slice(last, start),
      m('a', { href, target: '_blank', rel: 'noopener noreferrer' }, label),
    );
    last = start + end;
  }
  nodes.push(text.slice(last));
  return nodes;
}

/** Renders serialized structural provenance as a short human location. */
export function knowledgeProvenanceLabel(provenance: string): string {
  const describe = (value: unknown): string | undefined => {
    if (typeof value === 'string') {
      try {
        return describe(JSON.parse(value));
      } catch {
        return value.length === 0 ? undefined : value;
      }
    }
    if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined;
    const node = value as Record<string, unknown>;
    const number = (key: string): number | undefined =>
      typeof node[key] === 'number' ? node[key] : undefined;
    switch (node.kind) {
      case 'exact':
        return describe(node.value);
      case 'span': {
        const span =
          node.value !== null && typeof node.value === 'object' && !Array.isArray(node.value)
            ? (node.value as Record<string, unknown>)
            : node;
        const first = describe(span.first);
        const last = describe(span.last);
        if (first === undefined) return last;
        if (last === undefined || last === first) return first;
        return `${first} – ${last}`;
      }
      case 'pdfBlock': {
        const page = number('page_number');
        return page === undefined ? undefined : t('ragAsk', 'citationPage', { page });
      }
      case 'pdfImage': {
        const page = number('page_number');
        const image = number('image_index');
        return page === undefined ||
          !Number.isInteger(page) ||
          page <= 0 ||
          image === undefined ||
          !Number.isInteger(image) ||
          image <= 0
          ? undefined
          : t('knowledgeSearch', 'pdfImageCitation', { page, image });
      }
      case 'pdfPageRender': {
        const page = number('page_number');
        return page === undefined || !Number.isInteger(page) || page <= 0
          ? undefined
          : t('ragAsk', 'citationPage', { page });
      }
      case 'textLines':
      case 'codeLines': {
        const start = number('start_line');
        const end = number('end_line');
        return start === undefined || end === undefined
          ? undefined
          : t('ragAsk', 'citationLines', { start, end });
      }
      case 'epubText': {
        const spine = number('spine_index');
        const start = number('start_line');
        const end = number('end_line');
        return spine === undefined || start === undefined || end === undefined
          ? undefined
          : t('ragAsk', 'citationEpubLines', { chapter: spine + 1, start, end });
      }
      case 'jsonlRecord': {
        const line = number('line');
        return line === undefined ? undefined : t('ragAsk', 'citationRecord', { line });
      }
      case 'slide': {
        const slide = number('slide_number');
        return slide === undefined ? undefined : t('ragAsk', 'citationSlide', { slide });
      }
      case 'spreadsheetRange': {
        const range = spreadsheetRangeLabel(node);
        if (range === undefined) return undefined;
        const sheet = typeof node.sheet === 'string' ? node.sheet.trim() : '';
        return sheet.length === 0
          ? t('ragAsk', 'citationCells', { range })
          : t('ragAsk', 'citationSheetCells', { sheet, range });
      }
      case 'docxBlock': {
        const block = number('block_index');
        return block === undefined ? undefined : t('ragAsk', 'citationBlock', { block: block + 1 });
      }
      default:
        return undefined;
    }
  };
  return describe(provenance) ?? '';
}

/**
 * Renders a 0-based inclusive spreadsheet cell rectangle as A1 notation. A
 * single cell renders as `B3`, a rectangle as `B3:D8`; anything with a missing
 * or non-numeric bound renders as nothing rather than as an invented range.
 */
export function spreadsheetRangeLabel(node: Record<string, unknown>): string | undefined {
  const index = (key: string): number | undefined => {
    const value = node[key];
    return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : undefined;
  };
  const startRow = index('start_row');
  const startColumn = index('start_column');
  const endRow = index('end_row');
  const endColumn = index('end_column');
  if (
    startRow === undefined ||
    startColumn === undefined ||
    endRow === undefined ||
    endColumn === undefined
  ) {
    return undefined;
  }
  const first = `${spreadsheetColumnName(startColumn)}${startRow + 1}`;
  const last = `${spreadsheetColumnName(endColumn)}${endRow + 1}`;
  return first === last ? first : `${first}:${last}`;
}

/** Converts a 0-based column index into its spreadsheet letters (0 → `A`). */
export function spreadsheetColumnName(column: number): string {
  let remaining = column;
  let name = '';
  do {
    name = String.fromCharCode(65 + (remaining % 26)) + name;
    remaining = Math.floor(remaining / 26) - 1;
  } while (remaining >= 0);
  return name;
}

/**
 * Ranks documents by their best match and chunks within each document by
 * relevance. Adjacent chunks remain available to grounded-answer generation,
 * but are not presented as search matches. Two documents can share a title,
 * so the stable `documentId` remains the key.
 */
export function groupEvidenceByDocument(evidence: readonly KnowledgeEvidence[]): readonly {
  readonly documentId: string;
  readonly title: string;
  readonly rows: KnowledgeEvidence[];
  readonly openEvidence: KnowledgeEvidence;
  readonly bestRank: number;
  readonly bestScore: number;
}[] {
  const groups = new Map<
    string,
    {
      documentId: string;
      title: string;
      rows: KnowledgeEvidence[];
      openEvidence: KnowledgeEvidence;
      bestRank: number;
      bestScore: number;
    }
  >();
  for (const row of evidence) {
    if (row.adjacent) continue;
    const existing = groups.get(row.documentId);
    if (existing === undefined) {
      groups.set(row.documentId, {
        documentId: row.documentId,
        title: decodeEvidenceTitle(row.title) ?? row.title,
        rows: [row],
        openEvidence: row,
        bestRank: row.finalRank,
        bestScore: row.fusedScore,
      });
      continue;
    }
    existing.rows.push(row);
    const outranksBest =
      row.finalRank < existing.bestRank ||
      (row.finalRank === existing.bestRank && row.fusedScore > existing.bestScore);
    if (outranksBest) {
      existing.bestRank = row.finalRank;
      existing.bestScore = row.fusedScore;
    }
    const outranksOpenTarget =
      row.finalRank < existing.openEvidence.finalRank ||
      (row.finalRank === existing.openEvidence.finalRank &&
        row.fusedScore > existing.openEvidence.fusedScore);
    if (
      (!row.unavailable && existing.openEvidence.unavailable) ||
      (row.unavailable === existing.openEvidence.unavailable && outranksOpenTarget)
    ) {
      existing.openEvidence = row;
    }
  }
  return [...groups.values()]
    .map((group) => ({
      ...group,
      rows: group.rows.toSorted(
        (left, right) =>
          left.finalRank - right.finalRank ||
          left.sourcePosition - right.sourcePosition ||
          left.recordId.localeCompare(right.recordId),
      ),
    }))
    .toSorted(
      (left, right) =>
        left.bestRank - right.bestRank ||
        right.bestScore - left.bestScore ||
        left.documentId.localeCompare(right.documentId),
    );
}
