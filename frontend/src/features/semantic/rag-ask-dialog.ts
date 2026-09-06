import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  EntrySummary,
  LlmProfile,
  Location,
  RagAnswer,
  RagPreview,
  RagScope,
  RagScopeKind,
  SavedRagConversation,
  SemanticRootStatus,
} from '../../models';

export interface RagAskDialogAttrs {
  readonly open: boolean;
  readonly client: FileManagerClient;
  readonly workspaceId: string;
  readonly currentFolder: Location | undefined;
  readonly selectedEntries: readonly EntrySummary[];
  readonly semanticSourceIds: readonly string[];
  readonly onClose: () => void;
  readonly onIncludeCurrentFolder?: () => void;
  readonly onOpenCitation?: (sourceId: string) => void | Promise<void>;
}

function scopeLabel(kind: RagScopeKind): string {
  switch (kind) {
    case 'entireLibrary':
      return t('ragAsk', 'scopeEntireLibrary');
    case 'selectedFiles':
      return t('ragAsk', 'scopeSelectedFiles');
    case 'currentFolder':
      return t('ragAsk', 'scopeCurrentFolder');
    case 'semanticResults':
      return t('ragAsk', 'scopeSemanticResults');
    case 'enrolledRoots':
      return t('ragAsk', 'scopeEnrolledRoots');
  }
}

function buildScope(
  attrs: RagAskDialogAttrs,
  kind: RagScopeKind,
  roots: readonly SemanticRootStatus[],
): RagScope {
  return {
    workspaceId: attrs.workspaceId,
    kind,
    label: scopeLabel(kind),
    selectedFiles:
      kind === 'selectedFiles'
        ? attrs.selectedEntries
            .filter((entry) => entry.kind === 'file')
            .map((entry) => ({
              workspaceId: attrs.workspaceId,
              entryId: entry.id,
              location: entry.location,
            }))
        : [],
    folder: kind === 'currentFolder' ? (attrs.currentFolder ?? null) : null,
    semanticSourceIds: kind === 'semanticResults' ? [...attrs.semanticSourceIds] : [],
    enrolledRootIds: kind === 'enrolledRoots' ? roots.map((root) => root.id) : [],
  };
}

function scopeAvailable(
  attrs: RagAskDialogAttrs,
  kind: RagScopeKind,
  roots: readonly SemanticRootStatus[],
  currentFolderIncluded: boolean,
): boolean {
  switch (kind) {
    case 'entireLibrary':
      return true;
    case 'selectedFiles':
      return attrs.selectedEntries.some((entry) => entry.kind === 'file');
    case 'currentFolder':
      return attrs.currentFolder !== undefined && currentFolderIncluded;
    case 'semanticResults':
      return attrs.semanticSourceIds.length > 0;
    case 'enrolledRoots':
      return roots.length > 0;
  }
}

function coverageText(preview: RagPreview): string {
  const coverage = preview.coverage;
  return t('ragAsk', 'coverage', {
    indexed: coverage.indexed,
    eligible: coverage.eligible,
    pending: coverage.pending,
    stale: coverage.stale,
    failed: coverage.failed,
    excluded: coverage.excluded,
    unavailable: coverage.unavailable,
  });
}

export const RagAskDialog: FactoryComponent<RagAskDialogAttrs> = () => {
  let wasOpen = false;
  let busy: 'loading' | 'retrieving' | 'generating' | 'saving' | undefined;
  let error: string | undefined;
  let profiles: readonly LlmProfile[] = [];
  let roots: readonly SemanticRootStatus[] = [];
  let currentFolderIncluded = false;
  let currentFolderStatusError = false;
  let selectedProfileId = '';
  let selectedScope: RagScopeKind = 'entireLibrary';
  let question = '';
  let allowModelKnowledge = false;
  let preview: RagPreview | undefined;
  let answer: RagAnswer | undefined;
  let streamedText = '';
  let conversationId: string | undefined;
  let saved: readonly SavedRagConversation[] = [];
  let exportVisible = false;
  let abortController: AbortController | undefined;

  async function load(attrs: RagAskDialogAttrs): Promise<void> {
    busy = 'loading';
    error = undefined;
    currentFolderIncluded = false;
    currentFolderStatusError = false;
    try {
      const [availableProfiles, library, conversations] = await Promise.all([
        attrs.client.listLlmProfiles(),
        attrs.client.getSemanticLibraryStatus(),
        attrs.client.listSavedRagConversations(attrs.workspaceId),
      ]);
      profiles = availableProfiles;
      selectedProfileId = profiles[0]?.id ?? '';
      roots = library.roots.filter((root) => root.workspaceReferences.includes(attrs.workspaceId));
      saved = conversations;
      if (attrs.currentFolder !== undefined) {
        try {
          const folder = await attrs.client.getSemanticFolderStatus({
            workspaceId: attrs.workspaceId,
            location: attrs.currentFolder,
          });
          currentFolderIncluded =
            (folder.consent === 'includedHere' || folder.consent === 'inheritedFromParent') &&
            folder.workspaceReferenced;
        } catch {
          currentFolderStatusError = true;
        }
      }
    } catch {
      error = t('ragAsk', 'loadFailed');
    } finally {
      busy = undefined;
      m.redraw();
    }
  }

  function resetRetrieval(clearConversation = false): void {
    preview = undefined;
    answer = undefined;
    streamedText = '';
    if (clearConversation) conversationId = undefined;
    exportVisible = false;
  }

  async function retrieve(attrs: RagAskDialogAttrs): Promise<void> {
    if (selectedProfileId === '' || question.trim() === '') return;
    busy = 'retrieving';
    error = undefined;
    resetRetrieval();
    abortController = new AbortController();
    try {
      preview = await attrs.client.previewRag(
        {
          profileId: selectedProfileId,
          question: question.trim(),
          scope: buildScope(attrs, selectedScope, roots),
        },
        abortController.signal,
      );
    } catch (cause) {
      if (!(cause instanceof DOMException && cause.name === 'AbortError')) {
        error = t('ragAsk', 'retrievalFailed');
      }
    } finally {
      busy = undefined;
      abortController = undefined;
      m.redraw();
    }
  }

  async function generate(attrs: RagAskDialogAttrs): Promise<void> {
    if (preview === undefined) return;
    busy = 'generating';
    error = undefined;
    streamedText = '';
    answer = undefined;
    abortController = new AbortController();
    try {
      const response = await attrs.client.generateRagAnswer(
        {
          profileId: selectedProfileId,
          question: question.trim(),
          scope: preview.scope,
          expectedRetrievalFingerprint: preview.retrievalFingerprint,
          allowModelKnowledge,
          conversationId: conversationId ?? null,
        },
        abortController.signal,
      );
      conversationId = response.conversationId;
      for (const event of response.events) {
        if (event.type === 'retrieval') preview = event.preview;
        if (event.type === 'token') streamedText += event.text;
        if (event.type === 'done') answer = event.answer;
        m.redraw();
      }
    } catch (cause) {
      if (!(cause instanceof DOMException && cause.name === 'AbortError')) {
        error = t('ragAsk', 'generationFailed');
      }
    } finally {
      busy = undefined;
      abortController = undefined;
      m.redraw();
    }
  }

  async function save(attrs: RagAskDialogAttrs): Promise<void> {
    if (conversationId === undefined) return;
    busy = 'saving';
    error = undefined;
    try {
      const conversation = await attrs.client.saveRagConversation({
        conversationId,
        workspaceId: attrs.workspaceId,
      });
      saved = [conversation, ...saved.filter((item) => item.id !== conversation.id)];
    } catch {
      error = t('ragAsk', 'saveFailed');
    } finally {
      busy = undefined;
      m.redraw();
    }
  }

  async function removeSaved(attrs: RagAskDialogAttrs, id: string): Promise<void> {
    error = undefined;
    try {
      await attrs.client.deleteRagConversation({ conversationId: id });
      saved = saved.filter((conversation) => conversation.id !== id);
    } catch {
      error = t('ragAsk', 'deleteFailed');
    } finally {
      m.redraw();
    }
  }

  const scopeKinds: readonly RagScopeKind[] = [
    'entireLibrary',
    'selectedFiles',
    'currentFolder',
    'semanticResults',
    'enrolledRoots',
  ];

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) void load(attrs);
      wasOpen = attrs.open;
    },
    onremove: () => abortController?.abort(),
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('ragAsk', 'title'),
        className: 'fm-rag-ask-modal',
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onClose();
        },
        description: m('.fm-rag-ask', [
          error === undefined ? undefined : m('p.fm-rag-error', { role: 'alert' }, error),
          m('label.fm-rag-question', [
            m('span', t('ragAsk', 'question')),
            m('textarea', {
              rows: 5,
              value: question,
              disabled: busy !== undefined,
              autofocus: true,
              oninput: (event: InputEvent) => {
                question = (event.currentTarget as HTMLTextAreaElement).value;
                resetRetrieval();
              },
            }),
          ]),
          m('details.fm-rag-options', [
            m('summary', [
              m('span', t('ragAsk', 'options')),
              m('small', scopeLabel(selectedScope)),
            ]),
            m('.fm-rag-controls', [
              m('label', [
                m('span', t('ragAsk', 'profile')),
                m(
                  'select.browser-default',
                  {
                    value: selectedProfileId,
                    disabled: busy !== undefined,
                    onchange: (event: Event) => {
                      selectedProfileId = (event.currentTarget as HTMLSelectElement).value;
                      resetRetrieval(true);
                    },
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
              m('label', [
                m('span', t('ragAsk', 'scope')),
                m(
                  'select.browser-default',
                  {
                    value: selectedScope,
                    disabled: busy !== undefined,
                    onchange: (event: Event) => {
                      selectedScope = (event.currentTarget as HTMLSelectElement)
                        .value as RagScopeKind;
                      resetRetrieval(true);
                    },
                  },
                  scopeKinds.map((kind) =>
                    m(
                      'option',
                      {
                        key: kind,
                        value: kind,
                        disabled: !scopeAvailable(attrs, kind, roots, currentFolderIncluded),
                      },
                      scopeLabel(kind),
                    ),
                  ),
                ),
              ]),
              m('p.fm-rag-disclosure', t('ragAsk', 'readOnlyDisclosure')),
            ]),
          ]),
          preview === undefined
            ? undefined
            : m(
                'details.fm-rag-preview',
                { open: answer === undefined, 'aria-labelledby': 'rag-evidence-heading' },
                [
                  m('summary#rag-evidence-heading', t('ragAsk', 'evidence')),
                  m(
                    'p.fm-rag-disclosure',
                    preview.locality === 'cloud'
                      ? t('ragAsk', 'cloudDisclosure', { tokens: preview.evidenceTokens })
                      : t('ragAsk', 'localDisclosure', { tokens: preview.evidenceTokens }),
                  ),
                  m('p', { role: 'status' }, coverageText(preview)),
                  preview.insufficient
                    ? m('p.fm-rag-warning', t('ragAsk', 'insufficient'))
                    : undefined,
                  m(
                    'ol.fm-rag-evidence',
                    preview.evidence.map((item) =>
                      m('li', { key: item.label }, [
                        m('strong', `${item.label}${item.title == null ? '' : ` · ${item.title}`}`),
                        m('p', item.excerpt),
                        m(
                          'small',
                          [
                            item.sectionPath.join(' / '),
                            item.generated ? t('ragAsk', 'generatedEvidence') : undefined,
                            item.stale ? t('ragAsk', 'staleEvidence') : undefined,
                            !item.available ? t('ragAsk', 'unavailableEvidence') : undefined,
                          ]
                            .filter((value): value is string => value !== undefined && value !== '')
                            .join(' · '),
                        ),
                      ]),
                    ),
                  ),
                ],
              ),
          m('section.fm-rag-answer', { 'aria-live': 'polite' }, [
            m('h4', t('ragAsk', 'answer')),
            streamedText === '' && answer === undefined
              ? m('p.fm-rag-answer-placeholder', t('ragAsk', 'answerPlaceholder'))
              : m('p', answer?.text ?? streamedText),
            answer?.modelKnowledgeAllowed === true
              ? m('p.fm-rag-warning', t('ragAsk', 'modelKnowledgeUsed'))
              : undefined,
            m(
              'ul.fm-rag-citations',
              answer?.citations.map((citation) =>
                m('li', { key: `${citation.label}-${citation.sourceId}` }, [
                  m(
                    'button',
                    {
                      type: 'button',
                      disabled: citation.unavailable || attrs.onOpenCitation === undefined,
                      onclick: () => {
                        error = undefined;
                        void Promise.resolve(attrs.onOpenCitation?.(citation.sourceId)).catch(
                          () => {
                            error = t('ragAsk', 'citationFailed');
                            m.redraw();
                          },
                        );
                      },
                      'aria-label': t('ragAsk', 'openCitation', { label: citation.label }),
                    },
                    citation.label,
                  ),
                  ` ${citation.provenance}`,
                  citation.generated ? ` · ${t('ragAsk', 'generatedEvidence')}` : '',
                  citation.stale ? ` · ${t('ragAsk', 'staleEvidence')}` : '',
                  citation.unavailable ? ` · ${t('ragAsk', 'unavailableEvidence')}` : '',
                ]),
              ),
            ),
          ]),
          m('.fm-rag-preferences', [
            attrs.currentFolder === undefined || attrs.onIncludeCurrentFolder === undefined
              ? undefined
              : m('label', [
                  m('input', {
                    type: 'checkbox',
                    checked: currentFolderIncluded,
                    disabled:
                      busy !== undefined || currentFolderStatusError || currentFolderIncluded,
                    onchange: (event: Event) => {
                      if ((event.currentTarget as HTMLInputElement).checked) {
                        attrs.onIncludeCurrentFolder?.();
                      }
                    },
                  }),
                  m('span', t('ragAsk', 'includeCurrentFolder')),
                ]),
            m('label', [
              m('input', {
                type: 'checkbox',
                checked: allowModelKnowledge,
                disabled: busy !== undefined,
                onchange: (event: Event) => {
                  allowModelKnowledge = (event.currentTarget as HTMLInputElement).checked;
                  resetRetrieval(true);
                },
              }),
              m('span', t('ragAsk', 'allowModelKnowledge')),
            ]),
            currentFolderStatusError
              ? m('p.fm-rag-warning', { role: 'alert' }, t('ragAsk', 'folderStatusFailed'))
              : undefined,
          ]),
          exportVisible
            ? m('details.fm-rag-export', { open: true }, [
                m('summary', t('ragAsk', 'exportPreview')),
                m(
                  'pre',
                  JSON.stringify(
                    {
                      question,
                      answer,
                      profile: preview?.profileName,
                      scope: preview?.scope.label,
                      sources: answer?.citations.map(({ label, provenance, generated, stale }) => ({
                        label,
                        provenance,
                        generated,
                        stale,
                      })),
                    },
                    null,
                    2,
                  ),
                ),
              ])
            : undefined,
          saved.length === 0
            ? undefined
            : m('details.fm-rag-saved', [
                m('summary', t('ragAsk', 'savedConversations')),
                m(
                  'ul',
                  saved.map((conversation) =>
                    m('li', { key: conversation.id }, [
                      m(
                        'span',
                        t('ragAsk', 'savedConversation', {
                          turns: conversation.turns.length,
                          bytes: conversation.storageBytes,
                        }),
                      ),
                      m(
                        'button',
                        {
                          type: 'button',
                          onclick: () => void removeSaved(attrs, conversation.id),
                        },
                        t('button', 'delete'),
                      ),
                    ]),
                  ),
                ),
              ]),
        ]),
        buttons: [
          { label: t('button', 'close'), onclick: attrs.onClose },
          ...(busy === 'retrieving' || busy === 'generating'
            ? [{ label: t('button', 'cancel'), onclick: () => abortController?.abort() }]
            : []),
          ...(answer === undefined
            ? []
            : [
                {
                  label: t('ragAsk', 'exportPreview'),
                  onclick: () => {
                    exportVisible = !exportVisible;
                  },
                },
              ]),
          ...(conversationId === undefined
            ? []
            : [
                {
                  label: busy === 'saving' ? t('ragAsk', 'saving') : t('ragAsk', 'save'),
                  disabled: busy !== undefined,
                  onclick: () => void save(attrs),
                },
              ]),
          ...(preview === undefined
            ? [
                {
                  label:
                    busy === 'retrieving' ? t('ragAsk', 'retrieving') : t('ragAsk', 'retrieve'),
                  disabled:
                    busy !== undefined || selectedProfileId === '' || question.trim() === '',
                  onclick: () => void retrieve(attrs),
                },
              ]
            : [
                {
                  label:
                    busy === 'generating' ? t('ragAsk', 'generating') : t('ragAsk', 'generate'),
                  disabled: busy !== undefined,
                  onclick: () => void generate(attrs),
                },
              ]),
        ],
      }),
  };
};
