/** Diagnostics view component (spec §30). */

import type { FactoryComponent, Vnode } from 'mithril';
import m from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { DiagnosticsView } from './diagnostics';
import { diagnosticsFromDto } from './diagnostics';

interface DiagnosticsViewAttrs {
  readonly client: FileManagerClient;
}

interface DiagnosticsState {
  diagnostics: DiagnosticsView | null;
  loading: boolean;
  error: string | null;
}

/** Diagnostics view component for troubleshooting and bug reports. Goes through the
 * runtime-selected {@link FileManagerClient} (Tauri IPC on desktop, HTTP elsewhere) rather than a
 * raw `fetch`, so it works when there's no separately-running `fm-server` to proxy to. */
export const DiagnosticsViewComponent: FactoryComponent<DiagnosticsViewAttrs> = ({ attrs }) => {
  const { client } = attrs;
  const state: DiagnosticsState = {
    diagnostics: null,
    loading: true,
    error: null,
  };

  const loadDiagnostics = async () => {
    state.loading = true;
    state.error = null;
    try {
      const dto = await client.getDiagnostics();
      state.diagnostics = diagnosticsFromDto(dto);
    } catch (err) {
      state.error = err instanceof Error ? err.message : t('diagnostics', 'unknown');
    } finally {
      state.loading = false;
      m.redraw();
    }
  };

  return {
    oncreate: () => {
      void loadDiagnostics();
    },
    view: (): Vnode => {
      if (state.loading) {
        return m('div.diagnostics-view', m('p', t('diagnostics', 'loading')));
      }

      if (state.error !== null) {
        return m('div.diagnostics-view.error', [
          m('h2', t('diagnostics', 'errorTitle')),
          m('p', t('diagnostics', 'failedToLoad', { error: state.error })),
          m('button', { onclick: () => void loadDiagnostics() }, t('diagnostics', 'retry')),
        ]);
      }

      if (state.diagnostics === null) {
        return m('div.diagnostics-view', m('p', t('diagnostics', 'noDiagnostics')));
      }

      const diag = state.diagnostics;

      return m('div.diagnostics-view', [
        m('p.diagnostics-subtitle', [
          m(
            'button.copy-btn',
            {
              onclick: () => void copyDiagnosticsToClipboard(diag),
              title: t('diagnostics', 'copyTitle'),
            },
            t('diagnostics', 'copyForBugReport'),
          ),
        ]),

        m('section.diagnostics-section', [
          m('h2', t('diagnostics', 'versionInformation')),
          m('dl', [
            m('dt', t('diagnostics', 'frontendVersion')),
            m('dd', diag.frontendVersion || '(unknown)'),
            m('dt', t('diagnostics', 'backendVersion')),
            m('dd', diag.backendVersion || '(unknown)'),
            ...(diag.tauriVersion !== undefined
              ? [m('dt', t('diagnostics', 'tauriVersion')), m('dd', diag.tauriVersion)]
              : []),
            m('dt', t('diagnostics', 'platform')),
            m('dd', diag.platform),
          ]),
        ]),

        m('section.diagnostics-section', [
          m('h2', t('diagnostics', 'runtimeCapabilities')),
          m('dl', [
            m('dt', t('diagnostics', 'runtime')),
            m('dd', diag.runtimeCapabilities.runtime ?? t('diagnostics', 'unknown')),
            m('dt', t('diagnostics', 'nativeMenus')),
            m(
              'dd',
              diag.runtimeCapabilities.nativeMenus
                ? t('diagnostics', 'yes')
                : t('diagnostics', 'no'),
            ),
            m('dt', t('diagnostics', 'platformContextMenu')),
            m(
              'dd',
              diag.runtimeCapabilities.platformContextMenu
                ? t('diagnostics', 'yes')
                : t('diagnostics', 'no'),
            ),
            m('dt', t('diagnostics', 'nativeFileIcons')),
            m(
              'dd',
              diag.runtimeCapabilities.nativeFileIcons
                ? t('diagnostics', 'yes')
                : t('diagnostics', 'no'),
            ),
            m('dt', t('diagnostics', 'finderAliases')),
            m(
              'dd',
              diag.runtimeCapabilities.finderAliases
                ? t('diagnostics', 'yes')
                : t('diagnostics', 'no'),
            ),
            m('dt', t('diagnostics', 'systemTrash')),
            m(
              'dd',
              diag.runtimeCapabilities.systemTrash
                ? t('diagnostics', 'yes')
                : t('diagnostics', 'no'),
            ),
            m('dt', t('diagnostics', 'plugins')),
            m(
              'dd',
              diag.runtimeCapabilities.plugins ? t('diagnostics', 'yes') : t('diagnostics', 'no'),
            ),
          ]),
        ]),

        m('section.diagnostics-section', [
          m('h2', t('diagnostics', 'connectionState')),
          m('dl', [
            m('dt', t('diagnostics', 'status')),
            m(
              'dd',
              m(
                'span',
                {
                  class: diag.connectionState.connected ? 'status-ok' : 'status-error',
                },
                [
                  diag.connectionState.connected ? '✓' : '✗',
                  ' ',
                  diag.connectionState.statusMessage,
                ],
              ),
            ),
            m('dt', t('diagnostics', 'eventsReceived')),
            m('dd', diag.connectionState.eventsReceived.toString()),
            m('dt', t('diagnostics', 'uptime')),
            m('dd', formatDuration(diag.connectionState.uptimeSeconds)),
            ...(diag.connectionState.lastEventReceived !== undefined
              ? [
                  m('dt', t('diagnostics', 'lastEvent')),
                  m('dd', new Date(diag.connectionState.lastEventReceived).toLocaleString()),
                ]
              : []),
          ]),
        ]),

        diag.loadedPlugins.length > 0
          ? m('section.diagnostics-section', [
              m('h2', t('diagnostics', 'loadedPluginsCount', { count: diag.loadedPlugins.length })),
              m(
                'ul.plugin-list',
                diag.loadedPlugins.map((plugin) =>
                  m('li', [
                    m('span.plugin-name', plugin.name),
                    ' ',
                    m('span.plugin-status', [
                      plugin.enabled ? t('diagnostics', 'enabled') : t('diagnostics', 'disabled'),
                      plugin.errorCount > 0
                        ? ` ${t('diagnostics', 'errorsCount', { count: plugin.errorCount })}`
                        : '',
                    ]),
                  ]),
                ),
              ),
            ])
          : m('section.diagnostics-section', [
              m('h2', t('diagnostics', 'loadedPlugins')),
              m('p', t('diagnostics', 'noPluginsLoaded')),
            ]),

        m('section.diagnostics-section', [
          m('h2', t('diagnostics', 'operationQueue')),
          m('dl', [
            m('dt', t('diagnostics', 'queued')),
            m('dd', diag.operationQueueStatus.queuedCount.toString()),
            m('dt', t('diagnostics', 'running')),
            m('dd', diag.operationQueueStatus.runningCount.toString()),
            m('dt', t('diagnostics', 'paused')),
            m('dd', diag.operationQueueStatus.pausedCount.toString()),
            m('dt', t('diagnostics', 'completed')),
            m('dd', diag.operationQueueStatus.completedCount.toString()),
          ]),
        ]),

        diag.recentErrors.length > 0
          ? m('section.diagnostics-section', [
              m('h2', t('diagnostics', 'recentErrorsCount', { count: diag.recentErrors.length })),
              m(
                'div.errors-list',
                diag.recentErrors.map((error) =>
                  m('div.error-entry', [
                    m('strong', error.code),
                    ' ',
                    m('span.timestamp', new Date(error.timestamp).toLocaleString()),
                    m('p', error.message),
                  ]),
                ),
              ),
            ])
          : m('section.diagnostics-section', [
              m('h2', t('diagnostics', 'recentErrors')),
              m('p', t('diagnostics', 'noRecentErrors')),
            ]),
      ]);
    },
  };
};

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

async function copyDiagnosticsToClipboard(diag: DiagnosticsView): Promise<void> {
  const lines = [
    '=== Application Diagnostics ===',
    `Frontend: ${diag.frontendVersion}`,
    `Backend: ${diag.backendVersion}`,
    diag.tauriVersion !== undefined ? `Tauri: ${diag.tauriVersion}` : null,
    `Platform: ${diag.platform}`,
    `Runtime: ${diag.runtimeCapabilities.runtime ?? 'Unknown'}`,
    '',
    '=== Connection ===',
    `Status: ${diag.connectionState.statusMessage}`,
    `Events received: ${diag.connectionState.eventsReceived}`,
    `Uptime: ${formatDuration(diag.connectionState.uptimeSeconds)}`,
    '',
    `=== Plugins (${diag.loadedPlugins.length}) ===`,
    ...diag.loadedPlugins.map(
      (p) =>
        `  ${p.name} v${p.version} [${p.enabled ? 'enabled' : 'disabled'}]${p.errorCount > 0 ? ` ${p.errorCount} errors` : ''}`,
    ),
    '',
    `=== Recent Errors (${diag.recentErrors.length}) ===`,
    ...diag.recentErrors.map((e) => `  [${e.timestamp}] ${e.code}: ${e.message}`),
  ]
    .filter((l) => l !== null)
    .join('\n');

  if (navigator.clipboard) {
    await navigator.clipboard.writeText(lines);
  } else {
    const ta = document.createElement('textarea');
    ta.value = lines;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand('copy');
    document.body.removeChild(ta);
  }
}
