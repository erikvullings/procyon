import m, { type FactoryComponent } from 'mithril';
import { FlatButton, ModalPanel, Switch } from 'mithril-materialized';

import { t } from '../../i18n';
import type { PluginDescriptor, PluginId, PluginLogEntry, PluginPermissions } from '../../models';

export interface PluginManagementAttrs {
  readonly plugins: readonly PluginDescriptor[];
  readonly onToggle: (pluginId: PluginId, enabled: boolean) => Promise<void>;
  readonly onRequestLogs: (pluginId: PluginId) => Promise<readonly PluginLogEntry[]>;
}

type LogViewerState =
  | { pluginId: PluginId; status: 'loading' }
  | { pluginId: PluginId; status: 'loaded'; entries: readonly PluginLogEntry[] }
  | { pluginId: PluginId; status: 'error'; message: string };

function permissionLabels(): ReadonlyArray<{ key: keyof PluginPermissions; label: string }> {
  return [
    {
      key: 'selectedEntryMetadata',
      label: t('pluginManagement', 'permissionSelectedEntryMetadata'),
    },
    {
      key: 'selectedEntryContentRead',
      label: t('pluginManagement', 'permissionSelectedEntryContentRead'),
    },
    { key: 'filesystemRead', label: t('pluginManagement', 'permissionFilesystemRead') },
    { key: 'filesystemWrite', label: t('pluginManagement', 'permissionFilesystemWrite') },
    { key: 'clipboardRead', label: t('pluginManagement', 'permissionClipboardRead') },
    { key: 'clipboardWrite', label: t('pluginManagement', 'permissionClipboardWrite') },
    { key: 'network', label: t('pluginManagement', 'permissionNetwork') },
    { key: 'processSpawn', label: t('pluginManagement', 'permissionProcessSpawn') },
    { key: 'notifications', label: t('pluginManagement', 'permissionNotifications') },
    { key: 'settingsStorage', label: t('pluginManagement', 'permissionSettingsStorage') },
  ];
}

function isGranted(permissions: PluginPermissions, key: keyof PluginPermissions): boolean {
  const value = permissions[key];
  return typeof value === 'boolean' ? value : value.length > 0;
}

function grantedDetail(
  permissions: PluginPermissions,
  key: keyof PluginPermissions,
): string | undefined {
  const value = permissions[key];
  return Array.isArray(value) && value.length > 0 ? value.join(', ') : undefined;
}

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

/** Dense, non-Materialize-card plugin list used by the settings panel (spec §19, task 0057). */
export const PluginManagement: FactoryComponent<PluginManagementAttrs> = () => {
  const toggleErrors: Partial<Record<PluginId, string>> = {};
  let logViewer: LogViewerState | undefined;

  function handleToggle(attrs: PluginManagementAttrs, pluginId: PluginId, enabled: boolean): void {
    delete toggleErrors[pluginId];
    attrs.onToggle(pluginId, enabled).catch((error: unknown) => {
      toggleErrors[pluginId] = errorMessage(error, t('pluginManagement', 'updateFailed'));
      m.redraw();
    });
  }

  function openLogs(attrs: PluginManagementAttrs, pluginId: PluginId): void {
    logViewer = { pluginId, status: 'loading' };
    attrs.onRequestLogs(pluginId).then(
      (entries) => {
        logViewer = { pluginId, status: 'loaded', entries };
        m.redraw();
      },
      (error: unknown) => {
        logViewer = {
          pluginId,
          status: 'error',
          message: errorMessage(error, t('pluginManagement', 'loadLogFailed')),
        };
        m.redraw();
      },
    );
  }

  function closeLogs(): void {
    logViewer = undefined;
  }

  return {
    view: ({ attrs }) => {
      const viewedPlugin =
        logViewer === undefined
          ? undefined
          : attrs.plugins.find((plugin) => plugin.id === logViewer?.pluginId);
      return m('.fm-plugin-list', { 'aria-label': t('pluginManagement', 'ariaLabel') }, [
        attrs.plugins.length === 0
          ? m('.fm-plugin-empty', t('pluginManagement', 'noPlugins'))
          : attrs.plugins.map((plugin) =>
              m('article.fm-plugin-row', { 'data-plugin-id': plugin.id }, [
                m('.row.fm-plugin-summary', [
                  m('strong.col.s12', plugin.name),
                  m('span.fm-plugin-version.col.s12', `v${plugin.version}`),
                  m('p.fm-plugin-description.col.s12', plugin.description),
                  m(
                    '.col.s12',
                    m('.row', { style: { marginBottom: 0 } }, [
                      m(Switch, {
                        className: 'fm-plugin-toggle col s6',
                        label: t('pluginManagement', 'enabled', { name: plugin.name }),
                        checked: plugin.enabled,
                        left: t('settings', 'off'),
                        right: t('settings', 'on'),
                        onchange: (checked: boolean) => handleToggle(attrs, plugin.id, checked),
                      }),
                      m(FlatButton, {
                        className: 'fm-plugin-view-log right',
                        label: t('pluginManagement', 'viewLog'),
                        onclick: () => openLogs(attrs, plugin.id),
                      }),
                    ]),
                  ),
                ]),
                plugin.diagnostic === undefined
                  ? undefined
                  : m('.fm-plugin-diagnostic.col.s12', { role: 'alert' }, plugin.diagnostic),
                toggleErrors[plugin.id] === undefined
                  ? undefined
                  : m(
                      '.fm-plugin-toggle-error.col.s12',
                      { role: 'alert' },
                      toggleErrors[plugin.id],
                    ),
                plugin.permissions === undefined
                  ? undefined
                  : m(
                      '.col.s12',
                      m(
                        'ul.fm-plugin-permissions',
                        {
                          'aria-label': t('pluginManagement', 'permissionsAriaLabel', {
                            name: plugin.name,
                          }),
                        },
                        permissionLabels().map(({ key, label }) => {
                          const permissions = plugin.permissions as PluginPermissions;
                          const granted = isGranted(permissions, key);
                          const detail = grantedDetail(permissions, key);
                          return m('li.fm-plugin-permission', { 'data-granted': String(granted) }, [
                            m('span.fm-plugin-permission-state', granted ? '✓' : '✗'),
                            m('span', detail === undefined ? label : `${label}: ${detail}`),
                          ]);
                        }),
                      ),
                    ),
              ]),
            ),
        m(ModalPanel, {
          title:
            viewedPlugin === undefined
              ? t('pluginManagement', 'pluginLog')
              : t('pluginManagement', 'pluginLogNamed', { name: viewedPlugin.name }),
          description:
            logViewer?.status === 'loading'
              ? m('p', t('shell', 'loading'))
              : logViewer?.status === 'error'
                ? m('.fm-plugin-diagnostic', { role: 'alert' }, logViewer.message)
                : logViewer?.status === 'loaded' && logViewer.entries.length === 0
                  ? m('p', t('pluginManagement', 'noDiagnostics'))
                  : logViewer?.status === 'loaded'
                    ? m(
                        'ul.fm-plugin-log-entries',
                        logViewer.entries.map((entry) => m('li', entry.message)),
                      )
                    : m('p', ''),
          isOpen: logViewer !== undefined,
          closeOnEsc: true,
          onToggle: (open: boolean) => {
            if (!open) closeLogs();
          },
          buttons: [{ label: t('button', 'close'), onclick: closeLogs }],
        }),
      ]);
    },
  };
};
