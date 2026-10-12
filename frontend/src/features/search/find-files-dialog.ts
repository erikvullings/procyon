import m, { type FactoryComponent } from 'mithril';
import { FlatButton, IconButton, ModalPanel, ToggleButton } from 'mithril-materialized';
import { technicalTextInputAttrs } from '../../a11y/technical-text-input';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import {
  browserPlusIcon,
  chevronRightIcon,
  closeIcon,
  columnsIcon,
  directoryTreeIcon,
  folderOpenIcon,
  pencilIcon,
  starFilledIcon,
  starIcon,
  trashIcon,
} from '../../components/tabler-icons';
import { t } from '../../i18n';
import type {
  SavedSearch,
  SearchEntryKind,
  SearchGitStatus,
  SearchMode,
  SemanticSearchScope,
  SemanticVocabulary,
} from '../../models';
import { skosStarters } from '../settings/skos-starters';
import type { SavedSearchOpenTarget } from './find-files-controller';

/** Parameters passed to the search callback by the find-files dialog (task 0089). */
export interface FindFilesSearchParams {
  /** Explicit search interpretation; modes are never silently combined. */
  readonly mode?: SearchMode;
  /** Filename/glob query. */
  readonly filenameQuery: string;
  /** Optional content-search query. */
  readonly contentQuery?: string | undefined;
  /** Treat content query as regex. */
  readonly contentRegex: boolean;
  /** Match content with exact letter casing. */
  readonly contentCaseSensitive: boolean;
  /** Match content only at word boundaries. */
  readonly contentWholeWord: boolean;
  /** Dense-vector query text, present only in semantic mode. */
  readonly semanticQuery?: string | undefined;
  /** Visible semantic scope; search never changes enrolment. */
  readonly semanticScope?: SemanticSearchScope | undefined;
  readonly semanticLibraryId?: string;
  readonly conceptLabel?: string;
  readonly selectedConcept?: { readonly vocabularyId: string; readonly conceptUri: string };
  /** Search recursively into subdirectories. */
  readonly recurse: boolean;
  readonly entryKinds?: readonly SearchEntryKind[];
  readonly mimeTypes?: readonly string[];
  readonly minSizeBytes?: number;
  readonly maxSizeBytes?: number;
  readonly modifiedAfter?: string;
  readonly modifiedBefore?: string;
  readonly gitStatuses?: readonly SearchGitStatus[];
  readonly tags?: readonly string[];
  readonly metadata?: Readonly<Record<string, string>>;
}

/** The F7/Alt+F7 search dialog's props (task 0089). */
export interface FindFilesDialogAttrs {
  readonly open: boolean;
  readonly client?: FileManagerClient;
  readonly workspaceId?: string | undefined;
  /** Read-only context shown above the query field, e.g. the active directory's path. */
  readonly scopeLabel: string;
  readonly error?: string;
  readonly searchPending?: boolean;
  readonly onSearch: (params: FindFilesSearchParams) => void;
  readonly onCancel: () => void;
  readonly savedSearches?: readonly SavedSearch[];
  readonly onSave?: (name: string, params: FindFilesSearchParams, id?: string) => void;
  readonly onDeleteSaved?: (id: string) => void;
  readonly onToggleSavedPin?: (id: string) => void;
  readonly onOpenSaved?: (saved: SavedSearch, target: SavedSearchOpenTarget) => void;
}

const RECENT_CONCEPTS_KEY = 'procyon.recentConceptSearches.v1';
type RecentConcept = { workspaceId: string; vocabularyId: string; conceptUri: string };
type ConceptSource = Pick<SemanticVocabulary, 'id' | 'name' | 'concepts'>;

function readRecentConcepts(): RecentConcept[] {
  const stored = localStorage.getItem(RECENT_CONCEPTS_KEY);
  if (stored === null) return [];
  const parsed: unknown = JSON.parse(stored);
  if (
    !Array.isArray(parsed) ||
    !parsed.every(
      (item) =>
        typeof item === 'object' &&
        item !== null &&
        typeof item.workspaceId === 'string' &&
        typeof item.vocabularyId === 'string' &&
        typeof item.conceptUri === 'string',
    )
  ) {
    throw new Error('Invalid recent concept searches');
  }
  return (parsed as RecentConcept[]).slice(0, 8);
}

function rememberConcept(recent: RecentConcept): void {
  localStorage.setItem(
    RECENT_CONCEPTS_KEY,
    JSON.stringify(
      [
        recent,
        ...readRecentConcepts().filter(
          (item) =>
            item.workspaceId !== recent.workspaceId ||
            item.vocabularyId !== recent.vocabularyId ||
            item.conceptUri !== recent.conceptUri,
        ),
      ].slice(0, 8),
    ),
  );
}

/**
 * Moves focus away from the input before the modal closes, so the browser
 * never has to apply aria-hidden to an ancestor of the focused element.
 */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

type SizeUnit = 'B' | 'KB' | 'MB' | 'GB';

const SIZE_UNIT_BYTES: Readonly<Record<SizeUnit, number>> = {
  B: 1,
  KB: 1_000,
  MB: 1_000_000,
  GB: 1_000_000_000,
};

function displaySize(bytes: number | undefined): [string, SizeUnit] {
  if (bytes === undefined) return ['', 'B'];
  for (const unit of ['GB', 'MB', 'KB'] as const) {
    const factor = SIZE_UNIT_BYTES[unit];
    if (bytes >= factor && bytes % factor === 0) return [String(bytes / factor), unit];
  }
  return [String(bytes), 'B'];
}

function sizeField(
  id: string,
  label: string,
  value: string,
  unit: SizeUnit,
  setValue: (value: string) => void,
  setUnit: (unit: SizeUnit) => void,
): m.Children {
  return m('label.fm-find-files-size-field', [
    m('span', label),
    m('.fm-find-files-size-control', [
      m('input', {
        id,
        class: 'browser-default',
        type: 'number',
        min: 0,
        step: 'any',
        value,
        oninput: (event: InputEvent) => {
          setValue((event.currentTarget as HTMLInputElement).value);
        },
      }),
      m(
        'select.browser-default',
        {
          'aria-label': `${label} unit`,
          value: unit,
          onchange: (event: Event) => {
            setUnit((event.currentTarget as HTMLSelectElement).value as SizeUnit);
          },
        },
        (Object.keys(SIZE_UNIT_BYTES) as SizeUnit[]).map((candidate) =>
          m('option', { value: candidate }, candidate),
        ),
      ),
    ]),
  ]);
}

/** Materialized modal used by the `core.findFiles` (Alt+F7) action. */
export const FindFilesDialog: FactoryComponent<FindFilesDialogAttrs> = () => {
  let filenameQuery = '';
  let contentQuery = '';
  let contentRegex = false;
  let contentCaseSensitive = false;
  let contentWholeWord = false;
  let recurse = true;
  let mimeTypes = '';
  let minSize = '';
  let maxSize = '';
  let minSizeUnit: SizeUnit = 'B';
  let maxSizeUnit: SizeUnit = 'B';
  let modifiedAfter = '';
  let modifiedBefore = '';
  let tags = '';
  let savedName = '';
  let editingSavedId: string | undefined;
  let wasOpen = false;
  let conceptMode = false;
  let conceptFilter = '';
  let selectedVocabularyId = '';
  let selectedConcept: { vocabulary: ConceptSource; uri: string } | undefined;
  let vocabularies: readonly ConceptSource[] = [];
  let conceptLibraryId: string | undefined;
  let conceptAvailable = false;
  let conceptsLoading = false;
  let conceptError: string | undefined;
  let availabilityError: string | undefined;
  let currentFolderOnly = false;
  let recentConcepts: RecentConcept[] = [];
  let loadedWorkspace: string | undefined;
  let loadGeneration = 0;

  async function checkConceptAvailability(attrs: FindFilesDialogAttrs): Promise<void> {
    if (attrs.client === undefined || attrs.workspaceId === undefined) return;
    const workspaceId = attrs.workspaceId;
    const generation = ++loadGeneration;
    conceptAvailable = false;
    availabilityError = undefined;
    try {
      const [component, library, roots] = await Promise.all([
        attrs.client.getSemanticComponentStatus(),
        attrs.client.getSemanticLibraryStatus(),
        attrs.client.listKnowledgeRoots({ workspaceId }),
      ]);
      if (loadedWorkspace !== workspaceId || generation !== loadGeneration) return;
      conceptLibraryId = library.library?.libraryId;
      conceptAvailable =
        (component.lifecycle.state === 'installedEnabled' ||
          component.lifecycle.state === 'updateFailedRolledBack') &&
        library.available &&
        !library.paused &&
        conceptLibraryId !== undefined &&
        library.roots.some(
          (root) =>
            root.workspaceReferences.includes(workspaceId) &&
            roots.some(
              (indexed) =>
                indexed.rootId === root.id &&
                indexed.available &&
                indexed.indexedGeneration > 0 &&
                indexed.indexedSources > 0,
            ),
        );
      if (!conceptAvailable) conceptMode = false;
    } catch (cause) {
      if (loadedWorkspace === workspaceId && generation === loadGeneration) {
        availabilityError =
          cause instanceof Error ? cause.message : t('search', 'conceptAvailabilityError');
      }
    } finally {
      if (loadedWorkspace === workspaceId && generation === loadGeneration) m.redraw();
    }
  }

  async function loadConcepts(attrs: FindFilesDialogAttrs): Promise<void> {
    if (attrs.client === undefined || attrs.workspaceId === undefined || !conceptAvailable) return;
    const workspaceId = attrs.workspaceId;
    const generation = loadGeneration;
    conceptsLoading = true;
    conceptError = undefined;
    vocabularies = skosStarters.map(({ document }) => document);
    try {
      recentConcepts = readRecentConcepts().filter((item) => item.workspaceId === workspaceId);
      const listed = await attrs.client.listSemanticVocabularies();
      if (loadedWorkspace !== workspaceId || generation !== loadGeneration) return;
      vocabularies = [
        ...listed,
        ...skosStarters
          .map(({ document }) => document)
          .filter((starter) => !listed.some((vocabulary) => vocabulary.id === starter.id)),
      ];
    } catch (cause) {
      if (loadedWorkspace === workspaceId && generation === loadGeneration) {
        conceptError = cause instanceof Error ? cause.message : t('search', 'conceptLoadError');
      }
    } finally {
      if (loadedWorkspace === workspaceId && generation === loadGeneration) {
        conceptsLoading = false;
        m.redraw();
      }
    }
  }

  function conceptLabel(vocabulary: ConceptSource, uri: string): string {
    const concept = vocabulary.concepts.find((item) => item.uri === uri);
    return concept?.prefLabels.en ?? Object.values(concept?.prefLabels ?? {})[0] ?? uri;
  }

  function selectedConceptName(vocabulary: ConceptSource, uri: string): string {
    const starter = skosStarters.find(({ document }) => document.id === vocabulary.id);
    const label = conceptLabel(vocabulary, uri);
    return `${label} (${starter?.shortName ?? vocabulary.name})`;
  }

  function selectConcept(vocabulary: ConceptSource, uri: string): void {
    selectedConcept = { vocabulary, uri };
    conceptFilter = conceptLabel(vocabulary, uri);
    savedName = selectedConceptName(vocabulary, uri);
  }

  function hasPredicates(searchParams: FindFilesSearchParams): boolean {
    return (
      (searchParams.mode === 'semantic' && searchParams.semanticQuery !== undefined) ||
      searchParams.filenameQuery.length > 0 ||
      searchParams.contentQuery !== undefined ||
      (searchParams.mimeTypes?.length ?? 0) > 0 ||
      searchParams.minSizeBytes !== undefined ||
      searchParams.maxSizeBytes !== undefined ||
      searchParams.modifiedAfter !== undefined ||
      searchParams.modifiedBefore !== undefined ||
      (searchParams.tags?.length ?? 0) > 0
    );
  }

  function search(attrs: FindFilesDialogAttrs): void {
    const searchParams = params();
    if (!hasPredicates(searchParams)) return;
    rememberSelection(attrs);
    blurActive();
    attrs.onSearch(searchParams);
  }

  function rememberSelection(attrs: FindFilesDialogAttrs): void {
    if (conceptMode && selectedConcept !== undefined && attrs.workspaceId !== undefined) {
      try {
        rememberConcept({
          workspaceId: attrs.workspaceId,
          vocabularyId: selectedConcept.vocabulary.id,
          conceptUri: selectedConcept.uri,
        });
        recentConcepts = readRecentConcepts().filter(
          (item) => item.workspaceId === attrs.workspaceId,
        );
      } catch (cause) {
        conceptError = cause instanceof Error ? cause.message : t('search', 'conceptLoadError');
      }
    }
  }

  function params(): FindFilesSearchParams {
    if (
      conceptMode &&
      selectedConcept !== undefined &&
      conceptAvailable &&
      conceptLibraryId !== undefined
    ) {
      const concept = selectedConcept.vocabulary.concepts.find(
        (item) => item.uri === selectedConcept?.uri,
      );
      const label = conceptLabel(selectedConcept.vocabulary, selectedConcept.uri);
      const definition = concept?.definitions?.en;
      return {
        mode: 'semantic',
        filenameQuery: '',
        semanticQuery: definition ? `${label}. ${definition}` : label,
        conceptLabel: selectedConceptName(selectedConcept.vocabulary, selectedConcept.uri),
        selectedConcept: {
          vocabularyId: selectedConcept.vocabulary.id,
          conceptUri: selectedConcept.uri,
        },
        semanticLibraryId: conceptLibraryId,
        semanticScope: currentFolderOnly ? 'currentFolder' : 'entireLibrary',
        contentRegex: false,
        contentCaseSensitive: false,
        contentWholeWord: false,
        recurse: true,
      };
    }
    if (conceptMode) {
      return {
        mode: 'semantic',
        filenameQuery: '',
        contentRegex: false,
        contentCaseSensitive: false,
        contentWholeWord: false,
        recurse: true,
      };
    }
    const trimmedFilename = filenameQuery.trim();
    const trimmedContent = contentQuery.trim();
    const bytesOrUndefined = (value: string, unit: SizeUnit): number | undefined => {
      if (value.trim().length === 0) return undefined;
      const parsed = Number(value);
      if (!Number.isFinite(parsed) || parsed < 0) return undefined;
      return Math.round(parsed * SIZE_UNIT_BYTES[unit]);
    };
    const parsedMimeTypes = mimeTypes
      .split(',')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    const parsedTags = tags
      .split(',')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    const minimum = bytesOrUndefined(minSize, minSizeUnit);
    const maximum = bytesOrUndefined(maxSize, maxSizeUnit);
    return {
      mode: trimmedContent.length > 0 ? 'content' : 'name',
      filenameQuery: trimmedFilename,
      ...(trimmedContent.length > 0 ? { contentQuery: trimmedContent } : {}),
      contentRegex,
      contentCaseSensitive,
      contentWholeWord,
      recurse,
      ...(parsedMimeTypes.length === 0 ? {} : { mimeTypes: parsedMimeTypes }),
      ...(minimum === undefined ? {} : { minSizeBytes: minimum }),
      ...(maximum === undefined ? {} : { maxSizeBytes: maximum }),
      ...(modifiedAfter.length === 0
        ? {}
        : { modifiedAfter: new Date(`${modifiedAfter}T00:00:00Z`).toISOString() }),
      ...(modifiedBefore.length === 0
        ? {}
        : { modifiedBefore: new Date(`${modifiedBefore}T23:59:59Z`).toISOString() }),
      ...(parsedTags.length === 0 ? {} : { tags: parsedTags }),
    };
  }

  function cancel(attrs: FindFilesDialogAttrs): void {
    savedName = '';
    editingSavedId = undefined;
    blurActive();
    attrs.onCancel();
  }

  return {
    onupdate: ({ attrs }) => {
      if (!attrs.open && wasOpen) loadGeneration++;
      if (attrs.open && (!wasOpen || attrs.workspaceId !== loadedWorkspace)) {
        loadedWorkspace = attrs.workspaceId;
        selectedConcept = undefined;
        conceptFilter = '';
        selectedVocabularyId = '';
        vocabularies = [];
        conceptLibraryId = undefined;
        conceptAvailable = false;
        if (!wasOpen) conceptMode = false;
        void checkConceptAvailability(attrs);
      }
      if (attrs.open && !wasOpen) {
        if (editingSavedId === undefined && savedName.length === 0) {
          savedName = filenameQuery.trim();
        }
        const input = document.getElementById(
          conceptMode ? 'find-files-concept' : 'find-files-query',
        );
        if (input instanceof HTMLInputElement) {
          input.focus();
          input.select();
        }
      }
      wasOpen = attrs.open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        id: 'find-files-dialog',
        title: t('search', 'title'),
        className: 'fm-find-files-modal',
        description: m('.fm-find-files-body', [
          conceptAvailable && attrs.client && attrs.workspaceId
            ? m('.fm-find-files-modes', [
                m(
                  'button.btn-flat',
                  {
                    type: 'button',
                    'aria-pressed': String(!conceptMode),
                    onclick: () => {
                      conceptMode = false;
                      savedName = filenameQuery.trim();
                    },
                  },
                  t('search', 'filesMode'),
                ),
                m(
                  'button.btn-flat',
                  {
                    type: 'button',
                    'aria-pressed': String(conceptMode),
                    onclick: () => {
                      conceptMode = true;
                      loadedWorkspace = attrs.workspaceId;
                      selectedConcept = undefined;
                      conceptFilter = '';
                      selectedVocabularyId = '';
                      vocabularies = [];
                      void loadConcepts(attrs);
                      savedName = '';
                      queueMicrotask(() => document.getElementById('find-files-concept')?.focus());
                    },
                  },
                  t('search', 'conceptMode'),
                ),
              ])
            : undefined,
          availabilityError
            ? m('p.fm-field-error', { role: 'alert' }, availabilityError)
            : undefined,
          attrs.searchPending
            ? m('p', { role: 'status' }, t('search', 'startingSearch'))
            : undefined,
          attrs.error ? m('p.fm-field-error', { role: 'alert' }, attrs.error) : undefined,
          conceptMode
            ? m('.fm-find-concept', [
                m('.fm-find-concept-search-row', [
                  m('label.fm-find-concept-vocabulary', [
                    m('span', t('search', 'vocabulary')),
                    m(
                      'select.browser-default',
                      {
                        name: 'concept-vocabulary',
                        value: selectedVocabularyId,
                        onchange: (event: Event) => {
                          selectedVocabularyId = (event.currentTarget as HTMLSelectElement).value;
                          selectedConcept = undefined;
                        },
                      },
                      [
                        m('option', { value: '' }, t('search', 'allVocabularies')),
                        ...vocabularies.map((vocabulary) =>
                          m('option', { value: vocabulary.id }, vocabulary.name),
                        ),
                      ],
                    ),
                  ]),
                  m('label.fm-create-directory-field', [
                    m('span', t('search', 'chooseConcept')),
                    m('input#find-files-concept.browser-default', {
                      ...technicalTextInputAttrs,
                      type: 'search',
                      value: conceptFilter,
                      oninput: (event: InputEvent) => {
                        conceptFilter = (event.currentTarget as HTMLInputElement).value;
                        selectedConcept = undefined;
                      },
                      onkeydown: (event: KeyboardEvent) => {
                        event.stopPropagation();
                        if (event.key === 'Escape') cancel(attrs);
                      },
                    }),
                  ]),
                ]),
                conceptsLoading
                  ? m('p', { role: 'status' }, t('search', 'conceptLoading'))
                  : undefined,
                conceptError ? m('p.fm-field-error', { role: 'alert' }, conceptError) : undefined,
                !conceptsLoading && vocabularies.length === 0 && !conceptError
                  ? m('p', t('search', 'noConceptSources'))
                  : undefined,
                (() => {
                  const candidates = vocabularies
                    .filter(
                      (vocabulary) =>
                        !selectedVocabularyId || vocabulary.id === selectedVocabularyId,
                    )
                    .flatMap((vocabulary) =>
                      vocabulary.concepts.map((concept) => ({ vocabulary, concept })),
                    );
                  const term = conceptFilter.trim().toLocaleLowerCase();
                  const recent = recentConcepts.flatMap((item) => {
                    const candidate = candidates.find(
                      ({ vocabulary, concept }) =>
                        vocabulary.id === item.vocabularyId && concept.uri === item.conceptUri,
                    );
                    return candidate === undefined ? [] : [candidate];
                  });
                  const matches = term
                    ? candidates.filter(({ vocabulary, concept }) =>
                        [
                          vocabulary.name,
                          ...Object.values(concept.prefLabels),
                          ...Object.values(concept.altLabels ?? {}).flat(),
                        ].some((text) => text.toLocaleLowerCase().includes(term)),
                      )
                    : recent;
                  return m(
                    'ul.fm-find-concept-list',
                    matches.length === 0
                      ? m(
                          'li.fm-find-concept-empty',
                          t('search', term ? 'noConceptMatches' : 'conceptHint'),
                        )
                      : matches.slice(0, 20).map(({ vocabulary, concept }) =>
                          m('li', { key: `${vocabulary.id}:${concept.uri}` }, [
                            m(
                              'button.btn-flat.fm-find-concept-choice',
                              {
                                type: 'button',
                                'aria-pressed': String(
                                  selectedConcept?.vocabulary.id === vocabulary.id &&
                                    selectedConcept.uri === concept.uri,
                                ),
                                onclick: () => selectConcept(vocabulary, concept.uri),
                              },
                              [
                                m('strong', conceptLabel(vocabulary, concept.uri)),
                                m('span', ` · ${vocabulary.name}`),
                                concept.definitions?.en
                                  ? m('small.fm-find-concept-definition', concept.definitions.en)
                                  : undefined,
                              ],
                            ),
                            (() => {
                              const starter = skosStarters.find(
                                ({ document }) => document === vocabulary,
                              );
                              return starter
                                ? m('small.fm-find-concept-source', [
                                    `${starter.publisher} · ${starter.version} · `,
                                    m(
                                      'a',
                                      {
                                        href: starter.sourceUrl,
                                        target: '_blank',
                                        rel: 'noopener noreferrer',
                                      },
                                      t('semanticVocabulary', 'starterSourceLink'),
                                    ),
                                    ' · ',
                                    m(
                                      'a',
                                      {
                                        href: starter.licenseUrl,
                                        target: '_blank',
                                        rel: 'noopener noreferrer',
                                      },
                                      starter.license,
                                    ),
                                  ])
                                : undefined;
                            })(),
                          ]),
                        ),
                  );
                })(),
                m('label.fm-find-concept-scope', [
                  m('input.browser-default', {
                    type: 'checkbox',
                    checked: currentFolderOnly,
                    onchange: (event: Event) => {
                      currentFolderOnly = (event.currentTarget as HTMLInputElement).checked;
                    },
                  }),
                  m('span', t('search', 'conceptCurrentFolder')),
                ]),
              ])
            : [
                m('.fm-find-files-scope', t('search', 'searchIn', { location: attrs.scopeLabel })),
                m('label.fm-create-directory-field', [
                  m('span', t('search', 'name')),
                  m('input#find-files-query', {
                    ...technicalTextInputAttrs,
                    class: 'browser-default',
                    type: 'text',
                    value: filenameQuery,
                    placeholder: t('search', 'filenamePlaceholder'),
                    // No oncreate-focus here: ModalPanel keeps this input permanently mounted
                    // and only toggles CSS visibility, so an oncreate-focus would only ever
                    // fire once at app boot (before the dialog is ever shown) - and doing so
                    // poisons ModalPanel's own focus-restore-on-close logic, which captures
                    // whatever is focused when the dialog opens and refocuses it when the
                    // dialog closes. The onupdate hook below focuses on the real open
                    // transition instead.
                    oninput: (event: InputEvent) => {
                      const value = (event.currentTarget as HTMLInputElement).value;
                      const previousQuery = filenameQuery.trim();
                      filenameQuery = value;
                      if (
                        editingSavedId === undefined &&
                        (savedName.length === 0 || savedName === previousQuery)
                      ) {
                        savedName = value.trim();
                      }
                    },
                    onkeydown: (event: KeyboardEvent) => {
                      event.stopPropagation();
                      if (event.key === 'Escape') {
                        cancel(attrs);
                      } else if (event.key === 'Enter') {
                        event.preventDefault();
                        search(attrs);
                      }
                    },
                  }),
                ]),
                m('label.fm-create-directory-field.fm-find-files-content-field', [
                  m('span', t('search', 'content')),
                  m('.fm-find-files-content-control', [
                    m('input#find-files-content-query', {
                      ...technicalTextInputAttrs,
                      class: 'browser-default',
                      type: 'text',
                      value: contentQuery,
                      placeholder: t('search', 'contentPlaceholder'),
                      oninput: (event: InputEvent) => {
                        contentQuery = (event.currentTarget as HTMLInputElement).value;
                      },
                      onkeydown: (event: KeyboardEvent) => {
                        event.stopPropagation();
                        if (event.key === 'Escape') {
                          cancel(attrs);
                        } else if (event.key === 'Enter') {
                          event.preventDefault();
                          search(attrs);
                        }
                      },
                    }),
                    m(
                      'button.fm-file-viewer-search-toggle.fm-find-files-content-toggle',
                      {
                        type: 'button',
                        title: t('viewer', 'matchCase'),
                        'aria-pressed': contentCaseSensitive ? 'true' : 'false',
                        onclick: () => {
                          contentCaseSensitive = !contentCaseSensitive;
                        },
                      },
                      'Aa',
                    ),
                    m(
                      'button.fm-file-viewer-search-toggle.fm-find-files-content-toggle',
                      {
                        type: 'button',
                        title: t('viewer', 'matchWholeWord'),
                        'aria-pressed': contentWholeWord ? 'true' : 'false',
                        onclick: () => {
                          contentWholeWord = !contentWholeWord;
                        },
                      },
                      'Ab',
                    ),
                    m(
                      'button.fm-file-viewer-search-toggle.fm-find-files-content-toggle',
                      {
                        type: 'button',
                        title: t('viewer', 'useRegex'),
                        'aria-pressed': contentRegex ? 'true' : 'false',
                        onclick: () => {
                          contentRegex = !contentRegex;
                        },
                      },
                      '.*',
                    ),
                  ]),
                ]),
                m('.fm-find-files-advanced-section', [
                  m('details.fm-find-files-advanced', [
                    m('summary', [
                      chevronRightIcon({ size: 12, className: 'fm-find-files-advanced-caret' }),
                      m('span', t('search', 'advancedFilters')),
                    ]),
                    m('label', [
                      m('span', t('search', 'mimeTypes')),
                      m('input', {
                        class: 'browser-default',
                        value: mimeTypes,
                        placeholder: 'text/*, image/*, application/pdf',
                        oninput: (event: InputEvent) => {
                          mimeTypes = (event.currentTarget as HTMLInputElement).value;
                        },
                      }),
                    ]),
                    m('.fm-find-files-paired-fields', [
                      sizeField(
                        'find-files-min-size',
                        t('search', 'minimumBytes'),
                        minSize,
                        minSizeUnit,
                        (value) => {
                          minSize = value;
                        },
                        (unit) => {
                          minSizeUnit = unit;
                        },
                      ),
                      sizeField(
                        'find-files-max-size',
                        t('search', 'maximumBytes'),
                        maxSize,
                        maxSizeUnit,
                        (value) => {
                          maxSize = value;
                        },
                        (unit) => {
                          maxSizeUnit = unit;
                        },
                      ),
                    ]),
                    m('.fm-find-files-paired-fields', [
                      m('.fm-find-files-date-field.fm-find-files-filter-field', [
                        m(
                          'label',
                          { for: 'find-files-modified-after' },
                          t('search', 'modifiedAfter'),
                        ),
                        m('.fm-find-files-date-control', [
                          m('input#find-files-modified-after.browser-default', {
                            type: 'date',
                            value: modifiedAfter,
                            oninput: (event: InputEvent) => {
                              modifiedAfter = (event.currentTarget as HTMLInputElement).value;
                            },
                          }),
                          modifiedAfter.length === 0
                            ? undefined
                            : m(
                                'button.fm-find-files-clear-date',
                                {
                                  type: 'button',
                                  'aria-label': t('search', 'clearModifiedAfter'),
                                  title: t('search', 'clearModifiedAfter'),
                                  onclick: (event: MouseEvent) => {
                                    event.preventDefault();
                                    modifiedAfter = '';
                                  },
                                },
                                closeIcon({ size: 12 }),
                              ),
                        ]),
                      ]),
                      m('.fm-find-files-date-field.fm-find-files-filter-field', [
                        m(
                          'label',
                          { for: 'find-files-modified-before' },
                          t('search', 'modifiedBefore'),
                        ),
                        m('.fm-find-files-date-control', [
                          m('input#find-files-modified-before.browser-default', {
                            type: 'date',
                            value: modifiedBefore,
                            oninput: (event: InputEvent) => {
                              modifiedBefore = (event.currentTarget as HTMLInputElement).value;
                            },
                          }),
                          modifiedBefore.length === 0
                            ? undefined
                            : m(
                                'button.fm-find-files-clear-date',
                                {
                                  type: 'button',
                                  'aria-label': t('search', 'clearModifiedBefore'),
                                  title: t('search', 'clearModifiedBefore'),
                                  onclick: (event: MouseEvent) => {
                                    event.preventDefault();
                                    modifiedBefore = '';
                                  },
                                },
                                closeIcon({ size: 12 }),
                              ),
                        ]),
                      ]),
                    ]),
                    m('label', [
                      m('span', t('search', 'tags')),
                      m('input', {
                        class: 'browser-default',
                        value: tags,
                        oninput: (event: InputEvent) => {
                          tags = (event.currentTarget as HTMLInputElement).value;
                        },
                      }),
                    ]),
                  ]),
                  m(
                    'button.fm-find-files-recurse-toggle',
                    {
                      type: 'button',
                      title: t('search', 'recurseSubdirectories'),
                      'aria-label': t('search', 'includeSubdirectoriesState', {
                        state: recurse ? t('settings', 'on') : t('settings', 'off'),
                      }),
                      'aria-pressed': recurse ? 'true' : 'false',
                      onclick: () => {
                        recurse = !recurse;
                      },
                    },
                    directoryTreeIcon({ size: 14 }),
                  ),
                ]),
              ],
          attrs.savedSearches === undefined
            ? undefined
            : m('.fm-saved-searches', [
                m('.fm-saved-searches-title', t('search', 'savedSearches')),
                m(
                  '.fm-saved-search-list',
                  attrs.savedSearches.map((saved) =>
                    m('.fm-saved-search', { key: saved.id }, [
                      m('span.fm-saved-search-name', { title: saved.name }, saved.name),
                      m('.fm-saved-search-actions', [
                        m(
                          IconButton,
                          {
                            className: 'fm-saved-search-action',
                            'aria-label': t('search', 'openCurrentPane'),
                            title: t('search', 'openCurrentPane'),
                            onclick: () => attrs.onOpenSaved?.(saved, 'currentPane'),
                          },
                          folderOpenIcon({ size: 14 }),
                        ),
                        m(
                          IconButton,
                          {
                            className: 'fm-saved-search-action',
                            'aria-label': t('search', 'openOtherPane'),
                            title: t('search', 'openOtherPane'),
                            onclick: () => attrs.onOpenSaved?.(saved, 'otherPane'),
                          },
                          columnsIcon({ size: 14 }),
                        ),
                        m(
                          IconButton,
                          {
                            className: 'fm-saved-search-action',
                            'aria-label': t('search', 'openNewTab'),
                            title: t('search', 'openNewTab'),
                            onclick: () => attrs.onOpenSaved?.(saved, 'newTab'),
                          },
                          browserPlusIcon({ size: 14 }),
                        ),
                        saved.query.semantic === undefined
                          ? m(
                              IconButton,
                              {
                                className: 'fm-saved-search-action',
                                'aria-label': t('search', 'editSavedSearch'),
                                title: t('search', 'editSavedSearch'),
                                onclick: () => {
                                  filenameQuery = saved.query.name?.pattern ?? '';
                                  contentQuery = saved.query.content?.query ?? '';
                                  contentRegex = saved.query.content?.regex ?? false;
                                  contentCaseSensitive =
                                    saved.query.content?.caseSensitive ?? false;
                                  contentWholeWord = saved.query.content?.wholeWord ?? false;
                                  recurse = saved.query.scope.recurse;
                                  mimeTypes = saved.query.mimeTypes.join(', ');
                                  [minSize, minSizeUnit] = displaySize(saved.query.minSizeBytes);
                                  [maxSize, maxSizeUnit] = displaySize(saved.query.maxSizeBytes);
                                  modifiedAfter = saved.query.modifiedAfter?.slice(0, 10) ?? '';
                                  modifiedBefore = saved.query.modifiedBefore?.slice(0, 10) ?? '';
                                  tags = saved.query.tags.join(', ');
                                  savedName = saved.name;
                                  editingSavedId = saved.id;
                                },
                              },
                              pencilIcon({ size: 14 }),
                            )
                          : undefined,
                        m(ToggleButton, {
                          className: 'fm-saved-search-pin',
                          value: saved.id,
                          checked: saved.pinned,
                          'aria-label': saved.pinned
                            ? t('search', 'removeFromFavourites')
                            : t('search', 'addToFavourites'),
                          tooltip: saved.pinned
                            ? t('search', 'removeFromFavourites')
                            : t('search', 'addToFavourites'),
                          icon: m(
                            'span.fm-saved-search-pin-icon',
                            saved.pinned ? starFilledIcon({ size: 14 }) : starIcon({ size: 14 }),
                          ),
                          onchange: () => attrs.onToggleSavedPin?.(saved.id),
                        }),
                        m(
                          IconButton,
                          {
                            className: 'fm-saved-search-action',
                            'aria-label': t('search', 'deleteSavedSearch'),
                            title: t('search', 'deleteSavedSearch'),
                            onclick: () => attrs.onDeleteSaved?.(saved.id),
                          },
                          trashIcon({ size: 14 }),
                        ),
                      ]),
                    ]),
                  ),
                ),
                m('.fm-saved-search-save', [
                  m('input', {
                    value: savedName,
                    placeholder: t('search', 'savedSearchName'),
                    oninput: (event: InputEvent) => {
                      savedName = (event.currentTarget as HTMLInputElement).value;
                    },
                  }),
                  m(
                    FlatButton,
                    {
                      className: 'fm-save-search-button',
                      disabled: savedName.trim().length === 0 || !hasPredicates(params()),
                      onclick: () => {
                        rememberSelection(attrs);
                        attrs.onSave?.(savedName, params(), editingSavedId);
                        savedName = '';
                        editingSavedId = undefined;
                      },
                    },
                    editingSavedId === undefined
                      ? t('search', conceptMode ? 'saveConceptFolder' : 'saveSearch')
                      : t('search', 'updateSearch'),
                  ),
                ]),
              ]),
          availabilityError === undefined
            ? undefined
            : m('.fm-field-error', { role: 'alert' }, availabilityError),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open && attrs.open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('search', 'search'),
            disabled: attrs.searchPending || !hasPredicates(params()),
            onclick: () => search(attrs),
          },
        ],
      }),
  };
};
