import m, { type Vnode } from 'mithril';
import {
  closeIcon,
  listIcon,
  playerPauseIcon,
  playerPlayIcon,
  playerSkipBackIcon,
  playerSkipForwardIcon,
  repeatIcon,
  repeatOnceIcon,
  shuffleIcon,
  volumeIcon,
  volumeOffIcon,
} from '../../components/tabler-icons';
import { t } from '../../i18n';
import { formatTrackDuration } from '../playlist/playlist-format';
import type { PlayerController } from './player-controller';
import { currentItem, type PlayerState } from './player-state';
import './player.css';

export interface MiniPlayerAttrs {
  readonly state: PlayerState;
  readonly controller: PlayerController;
}

function control(
  label: string,
  icon: m.Children,
  onclick: () => void,
  options: { readonly pressed?: boolean; readonly className?: string } = {},
): m.Vnode {
  return m(
    `button.fm-player-button${options.className === undefined ? '' : `.${options.className}`}`,
    {
      type: 'button',
      title: label,
      'aria-label': label,
      ...(options.pressed === undefined ? {} : { 'aria-pressed': String(options.pressed) }),
      onclick,
    },
    icon,
  );
}

function repeatLabel(state: PlayerState): string {
  if (state.repeat === 'all') return t('player', 'repeatAll');
  if (state.repeat === 'one') return t('player', 'repeatOne');
  return t('player', 'repeatOff');
}

function renderQueue({ state, controller }: MiniPlayerAttrs): m.Children {
  return m(
    '.fm-player-queue[role=list]',
    { 'aria-label': t('player', 'queueTitle') },
    state.queue.map((item) => {
      const unavailable = state.unavailableIds.includes(item.id);
      const current = item.id === state.currentId;
      return m(
        `.fm-player-queue-row${current ? '.fm-player-queue-current' : ''}${unavailable ? '.fm-player-queue-unavailable' : ''}`,
        { key: item.id, role: 'listitem' },
        [
          m(
            'button.fm-player-queue-title',
            {
              type: 'button',
              title: item.title,
              ...(current ? { 'aria-current': 'true' } : {}),
              disabled: unavailable,
              onclick: () => controller.jumpTo(item.id),
            },
            unavailable ? `${item.title} — ${t('player', 'unavailable')}` : item.title,
          ),
          m('span.fm-player-queue-duration', formatTrackDuration(item.durationSeconds)),
          control(t('player', 'removeFromQueue'), closeIcon({ size: 14 }), () =>
            controller.remove(item.id),
          ),
        ],
      );
    }),
  );
}

/** Persistent transport bar for the audio queue; hidden while the queue is empty. */
export const MiniPlayer: m.Component<MiniPlayerAttrs> = {
  view({ attrs }: Vnode<MiniPlayerAttrs>) {
    const { state, controller } = attrs;
    const item = currentItem(state);
    if (state.queue.length === 0) return null;
    const duration = state.durationSeconds ?? item?.durationSeconds;
    const skipped = state.unavailableIds.length;
    return m('.fm-mini-player[role=region]', { 'aria-label': t('player', 'region') }, [
      state.queueOpen ? renderQueue(attrs) : undefined,
      skipped > 0
        ? m('.fm-player-note[role=status]', t('player', 'skippedTracks', skipped))
        : undefined,
      m('.fm-player-bar', [
        control(t('player', 'previous'), playerSkipBackIcon({ size: 18 }), controller.previous),
        control(
          state.playing ? t('player', 'pause') : t('player', 'play'),
          state.playing ? playerPauseIcon({ size: 20 }) : playerPlayIcon({ size: 20 }),
          controller.togglePlay,
          { className: 'fm-player-primary' },
        ),
        control(t('player', 'next'), playerSkipForwardIcon({ size: 18 }), controller.next),
        m(
          '.fm-player-title',
          { title: item?.title ?? '' },
          item?.title ?? t('player', 'nothingPlaying'),
        ),
        m('span.fm-player-time', formatTrackDuration(state.positionSeconds) || '0:00'),
        m('input.fm-player-seek[type=range]', {
          'aria-label': t('player', 'seek'),
          min: 0,
          max: Math.max(1, Math.round(duration ?? 1)),
          step: 1,
          disabled: duration === undefined,
          value: Math.round(state.positionSeconds),
          oninput: (event: Event) =>
            controller.seek(Number.parseFloat((event.target as HTMLInputElement).value)),
        }),
        m('span.fm-player-time', formatTrackDuration(duration) || '--:--'),
        control(
          state.muted ? t('player', 'unmute') : t('player', 'mute'),
          state.muted || state.volume === 0
            ? volumeOffIcon({ size: 18 })
            : volumeIcon({ size: 18 }),
          controller.toggleMute,
        ),
        m('input.fm-player-volume[type=range]', {
          'aria-label': t('player', 'volume'),
          min: 0,
          max: 100,
          step: 1,
          value: Math.round((state.muted ? 0 : state.volume) * 100),
          oninput: (event: Event) =>
            controller.setVolume(Number.parseFloat((event.target as HTMLInputElement).value) / 100),
        }),
        control(
          state.shuffle ? t('player', 'shuffleOn') : t('player', 'shuffleOff'),
          shuffleIcon({ size: 18 }),
          controller.toggleShuffle,
          { pressed: state.shuffle },
        ),
        control(
          repeatLabel(state),
          state.repeat === 'one' ? repeatOnceIcon({ size: 18 }) : repeatIcon({ size: 18 }),
          controller.cycleRepeat,
          { pressed: state.repeat !== 'off' },
        ),
        control(
          state.queueOpen ? t('player', 'hideQueue') : t('player', 'showQueue'),
          listIcon({ size: 18 }),
          controller.toggleQueue,
          { pressed: state.queueOpen },
        ),
        control(t('player', 'clearQueue'), closeIcon({ size: 16 }), controller.clear),
      ]),
    ]);
  },
};
