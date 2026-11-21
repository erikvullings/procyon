/**
 * Obsidian-style `[[wikilinks]]`: `[[Note]]`, `[[Note|label]]`, `[[Note#Heading]]` and the
 * `![[embed]]` form (shown as a link). They become `<a data-fm-wiki="…">` anchors, which the
 * Markdown views resolve by name when clicked (see `markdown-links.ts`). Fenced code blocks and
 * inline code spans are left untouched.
 */

/** `!?[[target#fragment|label]]`; the target may not start with whitespace (`[[ -f x ]]`). */
const WIKILINK = /!?\[\[(?!\s)([^\]|#\n]+)(#[^\]|\n]*)?(?:\|([^\]\n]+))?\]\]/g;
const FENCE = /^[ \t]*(```|~~~)/;
const INLINE_CODE = /(`+)[\s\S]*?\1/g;

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function anchor(target: string, fragment: string | undefined, label: string | undefined): string {
  const name = target.trim();
  const heading = fragment?.slice(1).trim() ?? '';
  const text = label?.trim() || (heading === '' ? name : `${name} › ${heading}`);
  const spec = heading === '' ? name : `${name}#${heading}`;
  return `<a href="#" class="fm-md-wikilink" data-fm-wiki="${escapeHtml(spec)}">${escapeHtml(text)}</a>`;
}

function replaceWikilinks(text: string): string {
  return text.replace(
    WIKILINK,
    (_match: string, target: string, fragment: string | undefined, label: string | undefined) =>
      anchor(target, fragment, label),
  );
}

function replaceOutsideInlineCode(line: string): string {
  let out = '';
  let last = 0;
  for (const match of line.matchAll(INLINE_CODE)) {
    const start = match.index;
    out += replaceWikilinks(line.slice(last, start));
    out += match[0];
    last = start + match[0].length;
  }
  return out + replaceWikilinks(line.slice(last));
}

/** Replaces every wikilink outside code with an HTML anchor; other Markdown is unchanged. */
export function wikilinksToHtml(markdown: string): string {
  if (!markdown.includes('[[')) return markdown;
  let fence: string | undefined;
  return markdown
    .split('\n')
    .map((line) => {
      const opener = FENCE.exec(line)?.[1];
      if (fence !== undefined) {
        if (opener === fence) fence = undefined;
        return line;
      }
      if (opener !== undefined) {
        fence = opener;
        return line;
      }
      return replaceOutsideInlineCode(line);
    })
    .join('\n');
}
