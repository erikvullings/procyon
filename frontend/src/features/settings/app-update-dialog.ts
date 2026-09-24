import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { AppUpdateInfo, AppUpdateProgress } from '../../models';

export interface AppUpdateDialogAttrs {
  readonly client: FileManagerClient;
  readonly update?: AppUpdateInfo;
  readonly onLater: () => void;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : t('settings', 'updateFailed');
}

/** Confirmation gate for an update found by the automatic startup check. */
export const AppUpdateDialog: FactoryComponent<AppUpdateDialogAttrs> = () => {
  let installing = false;
  let error: string | undefined;
  let downloadedBytes = 0;
  let totalBytes: number | undefined;

  async function install(attrs: AppUpdateDialogAttrs): Promise<void> {
    installing = true;
    error = undefined;
    downloadedBytes = 0;
    totalBytes = undefined;
    m.redraw();
    try {
      await attrs.client.installAppUpdate((progress: AppUpdateProgress) => {
        if (progress.event === 'started') totalBytes = progress.contentLength;
        if (progress.event === 'progress') downloadedBytes += progress.chunkLength;
        m.redraw();
      });
    } catch (cause) {
      installing = false;
      error = message(cause);
      m.redraw();
    }
  }

  return {
    view: ({ attrs }) =>
      m(ModalPanel, {
        className: 'fm-app-update-modal',
        title: t('settings', 'updateReadyTitle'),
        description:
          attrs.update === undefined
            ? undefined
            : m('.fm-app-update-dialog-content', [
                m(
                  'p',
                  t('settings', 'updateAvailableDescription', {
                    current: attrs.update.currentVersion,
                    version: attrs.update.version,
                  }),
                ),
                attrs.update.body === undefined
                  ? undefined
                  : m('p.fm-update-notes', attrs.update.body),
                installing
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
              ]),
        isOpen: attrs.update !== undefined,
        closeOnEsc: !installing,
        onToggle: (open: boolean) => {
          if (!open && !installing) attrs.onLater();
        },
        buttons: [
          {
            label: t('settings', 'updateLater'),
            onclick: attrs.onLater,
            disabled: installing,
          },
          {
            label: installing
              ? t('settings', 'installingUpdate')
              : t('settings', 'installAndRestart'),
            onclick: () => void install(attrs),
            disabled: installing,
            className: 'fm-update-install',
          },
        ],
      }),
  };
};
