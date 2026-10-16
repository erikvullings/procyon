import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { ClipboardState, Location } from '../../models';

/**
 * Shares Procyon's file clipboard with the operating system (task 0247).
 *
 * Copying or cutting local files also publishes them on the OS clipboard, so they paste in
 * Finder or Explorer. The OS side always sees a *copy*: move semantics stay inside Procyon, so
 * another application can never delete Procyon's source files. Non-local entries (SFTP, S3,
 * archives, ...) are never downloaded; the OS clipboard is then left as it was.
 *
 * Pasting prefers whichever clipboard changed last. When the OS clipboard still holds what it
 * held right after Procyon's last copy or cut, Procyon's own clipboard wins (preserving cut);
 * when it holds other file references, e.g. files copied in Finder, a paste copies those
 * through the operation engine like any other copy.
 *
 * Hosts without an OS file clipboard (browser, mock) keep the internal clipboard unchanged.
 */
export interface OsFileClipboard {
  /** Publishes a freshly copied or cut selection; never rejects. */
  mirror(clipboard: ClipboardState): void;
  /** The clipboard a paste should use; never rejects. */
  resolvePasteClipboard(internal: ClipboardState): Promise<ClipboardState>;
}

function fileKey(location: Location): string {
  return `${location.providerId}\u0000${location.uri.replace(/\/+$/u, '')}`;
}

function sameFiles(left: readonly Location[], right: readonly Location[]): boolean {
  const keys = new Set(left.map(fileKey));
  return keys.size === new Set(right.map(fileKey)).size && right.every((l) => keys.has(fileKey(l)));
}

export function createOsFileClipboard(getClient: () => FileManagerClient): OsFileClipboard {
  let supported: Promise<boolean> | undefined;
  let pending: Promise<void> = Promise.resolve();
  /** The OS clipboard's file references right after Procyon's last copy or cut, `undefined`
   * before the first one or when that read failed. */
  let snapshot: readonly Location[] | undefined;

  function isSupported(): Promise<boolean> {
    supported ??= getClient()
      .getRuntimeCapabilities()
      .then((capabilities) => capabilities.osFileClipboard)
      .catch(() => {
        supported = undefined;
        return false;
      });
    return supported;
  }

  async function publish(clipboard: ClipboardState): Promise<void> {
    if (!(await isSupported())) return;
    const client = getClient();
    try {
      await client.writeOsClipboardFiles(clipboard.locations);
    } catch (error) {
      console.warn('Copying files to the system clipboard failed', error);
    }
    try {
      snapshot = await client.readOsClipboardFiles();
    } catch {
      snapshot = undefined;
    }
  }

  return {
    mirror(clipboard) {
      if (clipboard.mode === undefined || clipboard.locations.length === 0) return;
      pending = pending.then(() => publish(clipboard));
    },

    async resolvePasteClipboard(internal) {
      await pending;
      if (!(await isSupported())) return internal;
      let external: Location[];
      try {
        external = await getClient().readOsClipboardFiles();
      } catch {
        return internal;
      }
      if (external.length === 0) return internal;
      const unchanged =
        snapshot === undefined ? internal.locations.length > 0 : sameFiles(external, snapshot);
      return unchanged ? internal : { mode: 'copy', locations: external };
    },
  };
}
