import m from 'mithril';
import type { FileManagerClient } from '../api/client/file-manager-client';
import type { SettingsSection } from '../features/settings/settings-editor';
import type { SemanticComponentStatus, Settings } from '../models';

/** Sets up the settingsWiring feature wiring and returns its functions. */
import type { ShellState } from './shell-state';

/** The narrow cross-cutting surface settings/appearance wiring needs from the shell. */
export interface SettingsCore {
  readonly refreshSemanticAssistantAvailability: (
    currentComponents?: SemanticComponentStatus,
  ) => Promise<void>;
  readonly refreshKnowledgeSearchAvailability: () => Promise<void>;
}

export function createSettingsWiring(shellState: ShellState, core: SettingsCore) {
  function applyAppearance(settings: Settings): void {
    shellState.settingsController.applyAppearance(settings);
  }

  async function applyShowHiddenFilesToAllTabs(
    client: FileManagerClient,
    showHidden: boolean,
  ): Promise<void> {
    await shellState.settingsController.applyShowHiddenFilesToAllTabs(client, showHidden);
  }

  function closeSettingsDialog(): void {
    const wasOpen = shellState.settingsDialogOpen;
    shellState.settingsController.closeSettingsDialog();
    if (wasOpen) settingsClosed();
  }

  function settingsClosed(): void {
    shellState.knowledgeAnswerRevision++;
    void core.refreshSemanticAssistantAvailability();
    void core.refreshKnowledgeSearchAvailability();
    m.redraw();
  }

  function openSettingsDialog(section: SettingsSection = 'appearance'): void {
    shellState.settingsInitialSection = section;
    if (shellState.settingsDisclosureElement === undefined) return;
    if (shellState.settingsDialogOpen) {
      m.redraw();
      return;
    }
    shellState.settingsDisclosureElement.open = true;
    shellState.settingsDialogOpen = true;
    m.redraw();
  }

  function applyIconTheme(themeId: string): void {
    shellState.settingsController.applyIconTheme(themeId);
  }

  function handleSystemThemeChange(): void {
    if (shellState.theme === 'auto') shellState.settingsController.syncTauriWindowBackground();
  }

  async function loadSettings(client: FileManagerClient): Promise<void> {
    await shellState.settingsController.loadSettings(client);
    if (
      shellState.currentSettings?.checkForUpdatesAutomatically !== true ||
      !client.supportsAppUpdates
    ) {
      return;
    }
    try {
      shellState.availableAppUpdate = (
        await client.checkForAppUpdate(undefined, {
          includeBeta: shellState.currentSettings.checkForBetaUpdates,
        })
      ).update;
      m.redraw();
    } catch (error) {
      console.warn('Automatic application update check failed.', error);
    }
  }

  return {
    applyAppearance,
    applyShowHiddenFilesToAllTabs,
    closeSettingsDialog,
    settingsClosed,
    openSettingsDialog,
    applyIconTheme,
    handleSystemThemeChange,
    loadSettings,
  };
}
