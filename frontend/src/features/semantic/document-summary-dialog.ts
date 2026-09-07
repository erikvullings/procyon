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

const INPUT_TOKEN_BUDGET = 4_096;

function target(attrs: DocumentSummaryDialogAttrs): DocumentSummaryTarget | undefined {
  if (attrs.entry === undefined) return undefined;
  return {
    workspaceId: attrs.workspaceId,
    entryId: attrs.entry.id,
    location: attrs.entry.location,
  };
}

export const DocumentSummaryDialog: FactoryComponent<DocumentSummaryDialogAttrs> = () => {
  let wasOpen = false;
  let busy = false;
  let error: string | undefined;
  let profiles: readonly LlmProfile[] = [];
  let selectedProfileId: string | undefined;
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
      [preview, summary] = await Promise.all([
        attrs.client.previewDocumentSummary({
          target: requestTarget,
          inputTokenBudget: INPUT_TOKEN_BUDGET,
          profileId: selectedProfileId ?? null,
        }),
        attrs.client.getDocumentSummary({ target: requestTarget }),
      ]);
    } catch {
      error = t('documentSummary', 'loadFailed');
    } finally {
      busy = false;
      m.redraw();
    }
  }

  async function changeProfile(
    attrs: DocumentSummaryDialogAttrs,
    profileId: string,
  ): Promise<void> {
    const requestTarget = target(attrs);
    if (requestTarget === undefined) return;
    selectedProfileId = profileId === '' ? undefined : profileId;
    busy = true;
    error = undefined;
    try {
      preview = await attrs.client.previewDocumentSummary({
        target: requestTarget,
        inputTokenBudget: INPUT_TOKEN_BUDGET,
        profileId: selectedProfileId ?? null,
      });
    } catch {
      error = t('documentSummary', 'loadFailed');
    } finally {
      busy = false;
      m.redraw();
    }
  }

  async function generate(attrs: DocumentSummaryDialogAttrs): Promise<void> {
    const requestTarget = target(attrs);
    if (requestTarget === undefined || selectedProfileId === undefined || preview === undefined)
      return;
    busy = true;
    error = undefined;
    try {
      summary = await attrs.client.generateDocumentSummary({
        target: requestTarget,
        inputTokenBudget: INPUT_TOKEN_BUDGET,
        expectedSelectionFingerprint: preview.selectionFingerprint,
        profileId: selectedProfileId,
      });
    } catch {
      error = t('documentSummary', 'generationFailed');
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
            : m('label', [
                m('span', t('documentSummary', 'profile')),
                m(
                  'select.browser-default',
                  {
                    value: selectedProfileId ?? '',
                    disabled: busy,
                    onchange: (event: Event) =>
                      void changeProfile(attrs, (event.currentTarget as HTMLSelectElement).value),
                  },
                  profiles.map((profile) =>
                    m(
                      'option',
                      { key: profile.id, value: profile.id },
                      `${profile.name} · ${profile.locality}`,
                    ),
                  ),
                ),
              ]),
          preview?.profile == null
            ? undefined
            : m(
                'p.fm-document-summary-disclosure',
                preview?.profile?.locality === 'cloud'
                  ? t('documentSummary', 'cloudDisclosure', {
                      tokens: preview.representativeTokens,
                    })
                  : t('documentSummary', 'localDisclosure', {
                      tokens: preview.representativeTokens,
                    }),
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
          m('h4', t('documentSummary', 'keyPassages')),
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
