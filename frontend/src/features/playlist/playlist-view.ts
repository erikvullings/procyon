import m, { type Vnode } from 'mithril';
import { t } from '../../i18n';
import { calculateVisibleWindow } from '../directory-table/windowing';
import type { QueueItem } from '../player/player-state';
import { queueItemsFromPlaylist } from '../player/queue-items';
import type { FileViewerPlaylistContent } from '../preview/file-viewer-controller';
import { formatTrackDuration, trackDisplayPath, trackDisplayTitle } from './playlist-format';
import type { PlaylistTrackStatus } from './playlist-resolver';

const ROW_HEIGHT = 28;

export type PlaylistPlayMode = 'play' | 'enqueue' | 'next';

export interface PlaylistViewAttrs {
  readonly content: FileViewerPlaylistContent;
  /** Hands playable rows to the audio queue; absent where no player exists. */
  readonly onPlay?: (
    items: readonly QueueItem[],
    startId: string | undefined,
    mode: PlaylistPlayMode,
    skipped: { readonly remote: number; readonly missing: number },
  ) => void;
}

function play(
  attrs: PlaylistViewAttrs,
  mode: PlaylistPlayMode,
  startRow: number | undefined,
): void {
  const queued = queueItemsFromPlaylist(attrs.content.tracks, trackDisplayTitle);
  const startId = startRow === undefined ? undefined : queued.itemIdByRow.get(startRow);
  attrs.onPlay?.(queued.items, startId, mode, {
    remote: queued.remoteSkipped,
    missing: queued.missingSkipped,
  });
}

function statusLabel(status: PlaylistTrackStatus): string | undefined {
  if (status === 'missing') return t('playlist', 'missing');
  if (status === 'checking') return t('playlist', 'checking');
  if (status === 'remote') return t('playlist', 'remote');
  return undefined;
}

/**
 * A virtualised track list (task 0261): only the rows near the scroll position are mounted, so
 * a 50 000-entry playlist stays cheap. Scroll position is view-local; the tracks themselves come
 * from the viewer state.
 */
export function PlaylistView(): m.Component<PlaylistViewAttrs> {
  let scrollTop = 0;
  let viewportHeight = 480;
  return {
    view({ attrs }: Vnode<PlaylistViewAttrs>) {
      const { content } = attrs;
      const missing = content.tracks.filter((track) => track.status === 'missing').length;
      const remote = content.tracks.filter((track) => track.status === 'remote').length;
      const visible = calculateVisibleWindow({
        entryCount: content.tracks.length,
        rowHeight: ROW_HEIGHT,
        scrollTop,
        viewportHeight,
        overscan: 8,
      });
      const summary = [
        content.format.toUpperCase(),
        t('playlist', 'trackCount', content.tracks.length),
        missing > 0 ? t('playlist', 'missingCount', missing) : undefined,
        remote > 0 ? t('playlist', 'remoteCount', remote) : undefined,
      ].filter((part) => part !== undefined);
      return m('.fm-file-viewer-body.fm-playlist', [
        m('.fm-playlist-toolbar', [
          m('.fm-playlist-summary', summary.join(' · ')),
          attrs.onPlay === undefined || content.tracks.length === 0
            ? undefined
            : [
                m(
                  'button.fm-playlist-action',
                  { type: 'button', onclick: () => play(attrs, 'play', undefined) },
                  t('player', 'playAll'),
                ),
                m(
                  'button.fm-playlist-action',
                  { type: 'button', onclick: () => play(attrs, 'enqueue', undefined) },
                  t('player', 'addToQueue'),
                ),
              ],
        ]),
        content.error === undefined
          ? undefined
          : m(
              '.fm-playlist-warning[role=alert]',
              t('playlist', 'malformed', { message: content.error }),
            ),
        content.truncated
          ? m('.fm-playlist-warning', t('playlist', 'truncated', { n: content.tracks.length }))
          : undefined,
        content.tracks.length === 0 && content.error === undefined
          ? m('.fm-playlist-empty', t('playlist', 'empty'))
          : m('.fm-playlist-header[aria-hidden=true]', [
              m('span', t('playlist', 'columnNumber')),
              m('span', t('playlist', 'columnTitle')),
              m('span', t('playlist', 'columnDuration')),
              m('span', t('playlist', 'columnPath')),
            ]),
        m(
          '.fm-playlist-scroll[role=list]',
          {
            tabindex: 0,
            onscroll: (event: Event) => {
              const element = event.currentTarget as HTMLElement;
              scrollTop = element.scrollTop;
              viewportHeight = element.clientHeight || viewportHeight;
            },
            oncreate: ({ dom }) => {
              viewportHeight = (dom as HTMLElement).clientHeight || viewportHeight;
            },
          },
          m(
            '.fm-playlist-spacer',
            { style: { height: `${visible.totalHeight}px` } },
            m(
              '.fm-playlist-window',
              { style: { transform: `translateY(${visible.offsetTop}px)` } },
              content.tracks.slice(visible.start, visible.end).map((track, offset) => {
                const index = visible.start + offset;
                const label = statusLabel(track.status);
                return m(
                  `.fm-playlist-row.fm-playlist-${track.status}`,
                  {
                    key: index,
                    role: 'listitem',
                    style: { height: `${ROW_HEIGHT}px` },
                    ...(attrs.onPlay === undefined
                      ? {}
                      : {
                          title: t('player', 'playTrack'),
                          ondblclick: () => play(attrs, 'play', index),
                        }),
                  },
                  [
                    m('span.fm-playlist-number', String(index + 1)),
                    m(
                      'span.fm-playlist-title',
                      { title: trackDisplayTitle(track) },
                      trackDisplayTitle(track),
                    ),
                    m('span.fm-playlist-duration', formatTrackDuration(track.durationSeconds)),
                    m(
                      'span.fm-playlist-path',
                      { title: trackDisplayPath(track) },
                      label === undefined
                        ? trackDisplayPath(track)
                        : m('span.fm-playlist-badge', label),
                    ),
                  ],
                );
              }),
            ),
          ),
        ),
        m('.fm-playlist-hint', t('playlist', 'editAsText')),
      ]);
    },
  };
}
