import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { type Theme, ThemeManager } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { setLocale } from '../../i18n';
import type {
  PaneId,
  PluginDescriptor,
  Settings,
  TabId,
  WorkspaceId,
  WorkspaceProjection,
} from '../../models';
import { installPluginIconTheme, restoreDefaultIconTheme } from '../../themes/plugin-icon-theme';
import type { RuntimeKind } from '../../utilities/runtime';
import type { EntryFormatSettings } from '../entry-formatting/entry-formatting';
import { dispatchWorkspaceCommand } from '../workspace/dispatch-workspace-command';

export interface SettingsControllerContext {
  setTheme(theme: Theme): void;
  setLoadedEntryFormatSettings(settings: EntryFormatSettings): void;
  getSettingsDialogOpen(): boolean;
  setSettingsDialogOpen(open: boolean): void;
  getSettingsDisclosureElement(): HTMLDetailsElement | undefined;
  getCurrentSettings(): Settings | undefined;
  setCurrentSettings(settings: Settings): void;
  getPlugins(): readonly PluginDescriptor[];
  getInstalledIconThemeId(): string | undefined;
  setInstalledIconThemeId(id: string | undefined): void;
  setNativeIconLoaderEnabled(enabled: boolean): void;
  getRuntimeKind(): RuntimeKind;
  getWorkspace(): WorkspaceProjection | undefined;
  setWorkspace(ws: WorkspaceProjection): void;
  getDirectories(): Map<string, unknown>;
  getNavigation(): { load(paneId: PaneId): Promise<unknown> };
  getClient(): FileManagerClient;
  redraw(): void;
}

export interface SettingsController {
  applyAppearance(settings: Settings): void;
  applyShowHiddenFilesToAllTabs(client: FileManagerClient, showHidden: boolean): Promise<void>;
  applyCurrentShowHiddenSetting(
    client: FileManagerClient,
    workspaceId: WorkspaceId,
    paneId: PaneId,
    tabId: TabId,
    expectedRevision: number,
  ): Promise<void>;
  closeSettingsDialog(): void;
  applyIconTheme(themeId: string): void;
  syncTauriWindowBackground(): void;
  loadSettings(client: FileManagerClient): Promise<void>;
}

export function createSettingsController(context: SettingsControllerContext): SettingsController {
  function replaceWorkspace(next: WorkspaceProjection): void {
    context.setWorkspace(next);
  }

  function tabKey(paneId: PaneId, tabId: TabId): string {
    return `${paneId}:${tabId}`;
  }

  function activeTabKey(paneId: PaneId): string {
    const pane = context.getWorkspace()?.panesById[paneId];
    return tabKey(paneId, pane?.activeTabId ?? '');
  }

  return {
    applyAppearance(settings: Settings): void {
      context.setTheme(settings.theme);
      context.setLoadedEntryFormatSettings({
        dateFormat: settings.dateFormat,
        sizeFormat: settings.sizeFormat,
        locale: navigator.language,
      });
      setLocale(settings.language);
      document.documentElement.style.setProperty('--fm-font-size', `${settings.fontSize}px`);
      document.documentElement.style.setProperty('--fm-row-height', `${settings.rowHeight}px`);
      ThemeManager.setTheme(settings.theme);
      this.applyIconTheme(settings.iconTheme);
      this.syncTauriWindowBackground();
    },

    async applyShowHiddenFilesToAllTabs(
      client: FileManagerClient,
      showHidden: boolean,
    ): Promise<void> {
      const workspace = context.getWorkspace();
      if (workspace === undefined) return;
      for (const paneId of workspace.paneOrder) {
        for (const tabId of workspace.panesById[paneId]?.tabOrder ?? []) {
          const current = context.getWorkspace();
          const tab = current?.panesById[paneId]?.tabsById[tabId];
          if (current === undefined || tab === undefined || tab.view.showHidden === showHidden) {
            continue;
          }
          try {
            await dispatchWorkspaceCommand(
              client,
              {
                type: 'updateView',
                workspaceId: current.id,
                paneId,
                tabId,
                patch: { showHidden },
                expectedRevision: current.revision,
              },
              replaceWorkspace,
            );
          } catch {
            continue;
          }
          if (activeTabKey(paneId) !== tabKey(paneId, tabId))
            context.getDirectories().delete(tabKey(paneId, tabId));
        }
      }
      for (const paneId of workspace.paneOrder) void context.getNavigation().load(paneId);
    },

    async applyCurrentShowHiddenSetting(
      client: FileManagerClient,
      workspaceId: WorkspaceId,
      paneId: PaneId,
      tabId: TabId,
      expectedRevision: number,
    ): Promise<void> {
      if (context.getCurrentSettings()?.showHiddenFiles !== true) return;
      try {
        await dispatchWorkspaceCommand(
          client,
          {
            type: 'updateView',
            workspaceId,
            paneId,
            tabId,
            patch: { showHidden: true },
            expectedRevision,
          },
          replaceWorkspace,
        );
      } catch {
        // Best-effort: the tab still works, just without hidden files until manually toggled.
      }
    },

    closeSettingsDialog(): void {
      context.setSettingsDialogOpen(false);
      const settings = context.getCurrentSettings();
      if (settings !== undefined) this.applyAppearance(settings);
      const disclosure = context.getSettingsDisclosureElement();
      if (disclosure !== undefined) disclosure.open = false;
      context.redraw();
    },

    applyIconTheme(themeId: string): void {
      if (themeId === 'native') {
        restoreDefaultIconTheme();
        context.setInstalledIconThemeId(themeId);
        context.setNativeIconLoaderEnabled(true);
        return;
      }
      context.setNativeIconLoaderEnabled(false);
      if (themeId === 'generic') {
        if (context.getInstalledIconThemeId() !== themeId) {
          restoreDefaultIconTheme();
          context.setInstalledIconThemeId(themeId);
        }
        return;
      }
      const plugin = context.getPlugins().find((candidate) => candidate.id === themeId);
      if (plugin?.iconTheme === undefined || !plugin.enabled) {
        if (context.getInstalledIconThemeId() !== undefined) restoreDefaultIconTheme();
        context.setInstalledIconThemeId(undefined);
        return;
      }
      if (themeId === context.getInstalledIconThemeId()) return;
      context.setInstalledIconThemeId(themeId);
      void installPluginIconTheme(context.getClient(), plugin.id, plugin.iconTheme).then(
        () => context.redraw(),
        () => {
          restoreDefaultIconTheme();
          context.setInstalledIconThemeId(undefined);
          context.redraw();
        },
      );
    },

    syncTauriWindowBackground(): void {
      if (context.getRuntimeKind() !== 'tauri') return;
      const styles = getComputedStyle(document.documentElement);
      const resolved = styles.getPropertyValue('--fm-surface-elevated').trim();
      if (resolved.length === 0) return;
      const win = getCurrentWindow();
      void win.setBackgroundColor(resolved);
      // 'auto' -> null lets the OS decide, matching the CSS @media fallback.
      const theme = context.getCurrentSettings()?.theme;
      void win.setTheme(theme === 'auto' || theme === undefined ? null : theme);
      // Windows draws its own caption, which otherwise stays at the OS chrome colour.
      void invoke('set_caption_colours', {
        background: resolved,
        foreground: styles.getPropertyValue('--fm-text').trim(),
      }).catch(() => undefined);
    },

    async loadSettings(client: FileManagerClient): Promise<void> {
      try {
        const settings = await client.getSettings();
        context.setCurrentSettings(settings);
        this.applyAppearance(settings);
        await this.applyShowHiddenFilesToAllTabs(client, settings.showHiddenFiles);
        context.redraw();
      } catch (error) {
        // A transport failure leaves the application usable with defaults, but silently -
        // favourites, recent locations and every other persisted setting then look reset with
        // no visible explanation. Logging at least makes that diagnosable via devtools.
        console.error('Failed to load settings; continuing with defaults.', error);
      }
    },
  };
}
