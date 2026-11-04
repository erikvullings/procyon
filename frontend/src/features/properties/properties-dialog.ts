import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';

type PropertiesVnode = m.Vnode<unknown, unknown>;

import { t } from '../../i18n';
import type {
  EntryId,
  EntryMetadata,
  EntrySummary,
  FinderTag,
  FinderTags,
  Location,
  PermissionsInfo,
  RuntimeCapabilities,
  SpotlightComment,
} from '../../models';
import type { FinderTagsLoader } from '../directory-table/finder-tags-loader';
import {
  DEFAULT_ENTRY_FORMAT_SETTINGS,
  type EntryFormatSettings,
  formatEntryModifiedAt,
  formatEntrySize,
} from '../entry-formatting/entry-formatting';
import { renderFinderTagChips } from '../entry-metadata/finder-tag-chips';
import { FinderTagsEditor } from '../entry-metadata/finder-tags-editor';
import { computeSelectionAggregate } from './selection-aggregate';

/** The slice of `FileManagerClient` the dialog needs, so tests can supply a minimal stub. */
export interface PropertiesMetadataClient {
  getEntryMetadata(
    request: { entryId: EntryId; location: Location },
    signal?: AbortSignal,
  ): Promise<EntryMetadata>;
}

/** The slice used to read and write an entry's Finder tags and Spotlight comment (task 0145),
 * kept as a separate narrow interface so the read-only metadata client stays minimal. */
export interface PropertiesEditingClient {
  getFinderTags(locationUri: string, signal?: AbortSignal): Promise<FinderTags | undefined>;
  setFinderTags(locationUri: string, tags: FinderTags, signal?: AbortSignal): Promise<FinderTags>;
  getSpotlightComment(
    locationUri: string,
    signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined>;
  setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    signal?: AbortSignal,
  ): Promise<SpotlightComment>;
}

export interface PropertiesDialogAttrs {
  readonly open: boolean;
  /** One entry shows detailed per-entry properties; more than one shows an aggregate. */
  readonly entries: readonly EntrySummary[];
  readonly client: PropertiesMetadataClient;
  readonly formatSettings?: EntryFormatSettings;
  readonly onCancel: () => void;
  /** Opens "Change attributes…" for these entries (task 0258); the button is hidden when absent. */
  readonly onChangeAttributes?: (entries: readonly EntrySummary[]) => void;
  /** Runtime capabilities gate the Tags/Comment sections (task 0145); absent ⇒ neither section. */
  readonly runtimeCapabilities?: RuntimeCapabilities | undefined;
  /** Read/write access for the Tags and Comment editors (task 0145). */
  readonly editingClient?: PropertiesEditingClient | undefined;
  /** Active pane's loader, so a tag edit updates the directory-table badge immediately (task 0145). */
  readonly finderTagsLoader?: FinderTagsLoader | undefined;
}

function kindLabel(entry: EntrySummary): string {
  switch (entry.kind) {
    case 'directory':
      return t('properties', 'kindFolder');
    case 'symlink':
      return t('properties', 'kindSymlink');
    default:
      return t('properties', 'kindFile');
  }
}

function exactSizeLabel(size: number | undefined): string | undefined {
  if (size === undefined) return undefined;
  return `${new Intl.NumberFormat().format(size)} bytes`;
}

function unixModeLabel(mode: number | undefined): string | undefined {
  if (mode === undefined) return undefined;
  return `0${(mode & 0o777).toString(8)}`;
}

function permissionsLabel(permissions: PermissionsInfo): string {
  const flags = `${permissions.readable ? 'r' : '-'}${permissions.writable ? 'w' : '-'}${
    permissions.executable ? 'x' : '-'
  }`;
  const mode = unixModeLabel(permissions.unixMode);
  return mode === undefined ? flags : `${flags} (${mode})`;
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

function propertyRow(label: string, value: string | undefined): PropertiesVnode | undefined {
  return value === undefined ? undefined : m('tr', [m('th', label), m('td', value)]);
}

function sizeRowValue(
  size: number | undefined,
  settings: EntryFormatSettings,
  entryKind: EntrySummary['kind'],
): string | undefined {
  if (size === undefined) return undefined;
  const exact = exactSizeLabel(size);
  return `${exact} (${formatEntrySize({ kind: entryKind, size }, settings)})`;
}

function renderSingle(
  entry: EntrySummary,
  settings: EntryFormatSettings,
  metadata: EntryMetadata | undefined,
  metadataError: string | undefined,
  tagsSection: PropertiesVnode | undefined,
  commentSection: PropertiesVnode | undefined,
): PropertiesVnode {
  const generalRows = [
    propertyRow(t('properties', 'name'), entry.name),
    propertyRow(t('properties', 'kind'), kindLabel(entry)),
    propertyRow(t('properties', 'size'), sizeRowValue(entry.size, settings, entry.kind)),
    propertyRow(
      t('properties', 'modified'),
      entry.modifiedAt === undefined
        ? undefined
        : formatEntryModifiedAt(entry.modifiedAt, settings),
    ),
    propertyRow(
      t('properties', 'created'),
      entry.createdAt === undefined ? undefined : formatEntryModifiedAt(entry.createdAt, settings),
    ),
    propertyRow(t('properties', 'location'), entry.location.uri),
  ];

  const permissionRows = [
    metadata?.permissions === undefined
      ? undefined
      : propertyRow(t('properties', 'permissions'), permissionsLabel(metadata?.permissions)),
    propertyRow(t('properties', 'owner'), metadata?.ownership?.owner),
    propertyRow(t('properties', 'group'), metadata?.ownership?.group),
  ].filter((row): row is PropertiesVnode => row !== undefined);

  const archiveRows = [
    propertyRow(
      t('properties', 'compressedSize'),
      exactSizeLabel(metadata?.archive?.compressedSize),
    ),
    propertyRow(
      t('properties', 'uncompressedSize'),
      exactSizeLabel(metadata?.archive?.uncompressedSize),
    ),
    propertyRow(t('properties', 'compressionMethod'), metadata?.archive?.compressionMethod),
  ].filter((row): row is PropertiesVnode => row !== undefined);

  return m('.fm-properties-body', [
    m('table.fm-properties-table', m('tbody', generalRows)),
    permissionRows.length === 0
      ? undefined
      : [
          m('h5', t('properties', 'permissionsSection')),
          m('table.fm-properties-table', m('tbody', permissionRows)),
        ],
    archiveRows.length === 0
      ? undefined
      : [
          m('h5', t('properties', 'archiveSection')),
          m('table.fm-properties-table', m('tbody', archiveRows)),
        ],
    tagsSection,
    commentSection,
    metadataError === undefined ? undefined : m('.fm-field-error', metadataError),
  ]);
}

function renderAggregate(
  entries: readonly EntrySummary[],
  settings: EntryFormatSettings,
): PropertiesVnode {
  const aggregate = computeSelectionAggregate(entries);
  const rows = [
    propertyRow(t('properties', 'items'), `${aggregate.itemCount}`),
    propertyRow(t('properties', 'files'), `${aggregate.fileCount}`),
    propertyRow(t('properties', 'folders'), `${aggregate.folderCount}`),
    propertyRow(t('properties', 'totalSize'), sizeRowValue(aggregate.totalSize, settings, 'file')),
  ];
  return m('.fm-properties-body', m('table.fm-properties-table', m('tbody', rows)));
}

/**
 * Alt+Enter Properties dialog (task 0140): per-entry detail for a single selection (byte-precise
 * size, timestamps, permissions, provider-specific metadata fetched lazily via
 * `getEntryMetadata`), or a 0097-style size/count aggregate for a multi-selection.
 *
 * For a single entry it also surfaces editable Finder tags (task 0145) — via the shared
 * {@link FinderTagsDialog} editor, opened from an "Edit tags…" button — and an inline Spotlight
 * comment editor, each present only when the runtime reports the matching capability so a host that
 * cannot read/write them never shows a broken affordance.
 */
export const PropertiesDialog: FactoryComponent<PropertiesDialogAttrs> = () => {
  let wasOpen = false;
  let requestedEntryId: EntryId | undefined;
  let metadata: EntryMetadata | undefined;
  let metadataError: string | undefined;
  let abortController: AbortController | undefined;

  // Finder tags (task 0145)
  let tags: FinderTag[] | undefined;
  let tagsError: string | undefined;
  let tagsEditing = false;

  // Spotlight comment (task 0145)
  let comment: string | undefined;
  let commentDraft = '';
  let commentError: string | undefined;
  let commentSaving = false;

  function cancel(attrs: PropertiesDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  function setTagsError(): void {
    tagsError = t('properties', 'unableToLoadMetadata');
    m.redraw();
  }

  function setCommentError(): void {
    commentError = t('properties', 'unableToLoadMetadata');
    m.redraw();
  }

  /** Persists a full replacement tag set (mirrors the standalone dialog's Save in app-dialogs). */
  function persistTags(
    attrs: PropertiesDialogAttrs,
    entry: EntrySummary,
    next: readonly FinderTag[],
  ): void {
    const controller = abortController;
    tagsEditing = false;
    if (attrs.editingClient === undefined || controller === undefined) {
      tags = [...next];
      m.redraw();
      return;
    }
    const uri = entry.location.uri;
    attrs.editingClient
      .setFinderTags(uri, { tags: [...next] }, controller.signal)
      .then((persisted) => {
        if (controller.signal.aborted) return;
        tags = persisted.tags;
        attrs.finderTagsLoader?.setCached(uri, persisted);
        m.redraw();
      })
      .catch(() => {
        if (controller.signal.aborted) return;
        setTagsError();
      });
  }

  function saveComment(attrs: PropertiesDialogAttrs, entry: EntrySummary): void {
    const controller = abortController;
    if (attrs.editingClient === undefined || controller === undefined) return;
    const value = commentDraft;
    commentSaving = true;
    m.redraw();
    attrs.editingClient
      .setSpotlightComment(
        entry.location.uri,
        { comment: value.trim().length === 0 ? null : value },
        controller.signal,
      )
      .then((persisted) => {
        if (controller.signal.aborted) return;
        comment = persisted.comment ?? '';
        commentDraft = comment;
        commentError = undefined;
      })
      .catch(() => {
        if (controller.signal.aborted) return;
        setCommentError();
      })
      .finally(() => {
        commentSaving = false;
        m.redraw();
      });
  }

  return {
    view: ({ attrs }) => {
      const singleEntry = attrs.entries.length === 1 ? attrs.entries[0] : undefined;
      const canEditTags =
        attrs.runtimeCapabilities?.finderTags === true && attrs.editingClient !== undefined;
      const canEditComment =
        attrs.runtimeCapabilities?.extendedAttributes === true && attrs.editingClient !== undefined;

      if (!attrs.open && wasOpen) {
        abortController?.abort();
        abortController = undefined;
        requestedEntryId = undefined;
        metadata = undefined;
        metadataError = undefined;
        tags = undefined;
        tagsError = undefined;
        tagsEditing = false;
        comment = undefined;
        commentDraft = '';
        commentError = undefined;
      }
      wasOpen = attrs.open;

      if (attrs.open && singleEntry !== undefined && requestedEntryId !== singleEntry.id) {
        requestedEntryId = singleEntry.id;
        metadata = undefined;
        metadataError = undefined;
        tags = undefined;
        tagsError = undefined;
        comment = undefined;
        commentDraft = '';
        commentError = undefined;
        abortController?.abort();
        const controller = new AbortController();
        abortController = controller;
        const uri = singleEntry.location.uri;
        attrs.client
          .getEntryMetadata(
            { entryId: singleEntry.id, location: singleEntry.location },
            controller.signal,
          )
          .then((result) => {
            if (controller.signal.aborted) return;
            metadata = result;
            m.redraw();
          })
          .catch((error: unknown) => {
            if (controller.signal.aborted) return;
            metadataError =
              error instanceof Error ? error.message : t('properties', 'unableToLoadMetadata');
            m.redraw();
          });

        if (canEditTags) {
          attrs.editingClient
            ?.getFinderTags(uri, controller.signal)
            .then((result) => {
              if (controller.signal.aborted) return;
              tags = result?.tags ?? [];
              m.redraw();
            })
            .catch(() => {
              if (controller.signal.aborted) return;
              setTagsError();
            });
        }
        if (canEditComment) {
          attrs.editingClient
            ?.getSpotlightComment(uri, controller.signal)
            .then((result) => {
              if (controller.signal.aborted) return;
              comment = result?.comment ?? '';
              commentDraft = comment;
              m.redraw();
            })
            .catch(() => {
              if (controller.signal.aborted) return;
              setCommentError();
            });
        }
      }

      const settings = attrs.formatSettings ?? DEFAULT_ENTRY_FORMAT_SETTINGS;

      let tagsSection: PropertiesVnode | undefined;
      let commentSection: PropertiesVnode | undefined;
      if (singleEntry !== undefined) {
        if (canEditTags) {
          tagsSection = tagsEditing
            ? m('.fm-properties-tags', [
                m('h5', t('properties', 'tags')),
                m(FinderTagsEditor, {
                  open: true,
                  initialTags: tags ?? [],
                  onConfirm: (next: readonly FinderTag[]) => persistTags(attrs, singleEntry, next),
                  onCancel: () => {
                    tagsEditing = false;
                    m.redraw();
                  },
                }),
              ])
            : m('.fm-properties-tags', [
                m('h5', t('properties', 'tags')),
                tags === undefined
                  ? m('p.fm-finder-tags-empty', t('entryMetadata', 'noTags'))
                  : tags.length === 0
                    ? m('p.fm-finder-tags-empty', t('entryMetadata', 'noTags'))
                    : renderFinderTagChips(tags),
                tagsError === undefined
                  ? m(
                      'button.fm-properties-edit-tags',
                      {
                        type: 'button',
                        onclick: () => {
                          tagsEditing = true;
                          m.redraw();
                        },
                      },
                      t('properties', 'editTags'),
                    )
                  : m('.fm-field-error', tagsError),
              ]);
        }
        if (canEditComment) {
          commentSection = m('.fm-spotlight-comment-field', [
            m('h5', t('properties', 'comment')),
            m('textarea#spotlight-comment-text', {
              rows: 4,
              value: commentDraft,
              oninput: (event: InputEvent) => {
                commentDraft = (event.currentTarget as HTMLTextAreaElement).value;
              },
              onkeydown: (event: KeyboardEvent) => {
                if (event.key === 'Escape') {
                  event.stopPropagation();
                  blurActive();
                } else if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
                  event.preventDefault();
                  event.stopPropagation();
                  saveComment(attrs, singleEntry);
                }
              },
            }),
            m(
              'button.fm-properties-save-comment',
              {
                type: 'button',
                disabled: commentSaving,
                onclick: () => saveComment(attrs, singleEntry),
              },
              t('button', 'save'),
            ),
            commentError === undefined ? undefined : m('.fm-field-error', commentError),
          ]);
        }
      }

      return m(ModalPanel, {
        title: t('properties', 'title'),
        className: 'fm-dense-modal fm-properties-dialog',
        description:
          singleEntry === undefined
            ? renderAggregate(attrs.entries, settings)
            : renderSingle(
                singleEntry,
                settings,
                metadata,
                metadataError,
                tagsSection,
                commentSection,
              ),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          ...(attrs.onChangeAttributes === undefined
            ? []
            : [
                {
                  label: t('attributes', 'action'),
                  onclick: () => attrs.onChangeAttributes?.(attrs.entries),
                },
              ]),
          { label: t('button', 'close'), onclick: () => cancel(attrs) },
        ],
      });
    },
  };
};
