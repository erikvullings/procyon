/**
 * Minimal allow-list SVG sanitizer (task 0095).
 *
 * Icon-theme plugin assets are untrusted third-party SVG markup fetched over the network, unlike
 * the vendored, build-time-constant strings `catppuccin-icons.ts` passes to `m.trust()`. Before
 * any plugin-sourced SVG reaches `m.trust()`, it must be parsed and reduced to a small allow-list
 * of elements/attributes — this strips `<script>`, `<foreignObject>`, event-handler (`on*`)
 * attributes, and any other markup capable of executing script or loading external resources
 * (e.g. `href`/`xlink:href`), regardless of nesting depth.
 */

const ALLOWED_ELEMENTS: ReadonlySet<string> = new Set([
  'svg',
  'path',
  'g',
  'circle',
  'rect',
  'polygon',
]);

const ALLOWED_ATTRIBUTES: ReadonlySet<string> = new Set([
  'viewbox',
  'width',
  'height',
  'fill',
  'stroke',
  'stroke-width',
  'stroke-linecap',
  'stroke-linejoin',
  'stroke-dasharray',
  'stroke-miterlimit',
  'fill-rule',
  'clip-rule',
  'opacity',
  'fill-opacity',
  'stroke-opacity',
  'd',
  'cx',
  'cy',
  'r',
  'x',
  'y',
  'rx',
  'ry',
  'points',
  'transform',
]);

const DEFAULT_VIEW_BOX = '0 0 16 16';

/** A sanitized icon ready to render: `viewBox` plus the inner (non-`<svg>`) markup. */
export interface SanitizedSvg {
  readonly viewBox: string;
  readonly innerMarkup: string;
}

function sanitizeElementInPlace(element: Element): void {
  for (const child of Array.from(element.children)) {
    if (!ALLOWED_ELEMENTS.has(child.tagName.toLowerCase())) {
      element.removeChild(child);
      continue;
    }
    sanitizeElementInPlace(child);
  }
  for (const attribute of Array.from(element.attributes)) {
    if (!ALLOWED_ATTRIBUTES.has(attribute.name.toLowerCase())) {
      element.removeAttribute(attribute.name);
    }
  }
}

/**
 * Parses `markup`, strips everything outside the element/attribute allow-list, and returns the
 * sanitized inner markup plus the source `viewBox` (falling back to a 16x16 default). Returns an
 * empty result (empty `innerMarkup`) for unparsable input or a root element that isn't `<svg>`,
 * so a broken/hostile asset degrades to a blank icon rather than throwing.
 */
export function sanitizeSvgMarkup(markup: string): SanitizedSvg {
  const document = new DOMParser().parseFromString(markup, 'image/svg+xml');
  if (document.querySelector('parsererror') !== null) {
    return { viewBox: DEFAULT_VIEW_BOX, innerMarkup: '' };
  }
  const root = document.documentElement;
  if (root === null || root.tagName.toLowerCase() !== 'svg') {
    return { viewBox: DEFAULT_VIEW_BOX, innerMarkup: '' };
  }
  const viewBox = root.getAttribute('viewBox') ?? DEFAULT_VIEW_BOX;
  sanitizeElementInPlace(root);
  return { viewBox, innerMarkup: root.innerHTML };
}
