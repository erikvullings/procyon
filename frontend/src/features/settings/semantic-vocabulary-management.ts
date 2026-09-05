import m, { type FactoryComponent } from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { SemanticVocabulary, WorkspaceId } from '../../models';

export interface SemanticVocabularyManagementAttrs {
  readonly client: FileManagerClient;
  readonly workspaceId?: WorkspaceId;
  readonly onCreateConceptFolder: (vocabulary: SemanticVocabulary, conceptUri: string) => void;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : t('semanticVocabulary', 'unknownError');
}

export const SemanticVocabularyManagement: FactoryComponent<
  SemanticVocabularyManagementAttrs
> = () => {
  let vocabularies: readonly SemanticVocabulary[] = [];
  let source = '';
  let loading = true;
  let busy = false;
  let error: string | undefined;
  let pendingDelete: string | undefined;

  const load = async (client: FileManagerClient): Promise<void> => {
    loading = true;
    error = undefined;
    try {
      vocabularies = await client.listSemanticVocabularies();
    } catch (cause) {
      error = message(cause);
    } finally {
      loading = false;
      m.redraw();
    }
  };

  return {
    oninit: ({ attrs }) => void load(attrs.client),
    view: ({ attrs }) =>
      m('section.fm-semantic-vocabularies.col.s12', { 'aria-busy': loading || busy }, [
        m('p', t('semanticVocabulary', 'description')),
        m('label', { for: 'semantic-vocabulary-source' }, t('semanticVocabulary', 'importLabel')),
        m('textarea#semantic-vocabulary-source.materialize-textarea', {
          value: source,
          rows: 5,
          placeholder: t('semanticVocabulary', 'importPlaceholder'),
          oninput: (event: InputEvent) => {
            source = (event.currentTarget as HTMLTextAreaElement).value;
          },
        }),
        m(
          'button.btn',
          {
            type: 'button',
            disabled: busy || source.trim().length === 0,
            onclick: async () => {
              busy = true;
              error = undefined;
              try {
                await attrs.client.importSemanticVocabulary(source);
                source = '';
                vocabularies = await attrs.client.listSemanticVocabularies();
              } catch (cause) {
                error = message(cause);
              } finally {
                busy = false;
                m.redraw();
              }
            },
          },
          t('semanticVocabulary', 'importAction'),
        ),
        error ? m('p.fm-semantic-error', { role: 'alert' }, error) : undefined,
        loading
          ? m('p', { role: 'status' }, t('semanticVocabulary', 'loading'))
          : vocabularies.length === 0
            ? m('p', t('semanticVocabulary', 'empty'))
            : vocabularies.map((vocabulary) =>
                m('article.fm-semantic-vocabulary', { key: vocabulary.id }, [
                  m('h5', vocabulary.name),
                  m(
                    'p',
                    t('semanticVocabulary', 'summary', {
                      concepts: vocabulary.concepts.length,
                      pending: vocabulary.reviewQueue.filter(({ status }) => status === 'pending')
                        .length,
                    }),
                  ),
                  m(
                    'button.btn-flat',
                    {
                      type: 'button',
                      onclick: async () => {
                        busy = true;
                        error = undefined;
                        try {
                          if (pendingDelete !== vocabulary.id) {
                            const impact = await attrs.client.deleteSemanticVocabulary(
                              vocabulary.id,
                              false,
                            );
                            if (impact.requiresConfirmation) {
                              pendingDelete = vocabulary.id;
                              return;
                            }
                          } else {
                            await attrs.client.deleteSemanticVocabulary(vocabulary.id, true);
                          }
                          vocabularies = vocabularies.filter(({ id }) => id !== vocabulary.id);
                          pendingDelete = undefined;
                        } catch (cause) {
                          error = message(cause);
                        } finally {
                          busy = false;
                          m.redraw();
                        }
                      },
                    },
                    pendingDelete === vocabulary.id
                      ? t('semanticVocabulary', 'confirmDelete')
                      : t('semanticVocabulary', 'delete'),
                  ),
                  attrs.workspaceId && !vocabulary.workspaceIds.includes(attrs.workspaceId)
                    ? m(
                        'button.btn-flat',
                        {
                          type: 'button',
                          onclick: async () => {
                            busy = true;
                            try {
                              const updated = await attrs.client.attachSemanticVocabulary({
                                vocabularyId: vocabulary.id,
                                ...(attrs.workspaceId === undefined
                                  ? {}
                                  : { workspaceId: attrs.workspaceId }),
                              });
                              vocabularies = vocabularies.map((item) =>
                                item.id === updated.id ? updated : item,
                              );
                            } catch (cause) {
                              error = message(cause);
                            } finally {
                              busy = false;
                              m.redraw();
                            }
                          },
                        },
                        t('semanticVocabulary', 'attachWorkspace'),
                      )
                    : undefined,
                  m(
                    'ul.fm-semantic-concept-list',
                    vocabulary.concepts.map((concept) =>
                      m('li', { key: concept.uri }, [
                        m(
                          'span',
                          concept.prefLabels.en ??
                            concept.prefLabels.nl ??
                            Object.values(concept.prefLabels)[0] ??
                            concept.uri,
                        ),
                        m(
                          'button.btn-flat',
                          {
                            type: 'button',
                            onclick: () => attrs.onCreateConceptFolder(vocabulary, concept.uri),
                          },
                          t('semanticVocabulary', 'createFolder'),
                        ),
                      ]),
                    ),
                  ),
                  vocabulary.reviewQueue
                    .filter(({ status }) => status === 'pending')
                    .map((candidate) =>
                      m('div.fm-semantic-candidate', { key: candidate.id }, [
                        m('strong', candidate.label),
                        m(
                          'span',
                          t('semanticVocabulary', 'candidateEvidence', {
                            confidence: Math.round(candidate.confidence * 100),
                            frequency: candidate.corpusFrequency,
                          }),
                        ),
                        (['accept', 'reject'] as const).map((action) =>
                          m(
                            'button.btn-flat',
                            {
                              type: 'button',
                              onclick: async () => {
                                busy = true;
                                try {
                                  const updated = await attrs.client.reviewSemanticConceptCandidate(
                                    {
                                      vocabularyId: vocabulary.id,
                                      candidateId: candidate.id,
                                      action,
                                    },
                                  );
                                  vocabularies = vocabularies.map((item) =>
                                    item.id === updated.id ? updated : item,
                                  );
                                } catch (cause) {
                                  error = message(cause);
                                } finally {
                                  busy = false;
                                  m.redraw();
                                }
                              },
                            },
                            t('semanticVocabulary', action),
                          ),
                        ),
                      ]),
                    ),
                ]),
              ),
      ]),
  };
};
