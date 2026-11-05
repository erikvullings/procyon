import m from 'mithril';
import type { ConnectionClient } from '../../api/client/connection-client.ts';
import type { SemanticClient } from '../../api/client/semantic-client.ts';
import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import { externalLinkIcon } from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { KnowledgeEvidence, KnowledgeSearchResult } from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { pdfPageRenderProvenance } from './evidence-provenance';
import { decodeEvidenceTitle } from './evidence-title';
import {
  decodeEvidenceText,
  evidenceTextWithLinks,
  externalEvidenceUrl,
  type groupEvidenceByDocument,
  KnowledgeChunkCopy,
  KnowledgeImagePreview,
  KnowledgeMediaCopy,
  knowledgeProvenanceLabel,
} from './knowledge-evidence-view';
import { mediaDescription, mediaKind, mediaLabel, mediaModifiedLabel } from './media-evidence';

/** What the evidence renderers need from the host dialog; no dialog state leaks in. */
export interface KnowledgeEvidenceContext {
  readonly client: ConnectionClient & SemanticClient & WorkspaceClient;
  readonly workspaceId: string;
  /** Whether the host can open a source at all. */
  readonly canOpen: boolean;
  readonly isSourceOpen: (evidence: KnowledgeEvidence) => boolean;
  readonly openSource: (evidence: KnowledgeEvidence) => Promise<void>;
  readonly openExternal: (url: string) => Promise<void>;
}

export function relevanceIndicator(row: KnowledgeEvidence): m.Children {
  const relevanceStrength = row.finalRank <= 3 ? 'high' : row.finalRank <= 10 ? 'medium' : 'low';
  const relevanceKey =
    relevanceStrength === 'high'
      ? 'resultRelevanceHigh'
      : relevanceStrength === 'medium'
        ? 'resultRelevanceMedium'
        : 'resultRelevanceLow';
  const relevance = t('knowledgeSearch', relevanceKey);
  return tooltip(
    relevance,
    m('span.fm-knowledge-relevance-indicator', {
      role: 'img',
      tabindex: 0,
      'aria-label': relevance,
      'data-strength': relevanceStrength,
    }),
    { 'data-tooltip-placement': 'above' },
  );
}

export function evidenceSection(
  ctx: KnowledgeEvidenceContext,
  row: KnowledgeEvidence,
  key: string,
  fileTitle?: string,
  positionInHeader = false,
): m.Vnode {
  const position = knowledgeProvenanceLabel(row.provenance);
  const pdfImage = row.mediaType === 'application/pdf' && row.chunkKind === 'pdf_image';
  const renderedPage = pdfImage ? pdfPageRenderProvenance(row.provenance) : undefined;
  const indexedTitle = row.sectionPath.join(' / ');
  const openable = !row.unavailable && ctx.canOpen;
  const kind = mediaKind(row.mediaType);
  const sourceOpen = ctx.isSourceOpen(row);
  const modifiedLabel = mediaModifiedLabel(row.modifiedAtMs);
  const description = kind === undefined ? undefined : mediaDescription(row.excerpt);
  const redundantImageDescription =
    kind === 'image' &&
    fileTitle !== undefined &&
    description === `image: ${fileTitle} (file-level evidence)`;
  const states = [
    row.adjacent ? t('knowledgeSearch', 'adjacentEvidence') : undefined,
    row.generated ? t('knowledgeSearch', 'generatedEvidence') : undefined,
    row.stale === true ? t('knowledgeSearch', 'staleEvidence') : undefined,
    row.stale == null ? t('knowledgeSearch', 'unknownFreshness') : undefined,
    row.unavailable ? t('knowledgeSearch', 'unavailableEvidence') : undefined,
    (row.pdfImageSkippedPages ?? 0) > 0
      ? t('semanticLibrary', 'pdfImagesSkippedPages', {
          pages: row.pdfImageSkippedPages ?? 0,
        })
      : undefined,
    row.duplicateSourceIds.length === 0
      ? undefined
      : t('knowledgeSearch', 'duplicateSources', { count: row.duplicateSourceIds.length }),
  ].filter((value): value is string => value !== undefined);
  return m('section.fm-knowledge-result', { key }, [
    fileTitle !== undefined || positionInHeader
      ? undefined
      : m('.fm-knowledge-section-heading', [
          indexedTitle === '' ? undefined : m('span.fm-knowledge-section-title', indexedTitle),
          m('.fm-knowledge-section-metadata', [
            position === ''
              ? undefined
              : m(
                  'button.fm-knowledge-page-link',
                  {
                    type: 'button',
                    disabled: !openable,
                    'aria-label': t('knowledgeSearch', 'openSection', { section: position }),
                    onclick: () => void ctx.openSource(row),
                  },
                  position,
                ),
            relevanceIndicator(row),
          ]),
        ]),
    pdfImage
      ? m(
          '.fm-knowledge-pdf-image-evidence',
          t('knowledgeSearch', renderedPage ? 'pdfPageRenderEvidence' : 'pdfImageEvidence'),
        )
      : kind === undefined
        ? m(
            '.fm-knowledge-result-markdown',
            {
              onclick: (event: MouseEvent) => {
                if (!(event.target instanceof Element)) return;
                const anchor = event.target.closest<HTMLAnchorElement>('a[href]');
                if (anchor === null) return;
                event.preventDefault();
                const url = externalEvidenceUrl(anchor.getAttribute('href') ?? '');
                if (url === undefined || document.getSelection()?.toString()) return;
                void ctx.openExternal(url);
              },
            },
            [
              m(KnowledgeChunkCopy, {
                evidence: row,
                title: fileTitle ?? decodeEvidenceTitle(row.title) ?? row.title,
              }),
              m(
                '.fm-knowledge-chunk-content',
                row.mediaType === 'application/pdf'
                  ? m('p', evidenceTextWithLinks(decodeEvidenceText(row.content)))
                  : m.trust(safeMarkdownHtml(row.content)),
              ),
            ],
          )
        : m('.fm-knowledge-media-evidence', [
            kind === 'image' &&
            !row.unavailable &&
            row.stale !== true &&
            row.sectionPath.length === 0 &&
            row.provenance === ''
              ? m(KnowledgeImagePreview, {
                  client: ctx.client,
                  workspaceId: ctx.workspaceId,
                  evidence: row,
                  title: fileTitle ?? decodeEvidenceTitle(row.title) ?? row.title,
                })
              : undefined,
            m('.fm-knowledge-media-summary', [
              m('strong', mediaLabel(kind)),
              modifiedLabel === undefined
                ? undefined
                : m('small.fm-knowledge-media-modified', modifiedLabel),
              m(KnowledgeMediaCopy, {
                client: ctx.client,
                workspaceId: ctx.workspaceId,
                evidence: row,
                title: fileTitle ?? decodeEvidenceTitle(row.title) ?? row.title,
                description: description ?? '',
              }),
              m(
                'button.fm-knowledge-media-open',
                {
                  type: 'button',
                  disabled: !openable,
                  'aria-expanded': sourceOpen ? 'true' : 'false',
                  onclick: () => void ctx.openSource(row),
                },
                t('search', sourceOpen ? 'closeMedia' : 'openMedia', {
                  type: mediaLabel(kind).toLowerCase(),
                }),
              ),
            ]),
            redundantImageDescription ? undefined : m('p', description),
          ]),
    states.length === 0
      ? undefined
      : m(
          'p.fm-knowledge-state',
          states.flatMap((state, index) => (index === 0 ? [state] : [' · ', state])),
        ),
  ]);
}

export function evidenceDocument(
  ctx: KnowledgeEvidenceContext,
  group: ReturnType<typeof groupEvidenceByDocument>[number],
  index: number,
): m.Vnode {
  const openable = !group.openEvidence.unavailable && ctx.canOpen;
  const firstRow = group.rows[0];
  const firstPosition =
    firstRow !== undefined && firstRow.sectionPath.length === 0
      ? knowledgeProvenanceLabel(firstRow.provenance)
      : '';
  const compactFileMedia =
    group.rows.length === 1 &&
    firstRow !== undefined &&
    mediaKind(firstRow.mediaType) !== undefined &&
    firstRow.sectionPath.length === 0 &&
    knowledgeProvenanceLabel(firstRow.provenance) === '';
  return m(
    'li.fm-knowledge-document-item',
    { key: `document-${group.documentId}`, value: index + 1 },
    m(
      'article.fm-knowledge-group',
      { className: compactFileMedia ? 'fm-knowledge-group-compact-media' : '' },
      [
        m('.fm-knowledge-document-heading', [
          m('span.fm-knowledge-document-number', { 'aria-hidden': 'true' }, `${index + 1}.`),
          m('h4.fm-knowledge-result-heading', [
            m(
              'button.fm-knowledge-source-link',
              {
                type: 'button',
                disabled: !openable,
                'aria-label': t('knowledgeSearch', 'openSource', { title: group.title }),
                title: group.title,
                onclick: () => void ctx.openSource(group.openEvidence),
              },
              [externalLinkIcon({ size: 14 }), m('span', group.title)],
            ),
            firstRow !== undefined && firstPosition !== ''
              ? m(
                  'button.fm-knowledge-page-link',
                  {
                    type: 'button',
                    disabled: firstRow.unavailable || !ctx.canOpen,
                    'aria-label': t('knowledgeSearch', 'openSection', { section: firstPosition }),
                    onclick: () => void ctx.openSource(firstRow),
                  },
                  firstPosition,
                )
              : undefined,
            firstRow !== undefined && firstPosition !== ''
              ? relevanceIndicator(firstRow)
              : undefined,
            compactFileMedia && firstRow !== undefined ? relevanceIndicator(firstRow) : undefined,
          ]),
        ]),
        m(
          '.fm-knowledge-sections',
          group.rows.map((row, rowIndex) =>
            evidenceSection(
              ctx,
              row,
              `${group.documentId}-${row.recordId}`,
              compactFileMedia ? group.title : undefined,
              rowIndex === 0 && firstPosition !== '',
            ),
          ),
        ),
      ],
    ),
  );
}

/**
 * Renders the privacy-safe retrieval trace the search was asked for: the
 * bounded planned queries with their per-route candidate counts, the fused
 * row count, and the rank constant that produced the fusion. When a trace was
 * requested but the host returned none, that is said explicitly rather than
 * leaving the checkbox looking like it did nothing.
 */
export function knowledgeTraceView(
  result: KnowledgeSearchResult | undefined,
  traceRequested: boolean,
): m.Children {
  if (result === undefined || !traceRequested) return undefined;
  const trace = result.trace;
  if (trace == null) {
    return m(
      'p.fm-knowledge-trace-body.fm-knowledge-hint',
      { role: 'status' },
      t('knowledgeSearch', 'traceUnavailable'),
    );
  }
  return m('.fm-knowledge-trace-body', [
    m('h4', t('knowledgeSearch', 'traceHeading')),
    m(
      'p',
      t('knowledgeSearch', 'traceSummary', {
        queries: trace.queries.length,
        fused: result.evidence.length,
        constant: trace.rankConstant,
      }),
    ),
    m(
      'ol.fm-knowledge-trace-queries',
      trace.queries.map((query, index) =>
        m('li', { key: `trace-${index}` }, [
          m('code', query.text),
          m(
            'small.fm-knowledge-hint',
            ` · ${t('knowledgeSearch', 'traceCandidates', {
              fullText: query.fullTextCandidates,
              semantic: query.semanticCandidates,
            })}`,
          ),
        ]),
      ),
    ),
  ]);
}
