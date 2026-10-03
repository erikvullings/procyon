export const SVG_NS = "http://www.w3.org/2000/svg";

export type ElementPlacement = "child" | "sibling";

export type SvgElementTemplate = {
  tagName: string;
  attrs: Record<string, string>;
  textContent?: string;
};

export const ELEMENT_TEMPLATES: Record<string, SvgElementTemplate> = {
  rect: {
    tagName: "rect",
    attrs: { width: "50", height: "50", fill: "#ccc" },
  },
  circle: {
    tagName: "circle",
    attrs: { cx: "25", cy: "25", r: "20", fill: "#ccc" },
  },
  ellipse: {
    tagName: "ellipse",
    attrs: { cx: "40", cy: "25", rx: "30", ry: "15", fill: "#ccc" },
  },
  line: {
    tagName: "line",
    attrs: {
      x1: "10",
      y1: "10",
      x2: "90",
      y2: "90",
      stroke: "#333",
      "stroke-width": "2",
    },
  },
  path: {
    tagName: "path",
    attrs: { d: "M 10 10 L 90 10 L 50 80 Z", fill: "#ccc", stroke: "#333" },
  },
  text: {
    tagName: "text",
    attrs: { x: "10", y: "24", "font-size": "16", fill: "#333" },
    textContent: "Text Content",
  },
  g: {
    tagName: "g",
    attrs: {},
  },
};

const CHILD_ELEMENT_CONTAINER_TAGS = new Set([
  "a",
  "clippath",
  "defs",
  "g",
  "marker",
  "mask",
  "pattern",
  "svg",
  "switch",
  "symbol",
]);

export function getSvgElementTemplate(
  tagName: string,
): SvgElementTemplate | null {
  const key = tagName.toLowerCase();
  return ELEMENT_TEMPLATES[key] ?? null;
}

export function canContainSvgElements(element: Element): boolean {
  return CHILD_ELEMENT_CONTAINER_TAGS.has(element.tagName.toLowerCase());
}

export function createSvgElementFromTemplate(
  doc: Document,
  tagName: string,
): Element | null {
  const template = getSvgElementTemplate(tagName);
  if (!template) return null;

  try {
    const element = doc.createElementNS(SVG_NS, template.tagName);
    Object.entries(template.attrs).forEach(([name, value]) => {
      element.setAttribute(name, value);
    });

    if (template.textContent) {
      element.appendChild(doc.createTextNode(template.textContent));
    }

    return element;
  } catch {
    return null;
  }
}

export function insertSvgElementFromTemplate(
  doc: Document,
  target: Element | null,
  tagName: string,
  placement: ElementPlacement,
): Element | null {
  if (!target) return null;

  const element = createSvgElementFromTemplate(doc, tagName);
  if (!element) return null;

  if (placement === "child") {
    if (!canContainSvgElements(target)) return null;
    target.appendChild(element);
    return element;
  }

  const parent = target.parentElement;
  if (!parent) return null;

  parent.insertBefore(element, target.nextSibling);
  return element;
}
