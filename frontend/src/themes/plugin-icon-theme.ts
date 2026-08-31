import m from 'mithril';
import type { FileManagerClient } from '../api/client/file-manager-client';
import type { IconAttrs } from '../components/icons';
import {
  type EntryIconRegistry,
  type EntryIconRenderer,
  entryIconRegistry,
  restoreDefaultIconTheme,
} from '../features/directory-table/entry-icons';
import type { PluginId } from '../models/ids';
import type { PluginIconTheme } from '../models/plugin';
import { sanitizeSvgMarkup } from './svg-sanitizer';

export { restoreDefaultIconTheme };

function setCaseInsensitiveAlias(
  map: Map<string, EntryIconRenderer>,
  key: string,
  renderer: EntryIconRenderer,
): void {
  const lower = key.toLowerCase();
  if (lower !== key && !map.has(lower)) {
    map.set(lower, renderer);
  }
}

/** Turns an icon-definition key into a safe CSS class fragment (keys may contain e.g. `.`/spaces). */
function cssClassFromKey(definitionKey: string): string {
  return `fm-icon-plugin-${definitionKey.toLowerCase().replace(/[^a-z0-9-]+/g, '-')}`;
}

function rendererFromSvgMarkup(markup: string, extraClass: string): EntryIconRenderer {
  const sanitized = sanitizeSvgMarkup(markup);
  return (attrs?: IconAttrs): m.Children => {
    const size = attrs?.size ?? 16;
    return m(
      `svg.fm-icon.fm-icon-plugin.${extraClass}${
        attrs?.className === undefined ? '' : `.${attrs.className}`
      }`,
      {
        'aria-hidden': 'true',
        viewBox: sanitized.viewBox,
        width: size,
        height: size,
      },
      // Safe: `sanitized.innerMarkup` has been reduced to an allow-list of elements/attributes by
      // `sanitizeSvgMarkup` (task 0095) before reaching this `m.trust()`.
      m.trust(sanitized.innerMarkup),
    );
  };
}

/**
 * Fetches, sanitizes, and installs a discovered plugin's icon theme into `registry` (task 0095) —
 * the generic, data-driven replacement for `installCatppuccinIconTheme`'s hardcoded TypeScript.
 * Each referenced SVG asset is fetched and sanitized once (cached by icon-definition key) rather
 * than per render. Definitions/mappings that reference an unknown key are silently skipped rather
 * than failing the whole install, matching the "invalid plugin content degrades gracefully"
 * pattern used elsewhere in this codebase.
 */
export async function installPluginIconTheme(
  client: FileManagerClient,
  pluginId: PluginId,
  iconTheme: PluginIconTheme,
  registry: EntryIconRegistry = entryIconRegistry,
): Promise<void> {
  const rendererByDefinitionKey = new Map<string, EntryIconRenderer>();

  async function rendererFor(
    definitionKey: string | undefined,
  ): Promise<EntryIconRenderer | undefined> {
    if (definitionKey === undefined) return undefined;
    const cached = rendererByDefinitionKey.get(definitionKey);
    if (cached !== undefined) return cached;
    const definition = iconTheme.iconDefinitions[definitionKey];
    if (definition === undefined) return undefined;
    const markup = await client.getPluginIconThemeAsset(pluginId, definition.iconPath);
    const renderer = rendererFromSvgMarkup(markup, cssClassFromKey(definitionKey));
    rendererByDefinitionKey.set(definitionKey, renderer);
    return renderer;
  }

  const folderRenderer = await rendererFor(iconTheme.folder);
  if (folderRenderer !== undefined) registry.kindIcons.set('directory', folderRenderer);

  const fileRenderer = await rendererFor(iconTheme.file);
  if (fileRenderer !== undefined) registry.kindIcons.set('file', fileRenderer);

  const symlinkRenderer = await rendererFor(iconTheme.symlink);
  if (symlinkRenderer !== undefined) registry.kindIcons.set('symlink', symlinkRenderer);

  for (const [extension, definitionKey] of Object.entries(iconTheme.fileExtensions)) {
    const renderer = await rendererFor(definitionKey);
    if (renderer !== undefined) registry.extensionIcons.set(extension, renderer);
  }

  for (const [fileName, definitionKey] of Object.entries(iconTheme.fileNames)) {
    const renderer = await rendererFor(definitionKey);
    if (renderer !== undefined) {
      registry.fileNameIcons.set(fileName, renderer);
      setCaseInsensitiveAlias(registry.fileNameIcons, fileName, renderer);
    }
  }

  for (const [folderName, definitionKey] of Object.entries(iconTheme.folderNames ?? {})) {
    const renderer = await rendererFor(definitionKey);
    if (renderer !== undefined) {
      registry.folderNameIcons.set(folderName, renderer);
      setCaseInsensitiveAlias(registry.folderNameIcons, folderName, renderer);
    }
  }

  for (const [prefix, definitionKey] of Object.entries(iconTheme.mimePrefixes)) {
    const renderer = await rendererFor(definitionKey);
    if (renderer !== undefined) registry.mimePrefixIcons.set(prefix, renderer);
  }
}
