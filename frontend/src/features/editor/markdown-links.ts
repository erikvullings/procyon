import { toast } from 'mithril-materialized';
import { t } from '../../i18n';
import type { EntrySummary } from '../../models';
import { type EditorLinkHandlers, isSafeExternalUrl, type ResolvedLink } from './link-actions';
import type { DetectedLink } from './link-detection';

/** What a clicked anchor in rendered Markdown points at. */
export type MarkdownLinkTarget =
  | { readonly kind: 'url'; readonly url: string }
  /** A relative or absolute path, e.g. `[x](other.md)`; any `#fragment` is dropped. */
  | { readonly kind: 'path'; readonly path: string }
  /** An Obsidian `[[wikilink]]` target, without its `#heading`. */
  | { readonly kind: 'wiki'; readonly name: string }
  /** A four-digit task ID from a TASKS file's `Depends on` line. */
  | { readonly kind: 'task'; readonly id: string };

const MARKDOWN_EXTENSIONS = ['.md', '.markdown'];

/** Reads the target of an anchor produced by `safeMarkdownHtml`; `undefined` for `#fragment`. */
export function markdownLinkTarget(anchor: Element): MarkdownLinkTarget | undefined {
  const task = anchor.getAttribute('data-fm-task');
  if (task !== null) return { kind: 'task', id: task };
  const wiki = anchor.getAttribute('data-fm-wiki');
  if (wiki !== null) {
    const name = wiki.split('#')[0]?.trim() ?? '';
    return name === '' ? undefined : { kind: 'wiki', name };
  }
  const href = anchor.getAttribute('href')?.trim() ?? '';
  if (href === '' || href.startsWith('#')) return undefined;
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) && !/^[a-z]:[\\/]/i.test(href)) {
    return { kind: 'url', url: href };
  }
  const path = decodePath(href.split('#')[0] ?? '');
  return path === '' ? undefined : { kind: 'path', path };
}

function decodePath(path: string): string {
  try {
    return decodeURIComponent(path);
  } catch {
    return path;
  }
}

function stem(name: string): string {
  const lower = name.toLowerCase();
  const extension = MARKDOWN_EXTENSIONS.find((candidate) => lower.endsWith(candidate));
  return extension === undefined ? lower : lower.slice(0, -extension.length);
}

function hasExtension(name: string): boolean {
  return /\.[a-z0-9]{1,8}$/i.test(name);
}

/**
 * The sibling a task ID or a bare wikilink name points at: `0336` matches `0336.md` or
 * `0336-anything.md`; `My Note` matches `my note.md` (case-insensitive), or `My Note.png` when
 * written with its extension. Markdown files win over other matches.
 */
export function matchSibling(
  target: Extract<MarkdownLinkTarget, { kind: 'task' | 'wiki' }>,
  siblings: readonly EntrySummary[],
): EntrySummary | undefined {
  const files = siblings.filter((entry) => entry.kind !== 'directory');
  if (target.kind === 'task') {
    return files.find((entry) => {
      const name = stem(entry.name);
      return (
        name !== entry.name.toLowerCase() &&
        (name === target.id || name.startsWith(`${target.id}-`))
      );
    });
  }
  const wanted = target.name.toLowerCase();
  if (hasExtension(target.name) && !MARKDOWN_EXTENSIONS.some((ext) => wanted.endsWith(ext))) {
    return files.find((entry) => entry.name.toLowerCase() === wanted);
  }
  return files.find((entry) => stem(entry.name) === stem(wanted));
}

function pathLink(path: string): DetectedLink {
  return { kind: 'path', path, line: undefined, column: undefined, from: 0, to: 0 };
}

/** Path candidates for a wikilink that is not a sibling, e.g. `[[sub/Note]]` → `sub/Note.md`. */
function wikiPaths(name: string): string[] {
  return hasExtension(name) ? [name] : [`${name}.md`, name];
}

/** Resolves a Markdown link to an existing file, folder or URL; `undefined` when nothing matches. */
export async function resolveMarkdownLink(
  target: MarkdownLinkTarget,
  handlers: Pick<EditorLinkHandlers, 'resolve' | 'siblings'>,
): Promise<ResolvedLink | undefined> {
  if (target.kind === 'url') return handlers.resolve({ ...target, from: 0, to: 0 });
  if (target.kind === 'path') return handlers.resolve(pathLink(target.path));
  const sibling = matchSibling(target, handlers.siblings?.() ?? []);
  if (sibling !== undefined) return handlers.resolve(pathLink(sibling.name));
  if (target.kind === 'task') return undefined;
  for (const path of wikiPaths(target.name)) {
    const resolved = await handlers.resolve(pathLink(path));
    if (resolved !== undefined) return resolved;
  }
  return undefined;
}

/**
 * Acts on a link clicked in rendered Markdown: web links go to `openExternal` (which may ask
 * first), files open in the viewer when the handlers can, folders open in a pane.
 */
export async function followMarkdownLink(
  target: MarkdownLinkTarget,
  handlers: EditorLinkHandlers,
  openExternal: (url: string) => void,
  onUnavailable: (target: MarkdownLinkTarget) => void,
): Promise<void> {
  if (target.kind === 'url') {
    if (isSafeExternalUrl(target.url)) openExternal(target.url);
    else onUnavailable(target);
    return;
  }
  const resolved = await resolveMarkdownLink(target, handlers);
  if (resolved === undefined || resolved.kind !== 'path') {
    onUnavailable(target);
    return;
  }
  if (resolved.entry.kind !== 'directory' && handlers.view !== undefined) handlers.view(resolved);
  else handlers.openInPane(resolved);
}

/**
 * A click handler for a container of rendered Markdown: every anchor click is intercepted (so the
 * web view itself never navigates) and followed through `handlers`; a target that cannot be
 * found or opened shows a toast.
 */
export function markdownLinkClickHandler(
  handlers: EditorLinkHandlers | undefined,
  openExternal: (url: string) => void,
): (event: MouseEvent) => void {
  return (event) => {
    const anchor =
      event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href]') : null;
    if (anchor === null) return;
    event.preventDefault();
    const target = markdownLinkTarget(anchor);
    if (target === undefined || handlers === undefined) return;
    void followMarkdownLink(target, handlers, openExternal, (missed) => {
      toast({ html: t('editorLinks', missed.kind === 'url' ? 'unsafeUrl' : 'notFound') });
    });
  };
}
