/**
 * Builds the document shown in the F3 viewer's sandboxed HTML preview.
 *
 * A previewed file is untrusted: the iframe carries an empty `sandbox` attribute (no scripts,
 * forms, popups or same-origin access) and this Content-Security-Policy additionally blocks every
 * network and script load, so opening a downloaded page never contacts a server or leaks that it
 * was opened. Inline styles and `data:` images still render; relative resources do not, because
 * the preview has no base URL to resolve them against.
 */
const PREVIEW_CSP =
  "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:; media-src data:";

/** The `srcdoc` for `html`: the file's own markup preceded by the restrictive policy. */
export function sandboxedHtmlDocument(html: string): string {
  return `<meta http-equiv="Content-Security-Policy" content="${PREVIEW_CSP}">${html}`;
}

/** Extensions the viewer renders as an HTML preview with a source toggle. */
export function isHtmlPreviewExtension(extension: string | undefined): boolean {
  const lower = extension?.toLowerCase();
  return lower === 'html' || lower === 'htm';
}
