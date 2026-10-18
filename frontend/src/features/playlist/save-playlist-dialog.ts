import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { Location } from '../../models/location';
import { validateEntryName } from '../operations/create-directory-dialog';
import {
  PLAYLIST_EXPORT_FORMATS,
  type PlaylistExportFormat,
  type PlaylistExportTrack,
  type PlaylistPathStyle,
} from './playlist-writer';

/** What the dialog is saving, and where. */
export interface SavePlaylistRequest {
  readonly tracks: readonly PlaylistExportTrack[];
  readonly directory: Location;
}

export interface SavePlaylistDialogAttrs {
  readonly request: SavePlaylistRequest | undefined;
  readonly onConfirm: (
    fileName: string,
    format: PlaylistExportFormat,
    pathStyle: PlaylistPathStyle,
  ) => void;
  readonly onCancel: () => void;
}

/** Appends `.format` unless the name already ends with that extension. */
export function playlistFileName(name: string, format: PlaylistExportFormat): string {
  const trimmed = name.trim();
  return trimmed.toLowerCase().endsWith(`.${format}`) ? trimmed : `${trimmed}.${format}`;
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

function decoded(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

/** "Save as playlist…" modal (task 0264): name, format and path style. */
export const SavePlaylistDialog: FactoryComponent<SavePlaylistDialogAttrs> = () => {
  let name = 'playlist';
  let format: PlaylistExportFormat = 'm3u8';
  let pathStyle: PlaylistPathStyle = 'relative';
  let wasOpen = false;

  const error = (): string | undefined => validateEntryName(playlistFileName(name, format));

  function confirm(attrs: SavePlaylistDialogAttrs): void {
    if (error() !== undefined) return;
    blurActive();
    attrs.onConfirm(playlistFileName(name, format), format, pathStyle);
  }

  function cancel(attrs: SavePlaylistDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  return {
    onupdate: ({ attrs }) => {
      const open = attrs.request !== undefined;
      if (open && !wasOpen) {
        name = 'playlist';
        format = 'm3u8';
        pathStyle = 'relative';
        document.getElementById('save-playlist-name')?.focus();
      }
      wasOpen = open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('playlistExport', 'title'),
        className: 'fm-dense-modal',
        description: m('.fm-save-playlist', [
          m('label.fm-create-directory-field', [
            m('span', t('playlistExport', 'nameLabel')),
            m('input#save-playlist-name', {
              type: 'text',
              value: name,
              'aria-invalid': error() === undefined ? undefined : 'true',
              oninput: (event: InputEvent) => {
                name = (event.currentTarget as HTMLInputElement).value;
              },
              onkeydown: (event: KeyboardEvent) => {
                if (event.key === 'Escape') {
                  event.stopPropagation();
                  cancel(attrs);
                } else if (event.key === 'Enter') {
                  event.preventDefault();
                  event.stopPropagation();
                  confirm(attrs);
                }
              },
            }),
            error() === undefined ? undefined : m('.fm-field-error', error()),
          ]),
          m('label.fm-create-directory-field', [
            m('span', t('playlistExport', 'formatLabel')),
            m(
              'select.browser-default',
              {
                value: format,
                onchange: (event: Event) => {
                  format = (event.currentTarget as HTMLSelectElement).value as PlaylistExportFormat;
                },
              },
              PLAYLIST_EXPORT_FORMATS.map((option) =>
                m('option', { value: option, selected: option === format }, `.${option}`),
              ),
            ),
          ]),
          m('label.fm-create-directory-field', [
            m('span', t('playlistExport', 'pathStyleLabel')),
            m(
              'select.browser-default',
              {
                value: pathStyle,
                onchange: (event: Event) => {
                  pathStyle = (event.currentTarget as HTMLSelectElement).value as PlaylistPathStyle;
                },
              },
              (['relative', 'absolute'] as const).map((option) =>
                m(
                  'option',
                  { value: option, selected: option === pathStyle },
                  t('playlistExport', option),
                ),
              ),
            ),
          ]),
          attrs.request === undefined
            ? undefined
            : [
                m('p', t('playlistExport', 'trackCount', attrs.request.tracks.length)),
                m(
                  'p.fm-save-playlist-destination',
                  t('playlistExport', 'destination', {
                    path: decoded(attrs.request.directory.uri),
                  }),
                ),
              ],
        ]),
        isOpen: attrs.request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('playlistExport', 'save'),
            disabled: error() !== undefined,
            onclick: () => confirm(attrs),
          },
        ],
      }),
  };
};
