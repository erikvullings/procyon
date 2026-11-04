import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';
import type {
  EntryId,
  EntryMetadata,
  EntrySummary,
  FinderTags,
  Location,
  RuntimeCapabilities,
  SpotlightComment,
} from '../../models';
import type { FinderTagsLoader } from '../directory-table/finder-tags-loader';
import {
  PropertiesDialog,
  type PropertiesEditingClient,
  type PropertiesMetadataClient,
} from './properties-dialog';

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

  function editingClientStub(): {
    getFinderTags: Mock;
    setFinderTags: Mock;
    getSpotlightComment: Mock;
    setSpotlightComment: Mock;
  } {
    return {
      getFinderTags: vi.fn().mockResolvedValue(undefined),
      setFinderTags: vi.fn().mockResolvedValue(undefined),
      getSpotlightComment: vi.fn().mockResolvedValue(undefined),
      setSpotlightComment: vi.fn().mockResolvedValue(undefined),
    };
  }

  function capabilities(overrides: Partial<RuntimeCapabilities>): RuntimeCapabilities {
    return {
      clipboard: true,
      extendedAttributes: true,
      finderAliases: false,
      finderTags: true,
      freshInstall: false,
      nativeDragOut: false,
      nativeFileIcons: false,
      nativeMenus: false,
      nativeThumbnails: false,
      openTerminal: false,
      osFileClipboard: false,
      platform: 'macos',
      platformContextMenu: false,
      plugins: true,
      revealInSystemFileManager: true,
      runtime: 'tauri',
      semanticComponentAuthority: 'unavailable',
      semanticRuntimeExecutableDownload: 'prohibitedByMacAppStore',
      serverAdministration: false,
      ...overrides,
    } as unknown as RuntimeCapabilities;
  }

  function setCachedSpy(): { loader: FinderTagsLoader; callUri: () => string | undefined } {
    const setCached = vi.fn();
    let capturedUri: string | undefined;
    setCached.mockImplementation((uri: string) => {
      capturedUri = uri;
    });
    return { loader: { setCached } as unknown as FinderTagsLoader, callUri: () => capturedUri };
  }

  it('shows fetched Finder tags as chips for a single entry when the runtime supports them', async () => {
    const localEntry = entry({
      id: 'tags-1',
      location: location('local', 'file:///Users/erik/photo.png'),
    });
    const editing = editingClientStub();
    editing.getFinderTags.mockResolvedValue({
      tags: [
        { name: 'Important', color: 'red' },
        { name: 'Done', color: 'green' },
      ],
    } satisfies FinderTags);

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, {
          open: true,
          entries: [localEntry],
          client: { getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(localEntry.id)) },
          editingClient: editing as unknown as PropertiesEditingClient,
          runtimeCapabilities: capabilities({ finderTags: true }),
          finderTagsLoader: setCachedSpy().loader,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await flush();

    expect(root.querySelector('.fm-properties-tags')).not.toBeNull();
    expect(root.textContent).toContain('Important');
    expect(root.textContent).toContain('Done');
    expect(editing.getFinderTags).toHaveBeenCalledWith(localEntry.location.uri, expect.anything());
  });

  it('hides the Tags section when finder tags are not supported', async () => {
    const localEntry = entry({
      id: 'tags-2',
      location: location('local', 'file:///Users/erik/photo.png'),
    });
    const editing = editingClientStub();

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, {
          open: true,
          entries: [localEntry],
          client: { getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(localEntry.id)) },
          editingClient: editing as unknown as PropertiesEditingClient,
          runtimeCapabilities: capabilities({ finderTags: false }),
          finderTagsLoader: setCachedSpy().loader,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await flush();

    expect(root.querySelector('.fm-properties-tags')).toBeNull();
    expect(editing.getFinderTags).not.toHaveBeenCalled();
  });

  it('shows an editable Spotlight comment and persists a change', async () => {
    const localEntry = entry({
      id: 'comment-1',
      location: location('local', 'file:///Users/erik/notes.txt'),
    });
    const editing = editingClientStub();
    editing.getSpotlightComment.mockResolvedValue({
      comment: 'initial',
    } satisfies SpotlightComment);

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, {
          open: true,
          entries: [localEntry],
          client: { getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(localEntry.id)) },
          editingClient: editing as unknown as PropertiesEditingClient,
          runtimeCapabilities: capabilities({ extendedAttributes: true }),
          finderTagsLoader: setCachedSpy().loader,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await flush();

    const textarea = root.querySelector<HTMLTextAreaElement>('#spotlight-comment-text');
    expect(textarea?.value).toBe('initial');

    const field = textarea as HTMLTextAreaElement;
    field.value = 'updated';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, bubbles: true }),
    );
    await flush();

    expect(editing.setSpotlightComment).toHaveBeenCalledWith(
      localEntry.location.uri,
      { comment: 'updated' },
      expect.anything(),
    );
    expect(root.querySelector('.fm-properties-save-comment')?.textContent).toContain('Save');
  });

  it('hides the Comment section when extended attributes are not supported', async () => {
    const localEntry = entry({
      id: 'comment-2',
      location: location('local', 'file:///Users/erik/notes.txt'),
    });
    const editing = editingClientStub();

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, {
          open: true,
          entries: [localEntry],
          client: { getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(localEntry.id)) },
          editingClient: editing as unknown as PropertiesEditingClient,
          runtimeCapabilities: capabilities({ extendedAttributes: false }),
          finderTagsLoader: setCachedSpy().loader,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await flush();

    expect(root.querySelector('#spotlight-comment-text')).toBeNull();
    expect(editing.getSpotlightComment).not.toHaveBeenCalled();
  });

  it('round-trips a tag edit through the nested editor, updating the display and the loader cache', async () => {
    const localEntry = entry({
      id: 'tags-3',
      location: location('local', 'file:///Users/erik/photo.png'),
    });
    const editing = editingClientStub();
    editing.getFinderTags.mockResolvedValue({ tags: [] } satisfies FinderTags);
    editing.setFinderTags.mockImplementation((_uri: string, request: FinderTags) =>
      Promise.resolve(request),
    );
    const spy = setCachedSpy();

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, {
          open: true,
          entries: [localEntry],
          client: { getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(localEntry.id)) },
          editingClient: editing as unknown as PropertiesEditingClient,
          runtimeCapabilities: capabilities({ finderTags: true }),
          finderTagsLoader: spy.loader,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await flush();

    // Open the nested tag editor and add a tag.
    root.querySelector<HTMLButtonElement>('.fm-properties-edit-tags')?.click();
    m.redraw.sync();
    await flush();
    const nameInput = root.querySelector<HTMLInputElement>('#finder-tags-new-name');
    if (nameInput === null) throw new Error('tag name input missing');
    nameInput.value = 'Urgent';
    nameInput.dispatchEvent(new Event('input', { bubbles: true }));
    m.redraw.sync();
    root
      .querySelector<HTMLButtonElement>('.fm-finder-tags-add-button')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    m.redraw.sync();
    await flush();
    expect(root.textContent).toContain('Urgent');

    // Confirm the editor (its own Save button, not the comment section's).
    root
      .querySelector<HTMLButtonElement>('.fm-finder-tags-save')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await flush();

    expect(editing.setFinderTags).toHaveBeenCalledWith(
      localEntry.location.uri,
      { tags: [{ name: 'Urgent', color: 'none' }] },
      expect.anything(),
    );
    expect(root.textContent).toContain('Urgent');
    expect(spy.callUri()).toBe(localEntry.location.uri);
  });

  it('offers "Change attributes..." only when the host provides it and passes the entries on', async () => {
    const localEntry = entry({ id: 'local-3', location: location('local', 'file:///y') });
    const client: PropertiesMetadataClient = {
      getEntryMetadata: vi.fn().mockResolvedValue(emptyMetadata(localEntry.id)),
    };
    const labelled = () =>
      Array.from(root.querySelectorAll('button')).find((button) =>
        button.textContent?.includes('Change attributes'),
      );

    m.mount(root, {
      view: () =>
        m(PropertiesDialog, { open: true, entries: [localEntry], client, onCancel: vi.fn() }),
    });
    m.redraw.sync();
    await flush();
    expect(labelled()).toBeUndefined();

    const onChangeAttributes = vi.fn();
    m.mount(root, {
      view: () =>
        m(PropertiesDialog, {
          open: true,
          entries: [localEntry],
          client,
          onCancel: vi.fn(),
          onChangeAttributes,
        }),
    });
    m.redraw.sync();
    await flush();
    labelled()?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onChangeAttributes).toHaveBeenCalledWith([localEntry]);
  });
});
