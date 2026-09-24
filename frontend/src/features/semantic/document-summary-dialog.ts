import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  DocumentSummary,
  DocumentSummaryPreview,
  DocumentSummaryTarget,
  EntrySummary,
  LlmProfile,
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

function inputTokenBudget(profile: LlmProfile | undefined, representativeOnly: boolean): number {
  if (representativeOnly || profile === undefined) return REPRESENTATIVE_TOKEN_BUDGET;
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

export const DocumentSummaryDialog: FactoryComponent<DocumentSummaryDialogAttrs> = () => {
  let wasOpen = false;
  let busy = false;
  let error: string | undefined;
  let profiles: readonly LlmProfile[] = [];
  let selectedProfileId: string | undefined;
  let representativeOnly = false;
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
      [preview, summary] = await Promise.all([
        attrs.client.previewDocumentSummary({
          target: requestTarget,
          inputTokenBudget: inputTokenBudget(selectedProfile, representativeOnly),
          profileId: selectedProfileId ?? null,
          includeImages,
        }),
        attrs.client.getDocumentSummary({ target: requestTarget }),
      ]);
    } catch (cause) {
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
      preview = await attrs.client.previewDocumentSummary({
        target: requestTarget,
        inputTokenBudget: inputTokenBudget(selectedProfile, representativeOnly),
        profileId: selectedProfileId ?? null,
        includeImages,
      });
    } catch (cause) {
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

  async function changeInputMode(
    attrs: DocumentSummaryDialogAttrs,
    useRepresentativePassages: boolean,
  ): Promise<void> {
    representativeOnly = useRepresentativePassages;
    await refreshPreview(attrs);
  }

  async function generate(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    const requestTarget = target(attrs);
    if (requestTarget === undefined || selectedProfileId === undefined || preview === undefined)
      return;
    busy = true;
    error = undefined;
    try {
      const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId);
      summary = await attrs.client.generateDocumentSummary({
        target: requestTarget,
        inputTokenBudget: inputTokenBudget(selectedProfile, representativeOnly),
        expectedSelectionFingerprint: preview.selectionFingerprint,
        profileId: selectedProfileId,
        includeImages,
      });
    } catch (cause) {
      error = summaryErrorMessage(cause, t('documentSummary', 'generationFailed'));
    } finally {
      busy = false;
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
          busy ? m('p', t('documentSummary', 'working')) : undefined,
          error === undefined ? undefined : m('p.fm-document-summary-error', error),
          profiles.length === 0
            ? m('p', t('documentSummary', 'keyPassagesOnly'))
            : preview?.profile == null
              ? undefined
              : m('p.fm-document-summary-profile', [
                  m('strong', `${t('documentSummary', 'profile')}: `),
                  `${preview.profile.profileName} · ${preview.profile.modelId} · ${
                    preview.profile.locality === 'loopback'
                      ? t('llmProfiles', 'local')
                      : t('llmProfiles', 'cloud')
                  }`,
                ]),
          profiles.length === 0
            ? undefined
            : m('label.fm-document-summary-input-mode', [
                m('input', {
                  type: 'checkbox',
                  checked: representativeOnly,
                  disabled: busy,
                  onchange: (event: Event) =>
                    void changeInputMode(attrs, (event.currentTarget as HTMLInputElement).checked),
                }),
                m('span', t('documentSummary', 'representativeOnly')),
              ]),
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
          preview?.profile == null
            ? undefined
            : m(
                'p.fm-document-summary-disclosure',
                t(
                  'documentSummary',
                  preview.selectionMode === 'fullDocument'
                    ? preview.profile.locality === 'cloud'
                      ? 'cloudFullDisclosure'
                      : 'localFullDisclosure'
                    : representativeOnly
                      ? preview.profile.locality === 'cloud'
                        ? 'cloudRepresentativeDisclosure'
                        : 'localRepresentativeDisclosure'
                      : preview.profile.locality === 'cloud'
                        ? 'cloudFallbackDisclosure'
                        : 'localFallbackDisclosure',
                  { tokens: preview.representativeTokens },
                ),
              ),
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
            preview?.keyPassages.map((passage) =>
              m('li', { key: passage.chunkId }, [
                m('strong', `${passage.label} · ${passage.sectionPath.join(' / ')}`),
                m('p', passage.content),
              ]),
            ),
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
