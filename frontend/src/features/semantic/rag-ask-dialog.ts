import m, { type FactoryComponent } from 'mithril';
import { IconButton, ModalPanel } from 'mithril-materialized';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { copyIcon, plusIcon } from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type {
  EntrySummary,
  LlmProfile,
  Location,
  RagAnswer,
  RagCitation,
  RagPreview,
  RagScope,
  RagScopeKind,
  SavedRagConversation,
  SemanticRootStatus,
} from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { copyText } from '../preview/clipboard';

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

function decodeEvidenceTitle(title: string | null | undefined): string | undefined {
  if (title == null) return undefined;
  try {
    return decodeURIComponent(title);
  } catch {
    return title;
  }
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function linkAnswerCitations(markdown: string, citations: readonly RagCitation[]): string {
  return citations.reduce((linked, citation) => {
    const label = escapeRegExp(citation.label);
    const target = `#rag-citation-${encodeURIComponent(citation.label)}`;
    return linked
      .replace(new RegExp(`\\[${label}\\](?!\\()`, 'g'), `[${citation.label}](${target})`)
      .replace(new RegExp(`\\(${label}(?=[,\\s)])`, 'g'), `([${citation.label}](${target})`);
  }, markdown);
}

function citationLocation(value: unknown): string | undefined {
  if (typeof value === 'string') {
    try {
      return citationLocation(JSON.parse(value));
    } catch {
      return undefined;
    }
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined;
  const provenance = value as Record<string, unknown>;
  const number = (key: string): number | undefined =>
    typeof provenance[key] === 'number' ? provenance[key] : undefined;
  switch (provenance.kind) {
    case 'exact':
      return citationLocation(provenance.value);
    case 'span': {
      const start = citationLocation(provenance.first);
      const end = citationLocation(provenance.last);
      if (start === undefined) return end;
      if (end === undefined || end === start) return start;
      return `${start} – ${end}`;
    }
    case 'pdfBlock': {
      const page = number('page_number');
      return page === undefined ? undefined : t('ragAsk', 'citationPage', { page });
    }
    case 'textLines':
    case 'codeLines': {
      const start = number('start_line');
      const end = number('end_line');
      return start === undefined || end === undefined
        ? undefined
        : t('ragAsk', 'citationLines', { start, end });
    }
    case 'slide': {
      const slide = number('slide_number');
      return slide === undefined ? undefined : t('ragAsk', 'citationSlide', { slide });
    }
    case 'docxBlock': {
      const block = number('block_index');
      return block === undefined ? undefined : t('ragAsk', 'citationBlock', { block: block + 1 });
    }
    default:
      return undefined;
  }
}

function formatCitationProvenance(provenance: string): string {
  try {
    return citationLocation(JSON.parse(provenance)) ?? '';
  } catch {
    return provenance;
  }
}

export const RagAskDialog: FactoryComponent<RagAskDialogAttrs> = () => {
  let wasOpen = false;
  let focusQuestionOnReady = false;
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

  function startNewQuestion(): void {
    abortController?.abort();
    error = undefined;
    question = '';
    resetRetrieval(true);
    queueMicrotask(() =>
      document.querySelector<HTMLTextAreaElement>('#fm-rag-question-input')?.focus(),
    );
  }

  async function openSource(attrs: RagAskDialogAttrs, sourceId: string): Promise<void> {
    if (attrs.onOpenCitation === undefined) return;
    error = undefined;
    try {
      await attrs.onOpenCitation(sourceId);
    } catch {
      error = t('ragAsk', 'citationFailed');
      m.redraw();
    }
  }

  async function retrieve(attrs: RagAskDialogAttrs): Promise<RagPreview | undefined> {
    if (selectedProfileId === '' || question.trim() === '') return undefined;
    busy = 'retrieving';
    error = undefined;
    resetRetrieval();
    abortController = new AbortController();
    let retrieved: RagPreview | undefined;
    try {
      retrieved = await attrs.client.previewRag(
        {
          profileId: selectedProfileId,
          question: question.trim(),
          retrievalStrategy: 'singleQuery',
          scope: buildScope(attrs, selectedScope, roots),
        },
        abortController.signal,
      );
      preview = retrieved;
    } catch (cause) {
      if (!(cause instanceof DOMException && cause.name === 'AbortError')) {
        error = t('ragAsk', 'retrievalFailed');
      }
    } finally {
      busy = undefined;
      abortController = undefined;
      m.redraw();
    }
    return retrieved;
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
          retrievalStrategy: 'singleQuery',
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

  async function submit(attrs: RagAskDialogAttrs): Promise<void> {
    if (busy !== undefined || selectedProfileId === '' || question.trim() === '') return;
    if (preview === undefined && (await retrieve(attrs)) === undefined) return;
    await generate(attrs);
  }

  async function copy(value: string): Promise<void> {
    try {
      await copyText(value);
    } catch {
      error = t('ragAsk', 'copyFailed');
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
      if (attrs.open && !wasOpen) {
        focusQuestionOnReady = true;
        void load(attrs);
      }
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
          m('.fm-rag-question', [
            m('.fm-rag-section-heading', [
              m('h3', m('label', { for: 'fm-rag-question-input' }, t('ragAsk', 'question'))),
              m('.fm-rag-heading-actions', [
                tooltip(
                  t('ragAsk', 'newQuestion'),
                  m(
                    IconButton,
                    {
                      type: 'button',
                      disabled: question === '' && preview === undefined && answer === undefined,
                      'aria-label': t('ragAsk', 'newQuestion'),
                      onclick: startNewQuestion,
                    },
                    plusIcon({ size: 18 }),
                  ),
                ),
                tooltip(
                  t('ragAsk', 'copyQuestion'),
                  m(
                    IconButton,
                    {
                      type: 'button',
                      disabled: question === '',
                      'aria-label': t('ragAsk', 'copyQuestion'),
                      onclick: () => void copy(question),
                    },
                    copyIcon({ size: 18 }),
                  ),
                ),
              ]),
            ]),
            m('textarea', {
              id: 'fm-rag-question-input',
              rows: 5,
              value: question,
              disabled: busy !== undefined,
              autofocus: true,
              oncreate: ({ dom }) => {
                if (attrs.open && busy === undefined) {
                  (dom as HTMLTextAreaElement).focus();
                  focusQuestionOnReady = false;
                }
              },
              onupdate: ({ dom }) => {
                if (focusQuestionOnReady && busy === undefined) {
                  (dom as HTMLTextAreaElement).focus();
                  focusQuestionOnReady = false;
                }
              },
              oninput: (event: InputEvent) => {
                question = (event.currentTarget as HTMLTextAreaElement).value;
                resetRetrieval();
              },
              onkeydown: (event: KeyboardEvent) => {
                if (
                  event.key !== 'Enter' ||
                  event.shiftKey ||
                  event.altKey ||
                  event.ctrlKey ||
                  event.metaKey ||
                  event.isComposing
                ) {
                  return;
                }
                event.preventDefault();
                void submit(attrs);
              },
            }),
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
                    preview.evidence.map((item) => {
                      const title = decodeEvidenceTitle(item.title);
                      const score = item.score.toFixed(3);
                      return m('li', { key: item.label }, [
                        m('.fm-rag-evidence-heading', [
                          m(
                            'button.fm-rag-source-link',
                            {
                              type: 'button',
                              disabled: !item.available || attrs.onOpenCitation === undefined,
                              'aria-label': t('ragAsk', 'openEvidence', { label: item.label }),
                              onclick: () => void openSource(attrs, item.sourceId),
                            },
                            `${item.label}${title === undefined ? '' : ` · ${title}`}`,
                          ),
                          tooltip(
                            t('ragAsk', 'copyEvidence', { label: item.label }),
                            m(
                              IconButton,
                              {
                                type: 'button',
                                'aria-label': t('ragAsk', 'copyEvidence', {
                                  label: item.label,
                                }),
                                onclick: () => void copy(item.excerpt),
                              },
                              copyIcon({ size: 16 }),
                            ),
                          ),
                        ]),
                        m('p', item.excerpt),
                        m(
                          'small',
                          [
                            m(
                              'span.fm-rag-evidence-score',
                              {
                                title: t('ragAsk', 'similarityDescription', { score }),
                                'aria-label': t('ragAsk', 'similarityDescription', { score }),
                              },
                              t('ragAsk', 'similarityScore', { score }),
                            ),
                            item.sectionPath.join(' / '),
                            item.generated ? t('ragAsk', 'generatedEvidence') : undefined,
                            item.stale ? t('ragAsk', 'staleEvidence') : undefined,
                            !item.available ? t('ragAsk', 'unavailableEvidence') : undefined,
                          ]
                            .filter((value) => value !== undefined && value !== '')
                            .flatMap((value, index) => (index === 0 ? [value] : [' · ', value])),
                        ),
                      ]);
                    }),
                  ),
                ],
              ),
          m('section.fm-rag-answer', [
            m('.fm-rag-section-heading', [
              m('h3', t('ragAsk', 'answer')),
              tooltip(
                t('ragAsk', 'copyAnswer'),
                m(
                  IconButton,
                  {
                    type: 'button',
                    disabled: answer === undefined && streamedText === '',
                    'aria-label': t('ragAsk', 'copyAnswer'),
                    onclick: () => void copy(answer?.text ?? streamedText),
                  },
                  copyIcon({ size: 18 }),
                ),
              ),
            ]),
            m(
              '.fm-rag-answer-content',
              {
                'aria-live': 'polite',
                onclick: (event: MouseEvent) => {
                  if (!(event.target instanceof Element)) return;
                  const link = event.target.closest<HTMLAnchorElement>('a[href^="#rag-citation-"]');
                  if (link === null) return;
                  const target = link.getAttribute('href');
                  const label =
                    target === null
                      ? undefined
                      : decodeURIComponent(target.slice('#rag-citation-'.length));
                  const citation = answer?.citations.find((item) => item.label === label);
                  if (citation === undefined || citation.unavailable) return;
                  event.preventDefault();
                  void openSource(attrs, citation.sourceId);
                },
              },
              [
                streamedText === '' && answer === undefined
                  ? m('p.fm-rag-answer-placeholder', t('ragAsk', 'answerPlaceholder'))
                  : m(
                      '.fm-rag-answer-markdown',
                      m.trust(
                        safeMarkdownHtml(
                          linkAnswerCitations(
                            answer?.text ?? streamedText,
                            answer?.citations ?? [],
                          ),
                        ),
                      ),
                    ),
                answer?.modelKnowledgeAllowed === true
                  ? m('p.fm-rag-warning', t('ragAsk', 'modelKnowledgeUsed'))
                  : undefined,
                m(
                  'ul.fm-rag-citations',
                  answer?.citations.map((citation) => {
                    const provenance = formatCitationProvenance(citation.provenance);
                    return m('li', { key: `${citation.label}-${citation.sourceId}` }, [
                      m(
                        'button',
                        {
                          type: 'button',
                          disabled: citation.unavailable || attrs.onOpenCitation === undefined,
                          onclick: () => void openSource(attrs, citation.sourceId),
                          'aria-label': t('ragAsk', 'openCitation', {
                            label: citation.label,
                          }),
                        },
                        citation.label,
                      ),
                      provenance === '' ? '' : ` · ${provenance}`,
                      citation.generated ? ` · ${t('ragAsk', 'generatedEvidence')}` : '',
                      citation.stale ? ` · ${t('ragAsk', 'staleEvidence')}` : '',
                      citation.unavailable ? ` · ${t('ragAsk', 'unavailableEvidence')}` : '',
                    ]);
                  }),
                ),
              ],
            ),
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
                      retrieval:
                        preview === undefined
                          ? undefined
                          : {
                              requestedStrategy: preview.requestedStrategy,
                              appliedStrategy: preview.appliedStrategy,
                              plannedQueries: preview.plannedQueries,
                              plannerVersion: preview.plannerVersion,
                              fusionVersion: preview.fusionVersion,
                              fallbackReason: preview.fallbackReason,
                            },
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
          m('.fm-rag-preferences', [
            m('.fm-rag-preference-row', [
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
            ]),
            currentFolderStatusError
              ? m('p.fm-rag-warning', { role: 'alert' }, t('ragAsk', 'folderStatusFailed'))
              : undefined,
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
            : []),
        ],
      }),
  };
};
