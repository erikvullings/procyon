import m, { type FactoryComponent } from 'mithril';
import { Switch } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { AppUpdateInfo, AppUpdateProgress } from '../../models';

export interface AppUpdateSettingsAttrs {
  readonly client: FileManagerClient;
  readonly automaticChecks: boolean;
  readonly onAutomaticChecksChange: (enabled: boolean) => void;
}

type UpdatePhase = 'idle' | 'checking' | 'current' | 'available' | 'installing' | 'error';

function message(error: unknown): string {
  return error instanceof Error ? error.message : t('settings', 'updateFailed');
}

/** Desktop update preference and explicit signed-update controls. */
export const AppUpdateSettings: FactoryComponent<AppUpdateSettingsAttrs> = () => {
  let phase: UpdatePhase = 'idle';
  let update: AppUpdateInfo | undefined;
  let error: string | undefined;
  let downloadedBytes = 0;
  let totalBytes: number | undefined;

  async function check(client: FileManagerClient): Promise<void> {
    phase = 'checking';
    update = undefined;
    error = undefined;
    m.redraw();
    try {
      update = await client.checkForAppUpdate();
      phase = update === undefined ? 'current' : 'available';
    } catch (cause) {
      phase = 'error';
      error = message(cause);
    }
    m.redraw();
  }

  async function install(client: FileManagerClient): Promise<void> {
    phase = 'installing';
    error = undefined;
    downloadedBytes = 0;
    totalBytes = undefined;
    m.redraw();
    try {
      await client.installAppUpdate((progress: AppUpdateProgress) => {
        if (progress.event === 'started') totalBytes = progress.contentLength;
        if (progress.event === 'progress') downloadedBytes += progress.chunkLength;
        m.redraw();
      });
    } catch (cause) {
      phase = 'error';
      error = message(cause);
      m.redraw();
    }
  }

  return {
    view: ({ attrs }) =>
      m('.fm-app-update-settings', [
        m(Switch, {
          label: t('settings', 'automaticUpdateChecks'),
          checked: attrs.automaticChecks,
          left: t('settings', 'off'),
          right: t('settings', 'on'),
          onchange: attrs.onAutomaticChecksChange,
        }),
        m('p.fm-settings-help', t('settings', 'automaticUpdateChecksHint')),
        attrs.client.supportsAppUpdates
          ? [
              m(
                'button.fm-update-check',
                {
                  type: 'button',
                  disabled: phase === 'checking' || phase === 'installing',
                  onclick: () => void check(attrs.client),
                },
                phase === 'checking'
                  ? t('settings', 'checkingForUpdates')
                  : t('settings', 'checkForUpdates'),
              ),
              phase === 'current'
                ? m('p.fm-update-status', { role: 'status' }, t('settings', 'appIsUpToDate'))
                : undefined,
              update === undefined
                ? undefined
                : m('.fm-update-available', [
                    m(
                      'p',
                      t('settings', 'updateAvailable', {
                        version: update.version,
                      }),
                    ),
                    update.body === undefined ? undefined : m('p.fm-update-notes', update.body),
                    m(
                      'button.fm-update-install',
                      {
                        type: 'button',
                        disabled: phase === 'installing',
                        onclick: () => void install(attrs.client),
                      },
                      phase === 'installing'
                        ? t('settings', 'installingUpdate')
                        : t('settings', 'installAndRestart'),
                    ),
                  ]),
              phase === 'installing'
                ? m(
                    'p.fm-update-status',
                    { role: 'status' },
                    totalBytes === undefined
                      ? t('settings', 'downloadingUpdate')
                      : t('settings', 'updateDownloadProgress', {
                          downloaded: downloadedBytes,
                          total: totalBytes,
                        }),
                  )
                : undefined,
              error === undefined ? undefined : m('.fm-update-error', { role: 'alert' }, error),
            ]
          : m('p.fm-update-unavailable', t('settings', 'updatesDesktopOnly')),
      ]),
  };
};
