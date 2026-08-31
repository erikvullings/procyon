import { describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../api/client/file-manager-client';
import {
  createDefaultEntryIconRegistry,
  type EntryIconRegistry,
  resolveEntryIcon,
} from '../features/directory-table/entry-icons';
import type { PluginIconTheme } from '../models/plugin';
import { installPluginIconTheme, restoreDefaultIconTheme } from './plugin-icon-theme';

const SAFE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M1 1h2v2z" /></svg>';
const HOSTILE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script><path d="M0 0" onclick="alert(2)" /></svg>';

function freshRegistry(): EntryIconRegistry {
  return createDefaultEntryIconRegistry();
}

function stubClient(assetsByPath: Readonly<Record<string, string>>): FileManagerClient {
  const getPluginIconThemeAsset = vi.fn(async (_pluginId: string, path: string) => {
    const markup = assetsByPath[path];
    if (markup === undefined) throw new Error(`unknown asset path ${path}`);
    return markup;
  });
  return { getPluginIconThemeAsset } as unknown as FileManagerClient;
}

const SAMPLE_THEME: PluginIconTheme = {
  iconDefinitions: {
    folder: { iconPath: 'icons/folder.svg' },
    file: { iconPath: 'icons/file.svg' },
    typescript: { iconPath: 'icons/ts.svg' },
  },
  folder: 'folder',
  file: 'file',
  fileExtensions: { ts: 'typescript' },
  fileNames: { 'Cargo.toml': 'typescript' },
  mimePrefixes: { 'image/': 'file' },
};

describe('installPluginIconTheme', () => {
  it('fetches referenced assets and installs kind/extension/mime-prefix icons', async () => {
    const registry = freshRegistry();
    const defaultFolderIcon = registry.kindIcons.get('directory');
    const client = stubClient({
      'icons/folder.svg': SAFE_SVG,
      'icons/file.svg': SAFE_SVG,
      'icons/ts.svg': SAFE_SVG,
    });

    await installPluginIconTheme(client, 'sample.icons', SAMPLE_THEME, registry);

    expect(registry.kindIcons.get('directory')).not.toBe(defaultFolderIcon);
    expect(registry.kindIcons.get('file')).toBeDefined();
    expect(registry.extensionIcons.get('ts')).toBeDefined();
    expect(registry.fileNameIcons.get('Cargo.toml')).toBeDefined();
    expect(registry.mimePrefixIcons.get('image/')).toBeDefined();
    expect(client.getPluginIconThemeAsset).toHaveBeenCalledWith('sample.icons', 'icons/folder.svg');
  });

  it('caches a definition fetch instead of refetching for repeated references', async () => {
    const registry = freshRegistry();
    const client = stubClient({
      'icons/folder.svg': SAFE_SVG,
      'icons/file.svg': SAFE_SVG,
      'icons/ts.svg': SAFE_SVG,
    });

    await installPluginIconTheme(client, 'sample.icons', SAMPLE_THEME, registry);

    // `file` backs both the default file icon and the image/ mime-prefix fallback.
    expect(client.getPluginIconThemeAsset).toHaveBeenCalledTimes(3);
  });

  it('sanitizes hostile SVG markup before installing it', async () => {
    const registry = freshRegistry();
    const client = stubClient({ 'icons/folder.svg': HOSTILE_SVG });
    const theme: PluginIconTheme = {
      iconDefinitions: { folder: { iconPath: 'icons/folder.svg' } },
      folder: 'folder',
      fileExtensions: {},
      fileNames: {},
      mimePrefixes: {},
    };

    await installPluginIconTheme(client, 'sample.icons', theme, registry);

    const renderer = registry.kindIcons.get('directory');
    expect(renderer).toBeDefined();
    const rendered = renderer?.();
    const trustedHtml = (rendered as { children?: readonly { children?: string }[] })?.children?.[0]
      ?.children;
    expect(trustedHtml ?? '').not.toContain('script');
    expect(trustedHtml ?? '').not.toContain('onclick');
  });

  it('skips a mapping that references an unknown icon-definition key', async () => {
    const registry = freshRegistry();
    const client = stubClient({ 'icons/file.svg': SAFE_SVG });
    const theme: PluginIconTheme = {
      iconDefinitions: { file: { iconPath: 'icons/file.svg' } },
      fileExtensions: { rs: 'missing-definition' },
      fileNames: {},
      mimePrefixes: {},
    };

    await installPluginIconTheme(client, 'sample.icons', theme, registry);

    expect(registry.extensionIcons.has('rs')).toBe(false);
  });

  it('defaults to mutating the shared entryIconRegistry singleton', async () => {
    const { entryIconRegistry } = await import('../features/directory-table/entry-icons');
    const defaultFolderIcon = entryIconRegistry.kindIcons.get('directory');
    const client = stubClient({ 'icons/folder.svg': SAFE_SVG });
    const theme: PluginIconTheme = {
      iconDefinitions: { folder: { iconPath: 'icons/folder.svg' } },
      folder: 'folder',
      fileExtensions: {},
      fileNames: {},
      mimePrefixes: {},
    };

    await installPluginIconTheme(client, 'sample.icons', theme);
    expect(entryIconRegistry.kindIcons.get('directory')).not.toBe(defaultFolderIcon);

    restoreDefaultIconTheme();
    expect(entryIconRegistry.kindIcons.get('directory')).toBe(defaultFolderIcon);
  });

  it('treats file and folder name mappings as case-insensitive by default', async () => {
    const registry = freshRegistry();
    const client = stubClient({
      'icons/folder.svg': SAFE_SVG,
      'icons/ts.svg': SAFE_SVG,
    });
    const theme: PluginIconTheme = {
      iconDefinitions: {
        folder: { iconPath: 'icons/folder.svg' },
        typescript: { iconPath: 'icons/ts.svg' },
      },
      fileExtensions: {},
      fileNames: { 'Cargo.toml': 'typescript' },
      folderNames: { Downloads: 'folder' },
      mimePrefixes: {},
    };

    await installPluginIconTheme(client, 'sample.icons', theme, registry);

    const fileIconByLower = resolveEntryIcon(
      {
        kind: 'file',
        id: '1',
        location: { providerId: 'file', uri: 'file:///tmp/cargo.toml' },
        name: 'cargo.toml',
        extension: 'toml',
        hidden: false,
        readOnly: false,
        metadataRevision: 0,
      },
      registry,
    );
    expect(fileIconByLower).toBe(registry.fileNameIcons.get('Cargo.toml'));

    const folderIconByLower = resolveEntryIcon(
      {
        kind: 'directory',
        id: '2',
        location: { providerId: 'file', uri: 'file:///tmp/downloads' },
        name: 'downloads',
        hidden: false,
        readOnly: false,
        metadataRevision: 0,
      },
      registry,
    );
    expect(folderIconByLower).toBe(registry.folderNameIcons.get('Downloads'));
  });
});

describe('restoreDefaultIconTheme (re-exported)', () => {
  it('restores the built-in default icon set after a plugin theme install', async () => {
    const registry = freshRegistry();
    const defaults = createDefaultEntryIconRegistry();
    const client = stubClient({
      'icons/folder.svg': SAFE_SVG,
      'icons/file.svg': SAFE_SVG,
      'icons/ts.svg': SAFE_SVG,
    });

    await installPluginIconTheme(client, 'sample.icons', SAMPLE_THEME, registry);
    restoreDefaultIconTheme(registry);

    expect(registry.kindIcons.get('directory')).toBe(defaults.kindIcons.get('directory'));
    expect(registry.extensionIcons.has('ts')).toBe(false);
  });
});
