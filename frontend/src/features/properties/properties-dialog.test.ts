import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EntryId, EntryMetadata, EntrySummary, Location } from '../../models';
import { PropertiesDialog, type PropertiesMetadataClient } from './properties-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.restoreAllMocks();
});

function location(providerId: string, uri: string): Location {
  return { providerId, uri };
}

function entry(
  overrides: Partial<EntrySummary> & { id: string; location: Location },
): EntrySummary {
  const base: EntrySummary = {
    id: overrides.id as EntryId,
    location: overrides.location,
    name: 'report.pdf',
    kind: 'file',
    size: 2_048,
    modifiedAt: '2026-07-29T12:00:00Z',
    hidden: false,
    readOnly: false,
    metadataRevision: 0,
  };
  return { ...base, ...overrides, id: overrides.id as EntryId };
}

function folderEntry(id: string, location: Location): EntrySummary {
  return {
    id: id as EntryId,
    location,
    name: id,
    kind: 'directory',
    hidden: false,
    readOnly: false,
    metadataRevision: 0,
  };
}

function emptyMetadata(entryId: EntryId): EntryMetadata {
  return {
    entryId,
    extendedAttributes: {},
    checksums: {},
    pluginFields: {},
  };
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  m.redraw.sync();
}

describe('PropertiesDialog', () => {
  it('shows byte-precise size, timestamps and location for a local entry, plus fetched permissions/ownership', async () => {
    const localEntry = entry({
      id: 'local-1',
      location: location('local', 'file:///Users/erik/report.pdf'),
    });
    const client: PropertiesMetadataClient = {
      getEntryMetadata: vi.fn().mockResolvedValue({
        ...emptyMetadata(localEntry.id),
        permissions: { readable: true, writable: true, executable: false, unixMode: 0o644 },
        ownership: { owner: '501', group: '20' },
      }),
    };

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, { open: true, entries: [localEntry], client, onCancel: vi.fn() }),
    });
    m.redraw.sync();
    await flush();

    expect(root.textContent).toContain('report.pdf');
    expect(root.textContent).toContain('2,048 bytes');
    expect(root.textContent).toContain('file:///Users/erik/report.pdf');
    expect(root.textContent).toContain('rw- (0644)');
    expect(root.textContent).toContain('501');
    expect(root.textContent).toContain('20');
    expect(client.getEntryMetadata).toHaveBeenCalledWith(
      { entryId: localEntry.id, location: localEntry.location },
      expect.anything(),
    );
  });

  it('shows fetched remote file-mode permissions for an SFTP entry', async () => {
    const sftpEntry = entry({
      id: 'sftp-1',
      location: location('sftp', 'sftp://host/home/erik/notes.txt'),
      name: 'notes.txt',
    });
    const client: PropertiesMetadataClient = {
      getEntryMetadata: vi.fn().mockResolvedValue({
        ...emptyMetadata(sftpEntry.id),
        permissions: { readable: true, writable: false, executable: false, unixMode: 0o444 },
        ownership: { owner: 'erik', group: 'staff' },
      }),
    };

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, { open: true, entries: [sftpEntry], client, onCancel: vi.fn() }),
    });
    m.redraw.sync();
    await flush();

    expect(root.textContent).toContain('r-- (0444)');
    expect(root.textContent).toContain('erik');
    expect(root.textContent).toContain('staff');
  });

  it('renders general fields without a permissions section for an FTP entry lacking permission metadata', async () => {
    const ftpEntry = entry({
      id: 'ftp-1',
      location: location('ftp', 'ftp://host/pub/readme.txt'),
      name: 'readme.txt',
    });
    const client: PropertiesMetadataClient = {
      getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(ftpEntry.id)),
    };

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, { open: true, entries: [ftpEntry], client, onCancel: vi.fn() }),
    });
    m.redraw.sync();
    await flush();

    expect(root.textContent).toContain('readme.txt');
    expect(root.querySelector('.fm-properties-body h5')).toBeNull();
  });

  it('shows compressed/uncompressed size and compression method for an archive entry', async () => {
    const archiveEntry = entry({
      id: 'archive-1',
      location: location('archive', 'archive:///Users/erik/bundle.zip!/notes.txt'),
      name: 'notes.txt',
      size: 4_096,
    });
    const client: PropertiesMetadataClient = {
      getEntryMetadata: vi.fn().mockResolvedValue({
        ...emptyMetadata(archiveEntry.id),
        archive: {
          uncompressedSize: 4_096,
          compressedSize: 1_024,
          compressionMethod: 'Deflated',
        },
      }),
    };

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, { open: true, entries: [archiveEntry], client, onCancel: vi.fn() }),
    });
    m.redraw.sync();
    await flush();

    expect(root.textContent).toContain('1,024 bytes');
    expect(root.textContent).toContain('4,096 bytes');
    expect(root.textContent).toContain('Deflated');
  });

  it('shows a total-size, item-count, folder/file breakdown aggregate for a multi-selection, without fetching metadata', async () => {
    const entries: EntrySummary[] = [
      entry({ id: 'a', location: location('local', 'file:///a'), kind: 'file', size: 100 }),
      entry({ id: 'b', location: location('local', 'file:///b'), kind: 'file', size: 250 }),
      folderEntry('c', location('local', 'file:///c')),
    ];
    const client: PropertiesMetadataClient = { getEntryMetadata: vi.fn() };

    m.mount(root, {
      view: () => m(PropertiesDialog, { open: true, entries, client, onCancel: vi.fn() }),
    });
    m.redraw.sync();
    await flush();

    expect(root.textContent).toContain('350 bytes');
    expect(client.getEntryMetadata).not.toHaveBeenCalled();
  });

  it('calls onCancel when closed', async () => {
    const localEntry = entry({ id: 'local-2', location: location('local', 'file:///x') });
    const client: PropertiesMetadataClient = {
      getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(localEntry.id)),
    };
    const onCancel = vi.fn();

    m.mount(root, {
      view: () => m(PropertiesDialog, { open: true, entries: [localEntry], client, onCancel }),
    });
    m.redraw.sync();
    await flush();

    const closeButton = Array.from(root.querySelectorAll('button')).find(
      (button) => button.textContent === 'Close',
    );
    closeButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
