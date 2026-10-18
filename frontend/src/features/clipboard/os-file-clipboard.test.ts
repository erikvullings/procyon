import { describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { ClipboardState, Location } from '../../models';
import { createOsFileClipboard } from './os-file-clipboard';

const local = (path: string): Location => ({ providerId: 'local', uri: `file://${path}` });

/** A desktop-like client whose OS clipboard holds local files only. */
function fakeClient(options: { supported?: boolean; initial?: Location[] } = {}) {
  let osFiles: Location[] = options.initial ?? [];
  const writeOsClipboardFiles = vi.fn(async (locations: readonly Location[]) => {
    if (!locations.every((location) => location.providerId === 'local')) return false;
    osFiles = [...locations];
    return true;
  });
  const readOsClipboardFiles = vi.fn(async () => osFiles);
  const client = {
    getRuntimeCapabilities: vi.fn(async () => ({ osFileClipboard: options.supported ?? true })),
    writeOsClipboardFiles,
    readOsClipboardFiles,
  } as unknown as FileManagerClient;
  return {
    client,
    writeOsClipboardFiles,
    readOsClipboardFiles,
    copyInFinder: (locations: Location[]) => {
      osFiles = locations;
    },
  };
}

describe('createOsFileClipboard', () => {
  it('publishes a copied local selection on the system clipboard', async () => {
    const fake = fakeClient();
    const clipboard = createOsFileClipboard(() => fake.client);
    const copied: ClipboardState = { mode: 'copy', locations: [local('/a/report.txt')] };

    clipboard.mirror(copied);

    expect(await clipboard.resolvePasteClipboard(copied)).toEqual(copied);
    expect(fake.writeOsClipboardFiles).toHaveBeenCalledWith(copied.locations);
  });

  it('keeps a cut as a move inside Procyon while the system clipboard is unchanged', async () => {
    const fake = fakeClient();
    const clipboard = createOsFileClipboard(() => fake.client);
    const cut: ClipboardState = { mode: 'move', locations: [local('/a/report.txt')] };

    clipboard.mirror(cut);

    expect(await clipboard.resolvePasteClipboard(cut)).toEqual(cut);
  });

  it('pastes files copied later in the system file manager as a copy', async () => {
    const fake = fakeClient();
    const clipboard = createOsFileClipboard(() => fake.client);
    const cut: ClipboardState = { mode: 'move', locations: [local('/a/report.txt')] };
    clipboard.mirror(cut);
    await clipboard.resolvePasteClipboard(cut);

    fake.copyInFinder([local('/finder/photo.jpg')]);

    expect(await clipboard.resolvePasteClipboard(cut)).toEqual({
      mode: 'copy',
      locations: [local('/finder/photo.jpg')],
    });
  });

  it('pastes system file manager files when Procyon has copied nothing', async () => {
    const fake = fakeClient({ initial: [local('/finder/photo.jpg')] });
    const clipboard = createOsFileClipboard(() => fake.client);

    expect(await clipboard.resolvePasteClipboard({ locations: [] })).toEqual({
      mode: 'copy',
      locations: [local('/finder/photo.jpg')],
    });
  });

  it('does not resurrect files a completed move already took off the clipboard', async () => {
    const fake = fakeClient();
    const clipboard = createOsFileClipboard(() => fake.client);
    clipboard.mirror({ mode: 'move', locations: [local('/a/report.txt')] });

    expect(await clipboard.resolvePasteClipboard({ locations: [] })).toEqual({ locations: [] });
  });

  it('keeps remote selections internal and still prefers them over older system files', async () => {
    const fake = fakeClient({ initial: [local('/finder/old.txt')] });
    const clipboard = createOsFileClipboard(() => fake.client);
    const remote: ClipboardState = {
      mode: 'copy',
      locations: [{ providerId: 'sftp', uri: 'sftp://host/home/me/a.txt' }],
    };

    clipboard.mirror(remote);

    expect(await clipboard.resolvePasteClipboard(remote)).toEqual(remote);
  });

  it('ignores trailing-slash and order differences when comparing with the snapshot', async () => {
    const fake = fakeClient();
    const clipboard = createOsFileClipboard(() => fake.client);
    const copied: ClipboardState = {
      mode: 'move',
      locations: [local('/a/folder'), local('/a/file.txt')],
    };
    clipboard.mirror(copied);
    await clipboard.resolvePasteClipboard(copied);

    fake.copyInFinder([local('/a/file.txt'), local('/a/folder/')]);

    expect(await clipboard.resolvePasteClipboard(copied)).toEqual(copied);
  });

  it('never touches the system clipboard on hosts without the capability', async () => {
    const fake = fakeClient({ supported: false, initial: [local('/finder/photo.jpg')] });
    const clipboard = createOsFileClipboard(() => fake.client);
    const copied: ClipboardState = { mode: 'copy', locations: [local('/a/report.txt')] };

    clipboard.mirror(copied);

    expect(await clipboard.resolvePasteClipboard(copied)).toEqual(copied);
    expect(await clipboard.resolvePasteClipboard({ locations: [] })).toEqual({ locations: [] });
    expect(fake.writeOsClipboardFiles).not.toHaveBeenCalled();
    expect(fake.readOsClipboardFiles).not.toHaveBeenCalled();
  });

  it('falls back to the internal clipboard when the system clipboard cannot be read', async () => {
    const fake = fakeClient();
    fake.readOsClipboardFiles.mockRejectedValue(new Error('busy'));
    const clipboard = createOsFileClipboard(() => fake.client);
    const copied: ClipboardState = { mode: 'copy', locations: [local('/a/report.txt')] };

    clipboard.mirror(copied);

    expect(await clipboard.resolvePasteClipboard(copied)).toEqual(copied);
  });
});
