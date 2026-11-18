import m, { type FactoryComponent } from 'mithril';
import { ModalPanel, Skeleton } from 'mithril-materialized';
import type { SemanticClient } from '../../api/client/semantic-client.ts';
import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import { checkIcon, copyIcon } from '../../components/tabler-icons';
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
import { documentSummaryActionKey, documentSummaryKind } from '../preview/document-summary-support';
import { commentStyleFor, insertCommentary } from './document-commentary';
import {
  blocksGeneration,
  checkProfileModel,
  type ProfileModelStatus,
  profileModelNotice,
} from './profile-model-check';

/** Largest file the dialog reads back to add commentary; larger files are left alone. */
const MAX_COMMENTARY_FILE_BYTES = 1024 * 1024;

export interface DocumentSummaryDialogAttrs {
  readonly open: boolean;
  readonly client: SemanticClient & Pick<WorkspaceClient, 'loadEditableFile' | 'saveEditableFile'>;
  /** Called after commentary was written, so open viewers and listings show the new file. */
  readonly onWritten?: (location: EntrySummary['location']) => void;
  readonly workspaceId: WorkspaceId | string;
  readonly entry: EntrySummary | undefined;
  readonly onClose: () => void;
}

const REPRESENTATIVE_TOKEN_BUDGET = 4_096;
const MAX_SUMMARY_INPUT_TOKENS = 131_072;
const PROMPT_TOKEN_RESERVE = 1_024;

/** Plain text of a generated summary, for the clipboard. */
export function summaryAsText(
  summary: DocumentSummary,
  labels: { title: string; authors: string; brief: string; full: string },
): string {
  const parts: string[] = [];
  if (summary.title) parts.push(`${labels.title}: ${summary.title.value}`);
  const authors = (summary.authors ?? []).map((author) => author.value);
  if (authors.length > 0) parts.push(`${labels.authors}: ${authors.join(', ')}`);
  parts.push(`${labels.brief}\n${summary.brief}`, `${labels.full}\n${summary.full}`);
  return parts.join('\n\n');
}

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
    typeof preview.textPreview !== 'string' ||
    preview.textPreview.length === 0 ||
    typeof preview.textPreviewTruncated !== 'boolean' ||
    (profileId !== undefined &&
      (!Number.isSafeInteger(preview.sourceTokens) ||
        preview.sourceTokens <= 0 ||
        !Number.isSafeInteger(preview.sourcePassCount) ||
        preview.sourcePassCount <= 0 ||
        !Number.isSafeInteger(preview.effectiveContextWindow) ||
        (preview.effectiveContextWindow ?? 0) <= 0))
  ) {
    throw new Error(t('documentSummary', 'invalidPreview'));
  }
  return preview;
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
  /** Whether the profile's model exists on its AI service (checked when the dialog loads). */
  let profileModelStatus: ProfileModelStatus | undefined;
  let includeImages = false;
  let preview: DocumentSummaryPreview | undefined;
  let summary: DocumentSummary | null = null;
  let writing = false;
  let writeStatus: string | undefined;
  let writeError: string | undefined;

  async function load(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    const requestTarget = target(attrs);
    if (requestTarget === undefined) return;
    busy = true;
    error = undefined;
    try {
      profiles = await attrs.client.listLlmProfiles();
      selectedProfileId = profiles[0]?.id;
      const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId);
      const [status, loadedPreview, loadedSummary] = await Promise.all([
        selectedProfile === undefined
          ? Promise.resolve(undefined)
          : checkProfileModel(attrs.client, selectedProfile),
        attrs.client.previewDocumentSummary({
          target: requestTarget,
          inputTokenBudget: inputTokenBudget(selectedProfile),
          profileId: selectedProfileId ?? null,
          includeImages,
        }),
        attrs.client.getDocumentSummary({ target: requestTarget }),
      ]);
      profileModelStatus = status;
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

  function summaryLabels(): { title: string; authors: string; brief: string; full: string } {
    return {
      title: t('documentSummary', 'extractedTitle'),
      authors: t('documentSummary', 'extractedAuthors'),
      brief: t('documentSummary', 'brief'),
      full: t('documentSummary', 'full'),
    };
  }

  /** Which copy button was just used, to flash "Copied" on it. */
  let copiedKey: string | undefined;

  async function copyText(key: string, text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      copiedKey = key;
    } catch {
      copiedKey = undefined;
    }
    m.redraw();
    setTimeout(() => {
      if (copiedKey === key) copiedKey = undefined;
      m.redraw();
    }, 1500);
  }

  /** Clipboard icon button; the check mark confirms the copy for a moment. */
  function copyButton(key: string, text: string, label: string): m.Vnode {
    const copied = copiedKey === key;
    return m(
      'button.fm-document-summary-copy',
      {
        type: 'button',
        title: copied ? t('documentSummary', 'copied') : label,
        'aria-label': copied ? t('documentSummary', 'copied') : label,
        onclick: () => void copyText(key, text),
      },
      copied ? checkIcon({ size: 15 }) : copyIcon({ size: 15 }),
    );
  }

  /** Code and plain or Markdown text can take a comment; other formats cannot be written back. */
  function writable(attrs: DocumentSummaryDialogAttrs): boolean {
    return attrs.entry !== undefined && commentStyleFor(attrs.entry) !== undefined;
  }

  function writeCommentaryControls(attrs: DocumentSummaryDialogAttrs): m.Children {
    return m('.fm-document-summary-write', [
      m(
        'button.fm-document-summary-write-start',
        {
          type: 'button',
          disabled: writing,
          onclick: () => void writeCommentary(attrs),
        },
        t('documentSummary', 'writeToFile'),
      ),
      writeStatus === undefined ? undefined : m('p', { role: 'status' }, writeStatus),
      writeError === undefined
        ? undefined
        : m('p.fm-document-summary-error', { role: 'alert' }, writeError),
    ]);
  }

  async function writeCommentary(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    const entry = attrs.entry;
    if (entry === undefined || summary === null) return;
    const style = commentStyleFor(entry);
    if (style === undefined) return;
    writing = true;
    writeError = undefined;
    writeStatus = undefined;
    m.redraw();
    try {
      const loaded = await attrs.client.loadEditableFile({ location: entry.location });
      if (loaded.size > MAX_COMMENTARY_FILE_BYTES) {
        throw new Error(t('documentSummary', 'writeFailed'));
      }
      const content = insertCommentary(
        loaded.content,
        summaryAsText(summary, summaryLabels()),
        style,
      );
      await attrs.client.saveEditableFile({
        location: entry.location,
        content,
        expectedRevision: loaded.revision,
        overwriteConflict: false,
      });
      writeStatus = t('documentSummary', 'writtenToFile');
      attrs.onWritten?.(entry.location);
    } catch (cause) {
      writeError = summaryErrorMessage(cause, t('documentSummary', 'writeFailed'));
    } finally {
      writing = false;
      m.redraw();
    }
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

  /** Opening the dialog generates at once when nothing is stored yet, and the model is ready. */
  async function openAndGenerate(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    await load(attrs);
    if (
      summary === null &&
      preview !== undefined &&
      selectedProfileId !== undefined &&
      !blocksGeneration(profileModelStatus)
    )
      await generate(attrs);
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) {
        writeStatus = undefined;
        writeError = undefined;
        void openAndGenerate(attrs);
      }
      wasOpen = attrs.open;
    },
    view: ({ attrs }) => {
      const isCode = attrs.entry !== undefined && documentSummaryKind(attrs.entry) === 'sourceCode';
      return m(ModalPanel, {
        title: isCode
          ? t('documentSummary', 'codeTitle', { name: attrs.entry?.name ?? '' })
          : t('documentSummary', 'title', { name: attrs.entry?.name ?? '' }),
        className: 'fm-dense-modal fm-document-summary-modal',
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onClose();
        },
        description: m('.fm-document-summary', [
          busy && !generating
            ? m('.fm-document-summary-progress', { role: 'status', 'aria-busy': 'true' }, [
                m('span', t('documentSummary', 'working')),
                m(Skeleton, { count: 4, className: 'fm-document-summary-skeleton' }),
              ])
            : undefined,
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
          !busy && profiles.length === 0
            ? m('p', t('documentSummary', 'profileRequired'))
            : undefined,
          profiles[0] === undefined ||
          profileModelNotice(profiles[0], profileModelStatus) === undefined
            ? undefined
            : m(
                'p.fm-document-summary-error',
                { role: 'alert' },
                profileModelNotice(profiles[0], profileModelStatus),
              ),
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
                m('.fm-document-summary-result-actions', [
                  copyButton(
                    'all',
                    summaryAsText(summary, summaryLabels()),
                    t('documentSummary', 'copyAll'),
                  ),
                ]),
                // Code has no title or byline to extract, so only prose shows them.
                isCode
                  ? undefined
                  : [
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
                    ],
                m('.fm-document-summary-heading', [
                  m('h4', t('documentSummary', 'brief')),
                  copyButton('brief', summary.brief, t('documentSummary', 'copy')),
                ]),
                m('p', summary.brief),
                m('.fm-document-summary-heading', [
                  m('h4', t('documentSummary', 'full')),
                  copyButton('full', summary.full, t('documentSummary', 'copy')),
                ]),
                m('p', summary.full),
                writable(attrs) ? writeCommentaryControls(attrs) : undefined,
              ]),
        ]),
        buttons: [
          { label: t('button', 'close'), onclick: attrs.onClose },
          ...(summary === null
            ? []
            : [
                {
                  label:
                    copiedKey === 'all'
                      ? t('documentSummary', 'copied')
                      : t('documentSummary', 'copyAll'),
                  onclick: () => {
                    if (summary !== null)
                      void copyText('all', summaryAsText(summary, summaryLabels()));
                  },
                },
              ]),
          // Regenerating a current summary from the same profile would only repeat it, so the
          // action appears when the source changed or the summary came from another profile.
          ...(selectedProfileId === undefined ||
          preview === undefined ||
          (summary !== null && !summary.stale && summary.profileId === selectedProfileId)
            ? []
            : [
                {
                  label:
                    summary === null
                      ? t('documentSummary', documentSummaryActionKey(attrs.entry))
                      : t('documentSummary', 'regenerate'),
                  disabled: busy || blocksGeneration(profileModelStatus),
                  onclick: () => void generate(attrs),
                },
              ]),
        ],
      });
    },
  };
};
