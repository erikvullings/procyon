import type { ConnectionClient } from './connection-client';
import type { OperationClient } from './operation-client';
import type { PluginClient } from './plugin-client';
import type { SemanticClient } from './semantic-client';
import type { WorkspaceClient } from './workspace-client';

export type {
  NativeFileDrop,
  NotImplementedError,
  OpenWithApplication,
  PluginPanelBounds,
  WindowChrome,
} from './client-types';
export type { ConnectionClient } from './connection-client';
export type { OperationClient } from './operation-client';
export type { PluginClient } from './plugin-client';
export type { SemanticClient } from './semantic-client';
export type { WorkspaceClient } from './workspace-client';

/**
 * The full transport-neutral file manager API (spec §12). Components must depend only on this
 * composite interface, never on `fetch`, `EventSource` or Tauri's `invoke` directly (spec §3 rule
 * 1).
 *
 * It is a **composite**: the capability sub-interfaces ({@link ConnectionClient},
 * {@link WorkspaceClient}, {@link OperationClient}, {@link SemanticClient}, {@link PluginClient})
 * partition the surface by domain so new callers import only the focused sub-interface they need,
 * rather than learning all 200+ methods. Adapters implement the composite; `createFileManagerClient`
 * assembles each adapter's sub-clients into it.
 */
export interface FileManagerClient
  extends ConnectionClient,
    WorkspaceClient,
    OperationClient,
    SemanticClient,
    PluginClient {}
