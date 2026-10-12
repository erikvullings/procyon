/** Returns the one-based preview page encoded by structural knowledge provenance. */
export function knowledgeEvidencePage(provenance: string): number | undefined {
  let value: unknown;
  try {
    value = JSON.parse(provenance);
  } catch {
    return undefined;
  }

  const pageFrom = (candidate: unknown): number | undefined => {
    if (candidate === null || typeof candidate !== 'object' || Array.isArray(candidate)) {
      return undefined;
    }
    const fields = candidate as Record<string, unknown>;
    if (fields.kind === 'exact') return pageFrom(fields.value);
    if (fields.kind === 'span') return pageFrom(fields.first) ?? pageFrom(fields.last);
    const page =
      fields.page_number ?? fields.pageNumber ?? fields.slide_number ?? fields.slideNumber;
    return typeof page === 'number' && Number.isInteger(page) && page > 0 ? page : undefined;
  };
  return pageFrom(value);
}

function exactPdfEvidence(provenance: string): Record<string, unknown> | undefined {
  let value: unknown;
  try {
    value = JSON.parse(provenance);
  } catch {
    return undefined;
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined;
  const wrapper = value as Record<string, unknown>;
  if (wrapper.kind !== 'exact') return undefined;
  const detail = wrapper.value;
  return detail !== null && typeof detail === 'object' && !Array.isArray(detail)
    ? (detail as Record<string, unknown>)
    : undefined;
}

/** Identifies an exact reusable PDF image without treating the source PDF as image bytes. */
export function pdfImageProvenance(
  provenance: string,
): { readonly page: number; readonly image: number } | undefined {
  const fields = exactPdfEvidence(provenance);
  if (fields === undefined) return undefined;
  const page = fields.page_number;
  const image = fields.image_index;
  return fields.kind === 'pdfImage' &&
    typeof page === 'number' &&
    Number.isInteger(page) &&
    page > 0 &&
    typeof image === 'number' &&
    Number.isInteger(image) &&
    image > 0
    ? { page, image }
    : undefined;
}

/** Identifies a whole-page raster without claiming an embedded image XObject. */
export function pdfPageRenderProvenance(provenance: string): { readonly page: number } | undefined {
  const fields = exactPdfEvidence(provenance);
  const page = fields?.page_number;
  return fields?.kind === 'pdfPageRender' &&
    typeof page === 'number' &&
    Number.isInteger(page) &&
    page > 0
    ? { page }
    : undefined;
}
