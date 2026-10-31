import type {
  ActionDescriptor,
  ActionResult,
  InvokeActionRequest,
  Location,
  PluginDescriptor,
  PluginId,
  PluginLogEntry,
  PluginPreviewResult,
} from '../../models';
import { desktopHostRequiredError } from '../fetch-mutator';
import type { MockContext } from './mock-context';
import { actions, MockClientError } from './mock-shared';
import type { PluginClient } from './plugin-client';

export class MockPluginClient implements PluginClient {
  constructor(private readonly ctx: MockContext) {}

  listActions(signal?: AbortSignal): Promise<ActionDescriptor[]> {
    return this.ctx.perform('listActions', signal, () => structuredClone(actions));
  }

  openPluginPanel(
    _pluginId: PluginId,
    _actionId: string,
    _location: Location,
    _bounds: import('./file-manager-client').PluginPanelBounds,
    _theme: 'light' | 'dark',
  ): Promise<string> {
    return Promise.reject(desktopHostRequiredError());
  }

  updatePluginPanelBounds(): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  setPluginPanelVisible(): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  setPluginPanelTheme(): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  closePluginPanel(): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  invokeAction(request: InvokeActionRequest, signal?: AbortSignal): Promise<ActionResult> {
    return this.ctx.perform('invokeAction', signal, () => {
      if (!actions.some((action) => action.id === request.actionId)) {
        throw new MockClientError('actionNotFound', `No mock action with id ${request.actionId}`);
      }
      return { actionId: request.actionId, invoked: true };
    });
  }

  listPlugins(signal?: AbortSignal): Promise<PluginDescriptor[]> {
    return this.ctx.perform('listPlugins', signal, () => structuredClone(this.ctx.pluginState));
  }

  setPluginEnabled(pluginId: PluginId, enabled: boolean, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('setPluginEnabled', signal, () => {
      if (!this.ctx.pluginState.some((plugin) => plugin.id === pluginId)) {
        throw new MockClientError('notFound', `No mock plugin with id ${pluginId}`);
      }
      this.ctx.pluginState = this.ctx.pluginState.map((plugin) =>
        plugin.id === pluginId ? { ...plugin, enabled } : plugin,
      );
    });
  }

  renderPluginPreview(
    pluginId: PluginId,
    location: Location,
    signal?: AbortSignal,
  ): Promise<PluginPreviewResult> {
    return this.ctx.perform('renderPluginPreview', signal, () => {
      const plugin = this.ctx.pluginState.find((candidate) => candidate.id === pluginId);
      if (plugin?.enabled !== true || plugin.preview === undefined) {
        throw new MockClientError('notFound', `No enabled mock preview plugin ${pluginId}`);
      }
      return {
        kind: 'text' as const,
        body: `${pluginId} previewed ${location.uri}`,
        truncated: false,
      };
    });
  }

  getPluginLogs(pluginId: PluginId, signal?: AbortSignal): Promise<PluginLogEntry[]> {
    return this.ctx.perform('getPluginLogs', signal, () => {
      if (!this.ctx.pluginState.some((plugin) => plugin.id === pluginId)) {
        throw new MockClientError('notFound', `No mock plugin with id ${pluginId}`);
      }
      return [];
    });
  }

  getPluginIconThemeAsset(
    pluginId: PluginId,
    assetPath: string,
    signal?: AbortSignal,
  ): Promise<string> {
    return this.ctx.perform('getPluginIconThemeAsset', signal, () => {
      const plugin = this.ctx.pluginState.find((candidate) => candidate.id === pluginId);
      const isDeclared = Object.values(plugin?.iconTheme?.iconDefinitions ?? {}).some(
        (definition) => definition.iconPath === assetPath,
      );
      if (!isDeclared) {
        throw new MockClientError(
          'notFound',
          `No icon theme asset ${assetPath} for plugin ${pluginId}`,
        );
      }
      return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"></svg>';
    });
  }
}
