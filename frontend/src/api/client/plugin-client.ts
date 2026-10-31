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
import type { PluginPanelBounds } from './client-types';

/**
 * Plugin discovery, action dispatch and in-app plugin panel hosting.
 */
export interface PluginClient {
  closePluginPanel(label: string): Promise<void>;

  /**
   * Fetches raw SVG markup for one icon-theme asset (task 0095); `assetPath` is a theme's
   * `PluginIconDefinition.iconPath` value, passed through verbatim.
   */
  getPluginIconThemeAsset(
    pluginId: PluginId,
    assetPath: string,
    signal?: AbortSignal,
  ): Promise<string>;

  getPluginLogs(pluginId: PluginId, signal?: AbortSignal): Promise<PluginLogEntry[]>;

  invokeAction(request: InvokeActionRequest, signal?: AbortSignal): Promise<ActionResult>;

  listActions(signal?: AbortSignal): Promise<ActionDescriptor[]>;

  listPlugins(signal?: AbortSignal): Promise<PluginDescriptor[]>;

  /** Opens a packaged SPA for the cursor entry; unsupported on browser hosts. */
  openPluginPanel(
    pluginId: PluginId,
    actionId: string,
    location: Location,
    bounds: PluginPanelBounds,
    theme: 'light' | 'dark',
  ): Promise<string>;

  /**
   * Renders `location` with an enabled plugin's sandboxed preview contribution (task 0142).
   * The result is untrusted: `html` bodies must be sanitized before display.
   */
  renderPluginPreview(
    pluginId: PluginId,
    location: Location,
    signal?: AbortSignal,
  ): Promise<PluginPreviewResult>;

  setPluginEnabled(pluginId: PluginId, enabled: boolean, signal?: AbortSignal): Promise<void>;

  setPluginPanelTheme(label: string, theme: 'light' | 'dark'): Promise<void>;

  setPluginPanelVisible(label: string, visible: boolean): Promise<void>;

  updatePluginPanelBounds(label: string, bounds: PluginPanelBounds): Promise<void>;
}
