import type { RuntimeKind } from '../../utilities/runtime';
import type { FileManagerClient } from './file-manager-client';
import { HttpFileManagerClient } from './http-file-manager-client';
import { MockFileManagerClient } from './mock-file-manager-client';
import { TauriFileManagerClient } from './tauri-file-manager-client';

function assertNever(value: never): never {
  throw new Error(`unreachable runtime kind: ${JSON.stringify(value)}`);
}

/**
 * Selects the `FileManagerClient` implementation for a build's `VITE_RUNTIME`
 * (spec §12).
 *
 * This is the frontend's single bootstrap location for choosing a transport:
 * no other module may import the concrete adapters directly (enforced by
 * `../import-boundaries.test.ts`).
 */
export function createFileManagerClient(runtime: RuntimeKind): FileManagerClient {
  switch (runtime) {
    case 'tauri':
      return new TauriFileManagerClient();
    case 'mock':
      return new MockFileManagerClient();
    case 'http':
      return new HttpFileManagerClient();
    default:
      return assertNever(runtime);
  }
}
