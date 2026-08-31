import { describe, expect, it } from 'vitest';
import { archiveIcon, fileIcon, folderIcon, imageIcon, symlinkIcon } from '../../components/icons';
import type { EntrySummary } from '../../models';
import {
  createDefaultEntryIconRegistry,
  entryIcon,
  hasSpecificEntryIcon,
  resolveEntryIcon,
} from './entry-icons';

function entry(overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: 'entry-1',
    location: { providerId: 'file', uri: 'mock:///report.txt' },
    name: 'report.txt',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...overrides,
  };
}

describe('resolveEntryIcon', () => {
  const registry = createDefaultEntryIconRegistry();

  it('resolves directories to the folder icon regardless of extension', () => {
    expect(resolveEntryIcon(entry({ kind: 'directory', extension: 'zip' }), registry)).toBe(
      folderIcon,
    );
  });

  it('resolves symlinks to the symlink icon', () => {
    expect(resolveEntryIcon(entry({ kind: 'symlink' }), registry)).toBe(symlinkIcon);
  });

  it('resolves a known extension to its themed icon', () => {
    expect(resolveEntryIcon(entry({ extension: 'png' }), registry)).toBe(imageIcon);
    expect(resolveEntryIcon(entry({ extension: 'ZIP' }), registry)).toBe(archiveIcon);
  });

  it('prefers an exact file-name icon over the extension icon', () => {
    const named = createDefaultEntryIconRegistry();
    named.fileNameIcons.set('Cargo.lock', folderIcon);

    expect(resolveEntryIcon(entry({ name: 'Cargo.lock', extension: 'lock' }), named)).toBe(
      folderIcon,
    );
    // Exact means exact: a different casing keeps the extension icon.
    expect(resolveEntryIcon(entry({ name: 'other.zip', extension: 'zip' }), named)).toBe(
      archiveIcon,
    );
    expect(hasSpecificEntryIcon(entry({ name: 'Cargo.lock', extension: 'lock' }), named)).toBe(
      true,
    );
  });

  it('falls back to a MIME type prefix when the extension has no registered icon', () => {
    expect(
      resolveEntryIcon(entry({ extension: 'bin', mimeType: 'image/x-custom' }), registry),
    ).toBe(imageIcon);
  });

  it('falls back to the generic file icon for unknown extensions and MIME types', () => {
    expect(resolveEntryIcon(entry({ extension: 'xyz' }), registry)).toBe(fileIcon);
    expect(resolveEntryIcon(entry(), registry)).toBe(fileIcon);
  });

  it('distinguishes specific theme mappings from generic kind fallbacks', () => {
    expect(hasSpecificEntryIcon(entry({ extension: 'png' }), registry)).toBe(true);
    expect(
      hasSpecificEntryIcon(entry({ extension: 'bin', mimeType: 'image/custom' }), registry),
    ).toBe(true);
    expect(hasSpecificEntryIcon(entry({ extension: 'unknown' }), registry)).toBe(false);
    expect(hasSpecificEntryIcon(entry({ kind: 'directory' }), registry)).toBe(false);

    const themedFolder = () => 'themed-folder';
    const themedFile = () => 'themed-file';
    registry.kindIcons.set('directory', themedFolder);
    registry.kindIcons.set('file', themedFile);
    expect(hasSpecificEntryIcon(entry({ kind: 'directory' }), registry)).toBe(true);
    expect(hasSpecificEntryIcon(entry({ extension: 'unknown' }), registry)).toBe(true);
  });

  it('lets a theme override an extension without editing directory-table.ts', () => {
    const customIcon = () => 'custom-icon';
    registry.extensionIcons.set('pdf', customIcon);

    expect(resolveEntryIcon(entry({ extension: 'pdf' }), registry)).toBe(customIcon);
  });
});

describe('entryIcon', () => {
  it('renders using the shared default registry', () => {
    const rendered = entryIcon(entry({ kind: 'directory' }), { className: 'fm-entry-icon' });
    expect(rendered).toEqual(folderIcon({ className: 'fm-entry-icon' }));
  });
});
