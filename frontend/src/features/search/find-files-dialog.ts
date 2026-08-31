import m, { type FactoryComponent } from 'mithril';
import { FlatButton, IconButton, ModalPanel, ToggleButton } from 'mithril-materialized';
import {
  browserPlusIcon,
  columnsIcon,
  folderOpenIcon,
  pencilIcon,
  starFilledIcon,
  starIcon,
  trashIcon,
} from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { SavedSearch, SearchEntryKind, SearchGitStatus } from '../../models';
import type { SavedSearchOpenTarget } from './find-files-controller';

/** Parameters passed to the search callback by the find-files dialog (task 0089). */
export interface FindFilesSearchParams {
  /** Filename/glob query. */
  readonly filenameQuery: string;
  /** Optional content-search query. */
  readonly contentQuery?: string | undefined;
  /** Treat content query as regex. */
  readonly contentRegex: boolean;
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
  /** Read-only context shown above the query field, e.g. the active directory's path. */
  readonly scopeLabel: string;
  readonly error?: string;
  readonly onSearch: (params: FindFilesSearchParams) => void;
  readonly onCancel: () => void;
  readonly savedSearches?: readonly SavedSearch[];
  readonly onSave?: (name: string, params: FindFilesSearchParams, id?: string) => void;
  readonly onDeleteSaved?: (id: string) => void;
  readonly onToggleSavedPin?: (id: string) => void;
  readonly onOpenSaved?: (saved: SavedSearch, target: SavedSearchOpenTarget) => void;
}

/**
 * Moves focus away from the input before the modal closes, so the browser
 * never has to apply aria-hidden to an ancestor of the focused element.
 */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** Materialized modal used by the `core.findFiles` (Alt+F7) action. */
export const FindFilesDialog: FactoryComponent<FindFilesDialogAttrs> = () => {
  let filenameQuery = '';
  let contentQuery = '';
  let contentRegex = false;
  let recurse = true;
  let mimeTypes = '';
  let minSizeBytes = '';
  let maxSizeBytes = '';
  let modifiedAfter = '';
  let modifiedBefore = '';
  let tags = '';
  let savedName = '';
  let editingSavedId: string | undefined;
  let wasOpen = false;

  function hasPredicates(searchParams: FindFilesSearchParams): boolean {
    return (
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
    blurActive();
    attrs.onSearch(searchParams);
  }

  function params(): FindFilesSearchParams {
    const trimmedFilename = filenameQuery.trim();
    const trimmedContent = contentQuery.trim();
    const numberOrUndefined = (value: string): number | undefined => {
      if (value.trim().length === 0) return undefined;
      const parsed = Number(value);
      return Number.isFinite(parsed) && parsed >= 0 ? parsed : undefined;
    };
    const parsedMimeTypes = mimeTypes
      .split(',')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    const parsedTags = tags
      .split(',')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    const minimum = numberOrUndefined(minSizeBytes);
    const maximum = numberOrUndefined(maxSizeBytes);
    return {
      filenameQuery: trimmedFilename,
      contentQuery: trimmedContent.length > 0 ? trimmedContent : undefined,
      contentRegex,
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
      if (attrs.open && !wasOpen) {
        if (editingSavedId === undefined && savedName.length === 0) {
          savedName = filenameQuery.trim();
        }
        const input = document.getElementById('find-files-query');
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
          // Filename query
          m('label.fm-create-directory-field', [
            m('span', t('search', 'searchIn', { location: attrs.scopeLabel })),
            m('input#find-files-query', {
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
                const previousQuery = filenameQuery.trim();
                filenameQuery = (event.currentTarget as HTMLInputElement).value;
                if (
                  editingSavedId === undefined &&
                  (savedName.length === 0 || savedName === previousQuery)
                ) {
                  savedName = filenameQuery.trim();
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
          // Content query
          m('label.fm-create-directory-field', [
            m('span', t('search', 'content')),
            m('input', {
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
          ]),
          // Options row
          m('div.fm-find-files-options', [
            m(
              FlatButton,
              {
                type: 'checkbox',
                checked: contentRegex,
                onclick: () => {
                  contentRegex = !contentRegex;
                  m.redraw();
                },
              },
              t('search', 'useRegex'),
            ),
            m(
              FlatButton,
              {
                type: 'checkbox',
                checked: recurse,
                onclick: () => {
                  recurse = !recurse;
                  m.redraw();
                },
              },
              recurse ? t('search', 'recurseSubdirectories') : t('search', 'currentDirectoryOnly'),
            ),
          ]),
          m('details.fm-find-files-advanced', [
            m('summary', t('search', 'advancedFilters')),
            m('label', [
              m('span', t('search', 'mimeTypes')),
              m('input', {
                value: mimeTypes,
                placeholder: 'video/*, application/pdf',
                oninput: (event: InputEvent) => {
                  mimeTypes = (event.currentTarget as HTMLInputElement).value;
                },
              }),
            ]),
            m('label', [
              m('span', t('search', 'minimumBytes')),
              m('input', {
                type: 'number',
                min: 0,
                value: minSizeBytes,
                oninput: (event: InputEvent) => {
                  minSizeBytes = (event.currentTarget as HTMLInputElement).value;
                },
              }),
            ]),
            m('label', [
              m('span', t('search', 'maximumBytes')),
              m('input', {
                type: 'number',
                min: 0,
                value: maxSizeBytes,
                oninput: (event: InputEvent) => {
                  maxSizeBytes = (event.currentTarget as HTMLInputElement).value;
                },
              }),
            ]),
            m('label', [
              m('span', t('search', 'modifiedAfter')),
              m('input', {
                type: 'date',
                value: modifiedAfter,
                oninput: (event: InputEvent) => {
                  modifiedAfter = (event.currentTarget as HTMLInputElement).value;
                },
              }),
            ]),
            m('label', [
              m('span', t('search', 'modifiedBefore')),
              m('input', {
                type: 'date',
                value: modifiedBefore,
                oninput: (event: InputEvent) => {
                  modifiedBefore = (event.currentTarget as HTMLInputElement).value;
                },
              }),
            ]),
            m('label', [
              m('span', t('search', 'tags')),
              m('input', {
                value: tags,
                oninput: (event: InputEvent) => {
                  tags = (event.currentTarget as HTMLInputElement).value;
                },
              }),
            ]),
          ]),
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
                        m(
                          IconButton,
                          {
                            className: 'fm-saved-search-action',
                            'aria-label': t('search', 'editSavedSearch'),
                            title: t('search', 'editSavedSearch'),
                            onclick: () => {
                              filenameQuery = saved.query.name?.pattern ?? '';
                              contentQuery = saved.query.content?.query ?? '';
                              contentRegex = saved.query.content?.regex ?? false;
                              recurse = saved.query.scope.recurse;
                              mimeTypes = saved.query.mimeTypes.join(', ');
                              minSizeBytes = saved.query.minSizeBytes?.toString() ?? '';
                              maxSizeBytes = saved.query.maxSizeBytes?.toString() ?? '';
                              modifiedAfter = saved.query.modifiedAfter?.slice(0, 10) ?? '';
                              modifiedBefore = saved.query.modifiedBefore?.slice(0, 10) ?? '';
                              tags = saved.query.tags.join(', ');
                              savedName = saved.name;
                              editingSavedId = saved.id;
                            },
                          },
                          pencilIcon({ size: 14 }),
                        ),
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
                      disabled: savedName.trim().length === 0,
                      onclick: () => {
                        attrs.onSave?.(savedName, params(), editingSavedId);
                        savedName = '';
                        editingSavedId = undefined;
                      },
                    },
                    editingSavedId === undefined
                      ? t('search', 'saveSearch')
                      : t('search', 'updateSearch'),
                  ),
                ]),
              ]),
          attrs.error === undefined ? undefined : m('.fm-field-error', attrs.error),
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
            disabled: !hasPredicates(params()),
            onclick: () => search(attrs),
          },
        ],
      }),
  };
};
