import DOMPurify from "dompurify";
import { parse, walk } from "css-tree";

function safeCss(css: string, context: "stylesheet" | "declarationList"): boolean {
  let safe = true;
  const ast = parse(css, {
    context,
    onParseError: () => { safe = false; },
  });
  walk(ast, (node) => {
    if (
      node.type === "Raw" ||
      node.type === "Atrule" ||
      (node.type === "Url" && !/^#[\w.:-]+$/.test(node.value)) ||
      (node.type === "Function" && /^(?:-webkit-)?(?:image-set|image|cross-fade|paint)$/i.test(node.name))
    ) {
      safe = false;
    }
  });
  return safe;
}

DOMPurify.addHook("uponSanitizeElement", (node, event) => {
  if (event.tagName === "style" && !safeCss(node.textContent || "", "stylesheet")) {
    node.parentNode?.removeChild(node);
  }
});

DOMPurify.addHook("uponSanitizeAttribute", (node, event) => {
  const name = event.attrName.toLowerCase();
  const value = event.attrValue;
  if (name === "href" || name === "xlink:href" || name === "src") {
    const local = /^#[\w.:-]+$/.test(value);
    const embeddedRaster =
      node.localName.toLowerCase() === "image" &&
      /^data:image\/(?:png|jpeg|gif|webp);base64,[a-z0-9+/=\s]+$/i.test(value);
    if (!local && !embeddedRaster) event.keepAttr = false;
  } else if (name === "style") {
    if (!safeCss(value, "declarationList")) event.keepAttr = false;
  } else if (/url\s*\(|\\|\/\*|image-set\s*\(/i.test(value)) {
    if (!safeCss(`fill:${value}`, "declarationList")) event.keepAttr = false;
  }
});

export function sanitizePreviewSvg(svg: string): string {
  return DOMPurify.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true },
    ADD_TAGS: ["style", "use"],
    FORBID_TAGS: [
      "foreignObject",
      "script",
      "animate",
      "animateMotion",
      "animateTransform",
      "set",
    ],
    FORBID_ATTR: ["xml:base"],
  });
}
