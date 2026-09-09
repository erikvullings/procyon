/** Decodes an indexed URI path segment without hiding malformed source metadata. */
export function decodeEvidenceTitle(title: string | null | undefined): string | undefined {
  if (title == null) return undefined;
  try {
    return decodeURIComponent(title);
  } catch {
    return title;
  }
}
