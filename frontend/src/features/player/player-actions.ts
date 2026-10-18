import type { EnglishCatalogue } from '../../i18n/en';

/** Frontend-only `client.*` actions for the audio queue (task 0262), listed in the command
 * palette and bindable in Settings while the queue is non-empty. */
export const PLAYER_CLIENT_ACTIONS: readonly {
  readonly id: string;
  readonly title: keyof EnglishCatalogue['player'];
}[] = [
  { id: 'client.playerTogglePlay', title: 'togglePlay' },
  { id: 'client.playerNext', title: 'next' },
  { id: 'client.playerPrevious', title: 'previous' },
];
