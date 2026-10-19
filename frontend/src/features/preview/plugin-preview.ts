import DOMPurify from 'dompurify';
import type { EntrySummary, PluginDescriptor } from '../../models';
import { editableLanguageForExtension } from '../editor/editor-language';
import { PREVIEW_SANITIZE_OPTIONS } from '../editor/markdown-preview';
import { previewExtension } from './content-preview';

/** An enabled plugin preview renderer and the extensions it claims (task 0142). */
export interface PreviewPluginRoute {
  readonly pluginId: string;
  readonly extensions: readonly string[];
}

/** Plain-text extensions the built-in text renderer owns even though they have no language mode. */
const BUILT_IN_PLAIN_TEXT_EXTENSIONS: readonly string[] = ['txt', 'log'];

/** Routes for every enabled plugin that contributes a preview. */
export function previewPluginRoutes(plugins: readonly PluginDescriptor[]): PreviewPluginRoute[] {
  return plugins.flatMap((plugin) =>
    plugin.enabled && plugin.preview != null
      ? [{ pluginId: plugin.id, extensions: plugin.preview.extensions }]
      : [],
  );
}

/**
 * The plugin that renders `entry`, if any. Built-in renderers always win: callers must only ask
 * for entries no built-in media/document/structured renderer claimed (`resolvePreviewKind` is
 * `text` and the structured viewer declined), and this additionally refuses extensions the text
 * viewer already highlights, so a plugin can only fill genuinely unhandled extensions.
 */
export function previewPluginFor(
  entry: EntrySummary,
  routes: readonly PreviewPluginRoute[],
): PreviewPluginRoute | undefined {
  const extension = previewExtension(entry);
  if (extension === undefined || BUILT_IN_PLAIN_TEXT_EXTENSIONS.includes(extension)) {
    return undefined;
  }
  if (editableLanguageForExtension(extension, entry.name) !== 'text') {
    return undefined;
  }
  return routes.find((route) => route.extensions.includes(extension));
}

/**
 * Sanitizes plugin-supplied HTML with the Markdown/EPUB DOMPurify configuration, and additionally
 * forbids every element that would fetch a remote resource: previewed files are never executed
 * and a preview must not phone home (task 0071's invariant, extended to plugins).
 */
export function sanitizePluginPreviewHtml(html: string): string {
  return DOMPurify.sanitize(html, {
    ...PREVIEW_SANITIZE_OPTIONS,
    FORBID_TAGS: [
      ...PREVIEW_SANITIZE_OPTIONS.FORBID_TAGS,
      'img',
      'input',
      'button',
      'textarea',
      'select',
      'area',
      'picture',
      'source',
      'video',
      'audio',
      'track',
      'link',
      'base',
      'meta',
    ],
    // Attributes that fetch a resource or navigate; links stay as inert text so a preview can
    // neither phone home nor move the app's webview away from the file manager.
    FORBID_ATTR: [
      ...PREVIEW_SANITIZE_OPTIONS.FORBID_ATTR,
      'href',
      'src',
      'srcset',
      'poster',
      'background',
      'action',
      'formaction',
      'xlink:href',
    ],
  });
}
