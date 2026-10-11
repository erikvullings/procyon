import m, { type FactoryComponent } from 'mithril';
import { ModalPanel, Skeleton } from 'mithril-materialized';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  DocumentSummary,
  DocumentSummaryPreview,
  DocumentSummaryTarget,
  EntrySummary,
  LlmProfile,
  SummaryAttribution,
  WorkspaceId,
} from '../../models';

export interface DocumentSummaryDialogAttrs {
  readonly open: boolean;
  readonly client: FileManagerClient;
  readonly workspaceId: WorkspaceId | string;
  readonly entry: EntrySummary | undefined;
  readonly onClose: () => void;
}

const REPRESENTATIVE_TOKEN_BUDGET = 4_096;
const MAX_SUMMARY_INPUT_TOKENS = 131_072;
const PROMPT_TOKEN_RESERVE = 1_024;

function inputTokenBudget(profile: LlmProfile | undefined): number {
  if (profile === undefined) return REPRESENTATIVE_TOKEN_BUDGET;
  return Math.min(
    MAX_SUMMARY_INPUT_TOKENS,
    Math.max(
      1,
      profile.advanced.contextWindow - profile.advanced.maximumAnswerTokens - PROMPT_TOKEN_RESERVE,
    ),
  );
}

function target(attrs: DocumentSummaryDialogAttrs): DocumentSummaryTarget | undefined {
  if (attrs.entry === undefined) return undefined;
  return {
    workspaceId: attrs.workspaceId,
    entryId: attrs.entry.id,
    location: attrs.entry.location,
  };
}

function summaryErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.length > 0) return error.message;
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string' &&
    error.message.length > 0
  ) {
    return error.message;
  }
  return fallback;
}

function validatedPreview(
  preview: DocumentSummaryPreview,
  profileId: string | undefined,
): DocumentSummaryPreview {
  if (
    profileId !== undefined &&
    (!Number.isSafeInteger(preview.sourceTokens) ||
      preview.sourceTokens <= 0 ||
      !Number.isSafeInteger(preview.sourcePassCount) ||
      preview.sourcePassCount <= 0 ||
      !Number.isSafeInteger(preview.effectiveContextWindow) ||
      (preview.effectiveContextWindow ?? 0) <= 0)
  ) {
    throw new Error(t('documentSummary', 'invalidPreview'));
  }
  return preview;
}

function passageBody(passage: DocumentSummaryPreview['keyPassages'][number]): string {
  const heading = passage.sectionPath.join(' > ');
  if (heading.length === 0) return passage.content;
  if (passage.content === heading) return '';
  const prefix = `${heading}\n\n`;
  return passage.content.startsWith(prefix)
    ? passage.content.slice(prefix.length)
    : passage.content;
}

function attributionOrigin(attribution: SummaryAttribution): string {
  switch (attribution.source) {
    case 'metadata':
      return t('documentSummary', 'fromMetadata');
    case 'byline':
      return t('documentSummary', 'fromByline');
    case 'aiExtracted':
      return t('documentSummary', 'aiExtracted');
  }
}

function attributionLocation(attribution: SummaryAttribution): string | undefined {
  if (!attribution.provenance) return undefined;
  try {
    const parsed: unknown = JSON.parse(attribution.provenance);
    if (typeof parsed !== 'object' || parsed === null || !('kind' in parsed)) return undefined;
    const exact = parsed.kind === 'exact' && 'value' in parsed ? parsed.value : parsed;
    if (typeof exact !== 'object' || exact === null) return undefined;
    if ('kind' in exact && exact.kind === 'pdfBlock' && 'page_number' in exact) {
      const page = exact.page_number;
      if (typeof page === 'number') return t('documentSummary', 'sourcePage', { page });
    }
  } catch {
    return undefined;
  }
  return undefined;
}

function attributionText(attribution: SummaryAttribution): string {
  const location = attributionLocation(attribution);
  return `${attribution.value} · ${attributionOrigin(attribution)}${location ? ` · ${location}` : ''}`;
}

function attributionQuote(attribution: SummaryAttribution): m.Children {
  return attribution.quote
    ? m('details.fm-document-summary-source', [
        m('summary', t('documentSummary', 'sourceExcerpt')),
        m('blockquote', attribution.quote),
      ])
    : undefined;
}

export const DocumentSummaryDialog: FactoryComponent<DocumentSummaryDialogAttrs> = () => {
  let wasOpen = false;
  let busy = false;
  let generating = false;
  let error: string | undefined;
  let profiles: readonly LlmProfile[] = [];
  let selectedProfileId: string | undefined;
  let includeImages = false;
  let preview: DocumentSummaryPreview | undefined;
  let summary: DocumentSummary | null = null;

  async function load(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    const requestTarget = target(attrs);
    if (requestTarget === undefined) return;
    busy = true;
    error = undefined;
    try {
      profiles = await attrs.client.listLlmProfiles();
      selectedProfileId = profiles[0]?.id;
      const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId);
      const [loadedPreview, loadedSummary] = await Promise.all([
        attrs.client.previewDocumentSummary({
          target: requestTarget,
          inputTokenBudget: inputTokenBudget(selectedProfile),
          profileId: selectedProfileId ?? null,
          includeImages,
        }),
        attrs.client.getDocumentSummary({ target: requestTarget }),
      ]);
      preview = validatedPreview(loadedPreview, selectedProfileId);
      summary = loadedSummary;
    } catch (cause) {
      preview = undefined;
      error = summaryErrorMessage(cause, t('documentSummary', 'loadFailed'));
    } finally {
      busy = false;
      m.redraw();
    }
  }

  async function refreshPreview(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    const requestTarget = target(attrs);
    if (requestTarget === undefined) return;
    const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId);
    busy = true;
    error = undefined;
    try {
      preview = validatedPreview(
        await attrs.client.previewDocumentSummary({
          target: requestTarget,
          inputTokenBudget: inputTokenBudget(selectedProfile),
          profileId: selectedProfileId ?? null,
          includeImages,
        }),
        selectedProfileId,
      );
    } catch (cause) {
      preview = undefined;
      error = summaryErrorMessage(cause, t('documentSummary', 'loadFailed'));
    } finally {
      busy = false;
      m.redraw();
    }
  }

  async function changeImageMode(
    attrs: DocumentSummaryDialogAttrs,
    enabled: boolean,
  ): Promise<void> {
    includeImages = enabled;
    await refreshPreview(attrs);
  }

  async function generate(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    const requestTarget = target(attrs);
    if (requestTarget === undefined || selectedProfileId === undefined || preview === undefined)
      return;
    busy = true;
    generating = true;
    error = undefined;
    try {
      const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId);
      summary = await attrs.client.generateDocumentSummary({
        target: requestTarget,
        inputTokenBudget: inputTokenBudget(selectedProfile),
        expectedSelectionFingerprint: preview.selectionFingerprint,
        profileId: selectedProfileId,
        includeImages,
      });
    } catch (cause) {
      error = summaryErrorMessage(cause, t('documentSummary', 'generationFailed'));
    } finally {
      busy = false;
      generating = false;
      m.redraw();
    }
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) void load(attrs);
      wasOpen = attrs.open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('documentSummary', 'title', { name: attrs.entry?.name ?? '' }),
        className: 'fm-dense-modal fm-document-summary-modal',
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onClose();
        },
        description: m('.fm-document-summary', [
          busy && !generating ? m('p', t('documentSummary', 'working')) : undefined,
          generating
            ? m('.fm-document-summary-progress', { role: 'status', 'aria-busy': 'true' }, [
                m(
                  'span',
                  t('documentSummary', 'generatingUsingProfile', {
                    name: preview?.profile?.profileName ?? '',
                    model: preview?.profile?.modelId ?? '',
                  }),
                ),
                m(Skeleton, { count: 4, className: 'fm-document-summary-skeleton' }),
              ])
            : undefined,
          error === undefined ? undefined : m('p.fm-document-summary-error', error),
          profiles.length === 0 ? m('p', t('documentSummary', 'keyPassagesOnly')) : undefined,
          preview?.imageInputAvailable !== true
            ? undefined
            : m('label.fm-document-summary-image-mode', [
                m('input', {
                  type: 'checkbox',
                  checked: includeImages,
                  disabled: busy,
                  onchange: (event: Event) =>
                    void changeImageMode(attrs, (event.currentTarget as HTMLInputElement).checked),
                }),
                m('span', t('documentSummary', 'includeImages')),
              ]),
          preview?.profile?.locality === 'cloud'
            ? m('p.fm-document-summary-disclosure', t('documentSummary', 'cloudDisclosure'))
            : undefined,
          generating && preview !== undefined && preview.sourcePassCount > 1
            ? m('p.fm-document-summary-disclosure', t('documentSummary', 'summarizingInParts'))
            : undefined,
          preview === undefined || !includeImages
            ? undefined
            : m(
                'p.fm-document-summary-disclosure',
                preview.includedImageCount > 0
                  ? preview.omittedImageCount > 0
                    ? t('documentSummary', 'imagesIncludedWithOmissions', {
                        count: preview.includedImageCount,
                        omitted: preview.omittedImageCount,
                      })
                    : t('documentSummary', 'imagesIncluded', {
                        count: preview.includedImageCount,
                      })
                  : t('documentSummary', 'imagesOmitted'),
              ),
          summary === null
            ? undefined
            : m('.fm-document-summary-result', [
                summary.stale
                  ? m('p.fm-document-summary-stale', t('documentSummary', 'stale'))
                  : undefined,
                m('h4', t('documentSummary', 'extractedTitle')),
                summary.title
                  ? m('div', [
                      m('p', attributionText(summary.title)),
                      attributionQuote(summary.title),
                    ])
                  : m('p', t('documentSummary', 'unavailable')),
                m('h4', t('documentSummary', 'extractedAuthors')),
                summary.authors?.length
                  ? m(
                      'ul',
                      summary.authors.map((author) =>
                        m('li', [attributionText(author), attributionQuote(author)]),
                      ),
                    )
                  : m('p', t('documentSummary', 'unavailable')),
                m('h4', t('documentSummary', 'brief')),
                m('p', summary.brief),
                m('h4', t('documentSummary', 'full')),
                m('p', summary.full),
              ]),
          m(
            'h4',
            t(
              'documentSummary',
              preview?.selectionMode === 'fullDocument' ? 'documentText' : 'keyPassages',
            ),
          ),
          m(
            'ol.fm-document-summary-passages',
            preview?.keyPassages.map((passage) => {
              const body = passageBody(passage);
              return m('li', { key: passage.chunkId }, [
                passage.sectionPath.length > 0
                  ? m('strong', passage.sectionPath.join(' / '))
                  : undefined,
                body ? m('p', body) : undefined,
              ]);
            }),
          ),
        ]),
        buttons: [
          { label: t('button', 'close'), onclick: attrs.onClose },
          ...(selectedProfileId === undefined || preview === undefined
            ? []
            : [
                {
                  label:
                    summary === null
                      ? t('documentSummary', 'generate')
                      : t('documentSummary', 'regenerate'),
                  disabled: busy,
                  onclick: () => void generate(attrs),
                },
              ]),
        ],
      }),
  };
};
