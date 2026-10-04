export const ROUNDABLE_ATTRS = new Set([
  "x",
  "y",
  "cx",
  "cy",
  "width",
  "height",
  "r",
  "rx",
  "ry",
  "opacity",
  "fill-opacity",
  "stroke-opacity",
  "stop-opacity",
  "stroke-width",
  "font-size",
]);

export const OPACITY_ATTRS = new Set([
  "opacity",
  "fill-opacity",
  "stroke-opacity",
  "stop-opacity",
]);

// These attributes are never rounded when precision = 0 (values like 0.35 must be preserved)
export const PRECISION_EXCLUDED_ATTRS = new Set(["opacity", "fill-opacity"]);

export const NUMERIC_ATTRS = new Set([
  "x",
  "y",
  "x1",
  "y1",
  "x2",
  "y2",
  "cx",
  "cy",
  "r",
  "rx",
  "ry",
  "width",
  "height",
  "dx",
  "dy",
  "font-size",
  "stroke-width",
  "opacity",
  "fill-opacity",
  "stroke-opacity",
  "stop-opacity",
  "stroke-dashoffset",
  "stroke-miterlimit",
  "letter-spacing",
  "word-spacing",
  "pathlength",
]);

export const NUMERIC_LIST_ATTRS = new Set([
  "viewbox",
  "points",
  "stroke-dasharray",
]);

export const KNOWN_SVG_ATTRS = new Set([
  "id",
  "class",
  "style",
  "transform",
  "opacity",
  "display",
  "visibility",
  "fill",
  "fill-opacity",
  "fill-rule",
  "stroke",
  "stroke-width",
  "stroke-linecap",
  "stroke-linejoin",
  "stroke-dasharray",
  "stroke-dashoffset",
  "stroke-miterlimit",
  "stroke-opacity",
  "clip-path",
  "mask",
  "filter",
  "vector-effect",
  "shape-rendering",
  "text-rendering",
  "paint-order",
  "pointer-events",
  "overflow",
  "enable-background",
  "x",
  "y",
  "x1",
  "y1",
  "x2",
  "y2",
  "cx",
  "cy",
  "r",
  "rx",
  "ry",
  "width",
  "height",
  "d",
  "points",
  "pathlength",
  "dx",
  "dy",
  "font-family",
  "font-size",
  "font-weight",
  "font-style",
  "text-anchor",
  "dominant-baseline",
  "letter-spacing",
  "word-spacing",
  "viewbox",
  "preserveaspectratio",
  "href",
  "xlink:href",
  "offset",
  "stop-color",
  "stop-opacity",
  "gradientunits",
  "gradienttransform",
  "fx",
  "fy",
  "markerwidth",
  "markerheight",
  "refx",
  "refy",
  "orient",
  "markerunits",
  "marker-start",
  "marker-mid",
  "marker-end",
  "patternunits",
  "patterncontentunits",
  "patterntransform",
  "maskunits",
  "maskcontentunits",
  "clippathunits",
  "version",
  "baseprofile",
  "xmlns",
  "xmlns:xlink",
  "xml:space",
]);

export function hasRoundableAttrs(node: Element): boolean {
  if (!node || node.nodeType !== 1) return false;

  for (const attr of Array.from(node.attributes || [])) {
    if (ROUNDABLE_ATTRS.has(attr.name)) return true;
  }

  return Array.from(node.children || []).some((child) =>
    hasRoundableAttrs(child),
  );
}

export function extractTranslate(transform: string | null) {
  if (!transform) return { dx: 0, dy: 0, rest: "" };

  let dx = 0;
  let dy = 0;

  const rest = transform
    .replace(/translate\(\s*([^)]+)\)/g, (_: string, args: string) => {
      const parts = args.split(/[\s,]+/).map(Number);
      dx += parts[0] || 0;
      dy += parts[1] || 0;
      return "";
    })
    .trim();

  return { dx, dy, rest };
}

export function roundAttrsRecursive(node: Element) {
  if (!node || node.nodeType !== 1) return;

  let dx = 0;
  let dy = 0;

  const transform = node.getAttribute("transform");
  if (transform) {
    const extracted = extractTranslate(transform);
    dx = extracted.dx;
    dy = extracted.dy;

    if (extracted.rest) {
      node.setAttribute("transform", extracted.rest);
    } else {
      node.removeAttribute("transform");
    }
  }

  for (const attr of Array.from(node.attributes)) {
    if (!ROUNDABLE_ATTRS.has(attr.name)) continue;

    let num = parseFloat(attr.value);
    if (!Number.isFinite(num)) continue;

    // Apply translation first
    if (attr.name === "x" || attr.name === "cx") {
      num += dx;
    } else if (attr.name === "y" || attr.name === "cy") {
      num += dy;
    }

    const newValue = roundNumericValueFixed(String(num), 0, attr.name);
    node.setAttribute(attr.name, newValue);
  }

  Array.from(node.children).forEach((child) => roundAttrsRecursive(child));
}

export function roundNumericValue(value: string, precision: number) {
  const num = parseFloat(value);
  if (!Number.isFinite(num)) return value;

  const absNum = Math.abs(num);
  let dynamicPrecision = precision;

  if (absNum >= 100) {
    dynamicPrecision = Math.max(0, precision - 2);
  } else if (absNum >= 10) {
    dynamicPrecision = Math.max(0, precision - 1);
  } else if (absNum >= 1) {
    dynamicPrecision = precision;
  } else {
    dynamicPrecision = Math.min(precision + 1, 5);
  }

  if (dynamicPrecision === 0) {
    return formatNumberCompact(Math.round(num));
  }

  const rounded = parseFloat(num.toFixed(dynamicPrecision));
  return formatNumberCompact(rounded);
}

export function roundNumericValueFixed(
  value: string,
  precision: number,
  attrName?: string,
) {
  const num = parseFloat(value);
  if (!Number.isFinite(num)) return value;

  const normalizedAttr = attrName?.toLowerCase();

  // Never round opacity/fill-opacity at precision=0 — fractional values like 0.35 are meaningful
  if (
    precision === 0 &&
    normalizedAttr &&
    PRECISION_EXCLUDED_ATTRS.has(normalizedAttr)
  ) {
    return value;
  }

  const shouldKeepOneDecimal =
    precision === 0 &&
    normalizedAttr &&
    (OPACITY_ATTRS.has(normalizedAttr) ||
      (normalizedAttr === "stroke-width" && Math.abs(num) < 0.5));

  if (shouldKeepOneDecimal) {
    return formatNumberCompact(parseFloat(num.toFixed(1)));
  }

  if (precision === 0) return formatNumberCompact(Math.round(num));
  const rounded = parseFloat(num.toFixed(precision));
  return formatNumberCompact(rounded);
}

export function formatNumberCompact(num: number) {
  if (!Number.isFinite(num)) return String(num);
  if (num === 0) return "0";
  let result = num.toString();
  if (result.startsWith("0.")) {
    result = result.substring(1);
  } else if (result.startsWith("-0.")) {
    result = "-" + result.substring(2);
  }
  return result;
}

export function roundNumericList(
  value: string,
  precision: number,
  attrName?: string,
) {
  return value.replace(/-?\d*\.?\d+(?:e[-+]?\d+)?/gi, (match) =>
    roundNumericValueFixed(match, precision, attrName),
  );
}

export function roundPathData(value: string, precision: number) {
  const tokenRe = /([a-zA-Z])|([-+]?\d*\.?\d+(?:e[-+]?\d+)?)/g;
  const tokens: Array<{ type: "cmd" | "num"; value: string }> = [];
  let match: RegExpExecArray | null;
  while ((match = tokenRe.exec(value)) !== null) {
    if (match[1]) {
      tokens.push({ type: "cmd", value: match[1] });
    } else {
      tokens.push({ type: "num", value: match[2] });
    }
  }

  if (tokens.length === 0) return value;

  const originalValues = tokens.map((token) => token.value);
  tokens.forEach((token) => {
    if (token.type === "num") {
      token.value = roundNumericValueFixed(token.value, precision);
    }
  });

  const commandArity: Record<string, number> = {
    m: 2,
    l: 2,
    h: 1,
    v: 1,
    c: 6,
    s: 4,
    q: 4,
    t: 2,
    a: 7,
  };
  const endpointOffsets: Record<string, number[]> = {
    m: [0, 1],
    l: [0, 1],
    h: [0],
    v: [0],
    c: [4, 5],
    s: [2, 3],
    q: [2, 3],
    t: [0, 1],
    a: [5, 6],
  };

  // Keep enough endpoint precision to prevent a non-zero relative segment
  // from collapsing into its preceding point.
  for (let commandIndex = 0; commandIndex < tokens.length; commandIndex++) {
    const commandToken = tokens[commandIndex];
    if (commandToken.type !== "cmd") continue;

    const command = commandToken.value;
    const normalizedCommand = command.toLowerCase();
    const arity = commandArity[normalizedCommand];
    if (!arity || command !== normalizedCommand) continue;

    let numberEnd = commandIndex + 1;
    while (numberEnd < tokens.length && tokens[numberEnd].type === "num") {
      numberEnd++;
    }

    let segmentIndex = 0;
    for (
      let segmentStart = commandIndex + 1;
      segmentStart + arity <= numberEnd;
      segmentStart += arity, segmentIndex++
    ) {
      if (normalizedCommand === "m" && segmentIndex === 0) continue;

      const endpointIndices = endpointOffsets[normalizedCommand].map(
        (offset) => segmentStart + offset,
      );
      const hadNonZeroEndpoint = endpointIndices.some(
        (index) => Number(originalValues[index]) !== 0,
      );
      const collapsedEndpoint = endpointIndices.every(
        (index) => Number(tokens[index].value) === 0,
      );
      if (!hadNonZeroEndpoint || !collapsedEndpoint) continue;

      let restoredEndpoint = false;
      for (
        let safePrecision = Math.max(0, precision) + 1;
        safePrecision <= 12;
        safePrecision++
      ) {
        const saferValues = endpointIndices.map((index) =>
          roundNumericValueFixed(originalValues[index], safePrecision),
        );
        if (saferValues.every((rounded) => Number(rounded) === 0)) continue;

        endpointIndices.forEach((index, endpointIndex) => {
          tokens[index].value = saferValues[endpointIndex];
        });
        restoredEndpoint = true;
        break;
      }

      if (!restoredEndpoint) {
        endpointIndices.forEach((index) => {
          tokens[index].value = formatNumberCompact(
            Number(originalValues[index]),
          );
        });
      }
    }
  }

  let out = "";
  for (const token of tokens) {
    if (token.type === "cmd") {
      out += token.value;
      continue;
    }

    const rounded = token.value;
    if (out.length === 0) {
      out += rounded;
      continue;
    }

    const prev = out[out.length - 1];
    if (/[0-9.+-]|[eE]/.test(prev)) {
      out += " " + rounded;
    } else {
      out += rounded;
    }
  }

  return out;
}

export function applyTranslateToPoints(value: string, dx: number, dy: number) {
  const parts = value
    .trim()
    .split(/[\s,]+/)
    .map(Number);
  if (parts.length < 2) return value;
  const result = [];
  for (let i = 0; i < parts.length; i += 2) {
    const x = parts[i];
    const y = parts[i + 1];
    if (!Number.isFinite(x) || !Number.isFinite(y)) return value;
    result.push(formatNumberCompact(x + dx), formatNumberCompact(y + dy));
  }
  return result.join(" ");
}

export function translatePathData(pathData: string, dx: number, dy: number) {
  const temp = document.createElementNS("http://www.w3.org/2000/svg", "path");
  temp.setAttribute("d", pathData);
  if (!temp.getPathData) {
    return translatePathDataFallback(pathData, dx, dy);
  }

  let segments: PathDataSegment[];
  try {
    segments = temp.getPathData({ normalize: true });
  } catch (e) {
    try {
      segments = temp.getPathData() || [];
    } catch (err) {
      return translatePathDataFallback(pathData, dx, dy);
    }
  }
  segments.forEach((seg) => {
    switch (seg.type) {
      case "M":
      case "L":
      case "T":
        seg.values[0] += dx;
        seg.values[1] += dy;
        break;
      case "H":
        seg.values[0] += dx;
        break;
      case "V":
        seg.values[0] += dy;
        break;
      case "C":
        seg.values[0] += dx;
        seg.values[1] += dy;
        seg.values[2] += dx;
        seg.values[3] += dy;
        seg.values[4] += dx;
        seg.values[5] += dy;
        break;
      case "S":
      case "Q":
        seg.values[0] += dx;
        seg.values[1] += dy;
        seg.values[2] += dx;
        seg.values[3] += dy;
        break;
      case "A":
        seg.values[5] += dx;
        seg.values[6] += dy;
        break;
      default:
        break;
    }
  });

  return segments
    .map((seg) => {
      const values = seg.values.length ? seg.values.join(" ") : "";
      return `${seg.type}${values ? " " + values : ""}`;
    })
    .join(" ");
}

export function translatePathDataFallback(
  pathData: string,
  dx: number,
  dy: number,
) {
  const tokenRe = /([a-zA-Z])|([-+]?\d*\.?\d+(?:e[-+]?\d+)?)/g;
  const tokens: Array<{ type: "cmd" | "num"; value: string }> = [];
  let match: RegExpExecArray | null;
  while ((match = tokenRe.exec(pathData)) !== null) {
    if (match[1]) {
      tokens.push({ type: "cmd", value: match[1] });
    } else {
      tokens.push({ type: "num", value: match[2] });
    }
  }

  if (tokens.length === 0) return null;

  const paramCounts: Record<string, number> = {
    m: 2,
    l: 2,
    h: 1,
    v: 1,
    c: 6,
    s: 4,
    q: 4,
    t: 2,
    a: 7,
    z: 0,
  };

  let i = 0;
  let cmd: string | null = null;
  let firstCommand = true;
  let currentX = 0;
  let currentY = 0;
  let subStartX = 0;
  let subStartY = 0;
  const out: string[] = [];

  function readNumbers() {
    const nums: number[] = [];
    while (i < tokens.length && tokens[i].type === "num") {
      nums.push(parseFloat(tokens[i].value));
      i += 1;
    }
    return nums;
  }

  while (i < tokens.length) {
    if (tokens[i].type === "cmd") {
      cmd = tokens[i].value;
      i += 1;
    }

    if (!cmd) return null;

    const cmdLower = cmd.toLowerCase();
    const isAbs = cmd === cmd.toUpperCase();
    const paramCount = paramCounts[cmdLower];

    if (paramCount === 0) {
      out.push("Z");
      currentX = subStartX;
      currentY = subStartY;
      firstCommand = false;
      continue;
    }

    const numbers = readNumbers();
    if (numbers.length === 0) {
      return null;
    }

    for (let n = 0; n < numbers.length; n += paramCount) {
      const chunk = numbers.slice(n, n + paramCount);
      if (chunk.length < paramCount) return null;

      switch (cmdLower) {
        case "m": {
          if (isAbs) {
            const nx = chunk[0] + dx;
            const ny = chunk[1] + dy;
            out.push(
              n === 0 ? "M" : "L",
              formatNumberCompact(nx),
              formatNumberCompact(ny),
            );
            currentX = chunk[0];
            currentY = chunk[1];
            if (n === 0) {
              subStartX = chunk[0];
              subStartY = chunk[1];
            }
          } else {
            if (firstCommand) {
              out.push(
                "M",
                formatNumberCompact(chunk[0] + dx),
                formatNumberCompact(chunk[1] + dy),
              );
              currentX = chunk[0];
              currentY = chunk[1];
              subStartX = chunk[0];
              subStartY = chunk[1];
            } else if (n > 0) {
              out.push(
                "l",
                formatNumberCompact(chunk[0]),
                formatNumberCompact(chunk[1]),
              );
              currentX += chunk[0];
              currentY += chunk[1];
            } else {
              out.push(
                "m",
                formatNumberCompact(chunk[0]),
                formatNumberCompact(chunk[1]),
              );
              currentX += chunk[0];
              currentY += chunk[1];
              subStartX = currentX;
              subStartY = currentY;
            }
          }
          break;
        }
        case "l": {
          if (isAbs) {
            out.push(
              "L",
              formatNumberCompact(chunk[0] + dx),
              formatNumberCompact(chunk[1] + dy),
            );
            currentX = chunk[0];
            currentY = chunk[1];
          } else {
            out.push(
              "l",
              formatNumberCompact(chunk[0]),
              formatNumberCompact(chunk[1]),
            );
            currentX += chunk[0];
            currentY += chunk[1];
          }
          break;
        }
        case "h": {
          if (isAbs) {
            out.push("H", formatNumberCompact(chunk[0] + dx));
            currentX = chunk[0];
          } else {
            out.push("h", formatNumberCompact(chunk[0]));
            currentX += chunk[0];
          }
          break;
        }
        case "v": {
          if (isAbs) {
            out.push("V", formatNumberCompact(chunk[0] + dy));
            currentY = chunk[0];
          } else {
            out.push("v", formatNumberCompact(chunk[0]));
            currentY += chunk[0];
          }
          break;
        }
        case "c": {
          if (isAbs) {
            out.push(
              "C",
              formatNumberCompact(chunk[0] + dx),
              formatNumberCompact(chunk[1] + dy),
              formatNumberCompact(chunk[2] + dx),
              formatNumberCompact(chunk[3] + dy),
              formatNumberCompact(chunk[4] + dx),
              formatNumberCompact(chunk[5] + dy),
            );
            currentX = chunk[4];
            currentY = chunk[5];
          } else {
            out.push(
              "c",
              formatNumberCompact(chunk[0]),
              formatNumberCompact(chunk[1]),
              formatNumberCompact(chunk[2]),
              formatNumberCompact(chunk[3]),
              formatNumberCompact(chunk[4]),
              formatNumberCompact(chunk[5]),
            );
            currentX += chunk[4];
            currentY += chunk[5];
          }
          break;
        }
        case "s": {
          if (isAbs) {
            out.push(
              "S",
              formatNumberCompact(chunk[0] + dx),
              formatNumberCompact(chunk[1] + dy),
              formatNumberCompact(chunk[2] + dx),
              formatNumberCompact(chunk[3] + dy),
            );
            currentX = chunk[2];
            currentY = chunk[3];
          } else {
            out.push(
              "s",
              formatNumberCompact(chunk[0]),
              formatNumberCompact(chunk[1]),
              formatNumberCompact(chunk[2]),
              formatNumberCompact(chunk[3]),
            );
            currentX += chunk[2];
            currentY += chunk[3];
          }
          break;
        }
        case "q": {
          if (isAbs) {
            out.push(
              "Q",
              formatNumberCompact(chunk[0] + dx),
              formatNumberCompact(chunk[1] + dy),
              formatNumberCompact(chunk[2] + dx),
              formatNumberCompact(chunk[3] + dy),
            );
            currentX = chunk[2];
            currentY = chunk[3];
          } else {
            out.push(
              "q",
              formatNumberCompact(chunk[0]),
              formatNumberCompact(chunk[1]),
              formatNumberCompact(chunk[2]),
              formatNumberCompact(chunk[3]),
            );
            currentX += chunk[2];
            currentY += chunk[3];
          }
          break;
        }
        case "t": {
          if (isAbs) {
            out.push(
              "T",
              formatNumberCompact(chunk[0] + dx),
              formatNumberCompact(chunk[1] + dy),
            );
            currentX = chunk[0];
            currentY = chunk[1];
          } else {
            out.push(
              "t",
              formatNumberCompact(chunk[0]),
              formatNumberCompact(chunk[1]),
            );
            currentX += chunk[0];
            currentY += chunk[1];
          }
          break;
        }
        case "a": {
          if (isAbs) {
            out.push(
              "A",
              formatNumberCompact(chunk[0]),
              formatNumberCompact(chunk[1]),
              formatNumberCompact(chunk[2]),
              formatNumberCompact(chunk[3]),
              formatNumberCompact(chunk[4]),
              formatNumberCompact(chunk[5] + dx),
              formatNumberCompact(chunk[6] + dy),
            );
            currentX = chunk[5];
            currentY = chunk[6];
          } else {
            out.push(
              "a",
              formatNumberCompact(chunk[0]),
              formatNumberCompact(chunk[1]),
              formatNumberCompact(chunk[2]),
              formatNumberCompact(chunk[3]),
              formatNumberCompact(chunk[4]),
              formatNumberCompact(chunk[5]),
              formatNumberCompact(chunk[6]),
            );
            currentX += chunk[5];
            currentY += chunk[6];
          }
          break;
        }
        default:
          return null;
      }
      firstCommand = false;
    }
  }

  return out.join(" ");
}

export function collapseTransforms(svg: string) {
  const parser = new DOMParser();
  const doc = parser.parseFromString(svg, "image/svg+xml");
  const svgEl = doc.querySelector("svg");
  if (!svgEl) return svg;

  const XLINK_NS = "http://www.w3.org/1999/xlink";

  const getHref = (el: Element): string | null => {
    return (
      el.getAttribute("href") ||
      el.getAttribute("xlink:href") ||
      el.getAttributeNS(XLINK_NS, "href")
    );
  };

  function getPaintRefId(value: string | null): string | null {
    if (!value) return null;
    const match = value.match(/url\(#([^)]+)\)/);
    return match ? match[1] : null;
  }

  const paintUserSpace = new Map<string, boolean>();
  const paintElements = Array.from(
    doc.querySelectorAll("linearGradient, radialGradient, pattern"),
  );

  const resolveUnits = (el: Element, trail: Set<Element>): string | null => {
    const tag = el.tagName.toLowerCase();
    if (tag === "lineargradient" || tag === "radialgradient") {
      const units = el.getAttribute("gradientUnits");
      if (units) return units;
    } else if (tag === "pattern") {
      const units = el.getAttribute("patternUnits");
      if (units) return units;
    }

    const href = getHref(el);
    if (!href || !href.startsWith("#")) return null;
    const targetId = href.slice(1);
    const target = doc.querySelector(`[id="${targetId}"]`);
    if (!target || trail.has(target)) return null;
    trail.add(target);
    return resolveUnits(target, trail);
  };

  paintElements.forEach((el) => {
    const id = el.getAttribute("id");
    if (!id) return;
    const units = resolveUnits(el, new Set([el]));
    paintUserSpace.set(id, units === "userSpaceOnUse");
  });

  function hasUserSpacePaint(el: Element): boolean {
    const fillId = getPaintRefId(el.getAttribute("fill"));
    const strokeId = getPaintRefId(el.getAttribute("stroke"));
    const fillUserSpace = fillId ? paintUserSpace.get(fillId) : false;
    const strokeUserSpace = strokeId ? paintUserSpace.get(strokeId) : false;
    if (fillUserSpace || strokeUserSpace) {
      return true;
    }
    const style = el.getAttribute("style");
    if (style) {
      const fillMatch = style.match(/fill\s*:\s*url\(#([^)]+)\)/i);
      const strokeMatch = style.match(/stroke\s*:\s*url\(#([^)]+)\)/i);
      const styleFillUserSpace = fillMatch
        ? paintUserSpace.get(fillMatch[1])
        : false;
      const styleStrokeUserSpace = strokeMatch
        ? paintUserSpace.get(strokeMatch[1])
        : false;
      if (styleFillUserSpace || styleStrokeUserSpace) {
        return true;
      }
    }
    return false;
  }

  function subtreeUsesUserSpacePaint(el: Element): boolean {
    if (hasUserSpacePaint(el)) return true;
    for (const child of Array.from(el.children)) {
      if (subtreeUsesUserSpacePaint(child)) return true;
    }
    return false;
  }

  function canTranslate(el: Element) {
    const tag = el.tagName.toLowerCase();
    if (tag === "g" || tag === "svg") {
      return Array.from(el.children).every((child) => canTranslate(child));
    }
    if (tag === "path") {
      const d = el.getAttribute("d");
      if (!d) return true;
      if (typeof document.createElementNS === "function") {
        const temp = document.createElementNS(
          "http://www.w3.org/2000/svg",
          "path",
        );
        if (typeof temp.getPathData === "function") return true;
      }
      return translatePathDataFallback(d, 0, 0) !== null;
    }
    return true;
  }

  function applyTranslate(el: Element, dx: number, dy: number) {
    if (dx === 0 && dy === 0) return true;

    const tag = el.tagName.toLowerCase();
    if (tag === "g" || tag === "svg") {
      if (subtreeUsesUserSpacePaint(el)) return false;
      if (!canTranslate(el)) return false;
      Array.from(el.children).forEach((child) => {
        applyTranslate(child, dx, dy);
      });
      return true;
    }

    if (hasUserSpacePaint(el)) return false;

    if (tag === "path") {
      const d = el.getAttribute("d");
      if (d) {
        const translated = translatePathData(d, dx, dy);
        if (translated) {
          el.setAttribute("d", translated);
        } else {
          // Can't translate path reliably without path data API
          return false;
        }
      }
    }

    if (el.hasAttribute("points")) {
      const points = el.getAttribute("points");
      if (points) {
        const translated = applyTranslateToPoints(points, dx, dy);
        el.setAttribute("points", translated);
      }
    }

    const xAttrs = ["x", "x1", "x2", "cx"];
    const yAttrs = ["y", "y1", "y2", "cy"];

    xAttrs.forEach((attr) => {
      if (el.hasAttribute(attr)) {
        const val = parseFloat(el.getAttribute(attr) || "");
        if (Number.isFinite(val)) {
          el.setAttribute(attr, formatNumberCompact(val + dx));
        }
      }
    });

    yAttrs.forEach((attr) => {
      if (el.hasAttribute(attr)) {
        const val = parseFloat(el.getAttribute(attr) || "");
        if (Number.isFinite(val)) {
          el.setAttribute(attr, formatNumberCompact(val + dy));
        }
      }
    });

    for (const child of Array.from(el.children)) {
      if (!applyTranslate(child, dx, dy)) return false;
    }
    return true;
  }

  Array.from(doc.querySelectorAll("[transform]")).forEach((el) => {
    const transform = el.getAttribute("transform");
    if (!transform) return;
    const extracted = extractTranslate(transform);
    if (extracted.rest) {
      // Only remove pure translate transforms
      el.setAttribute("transform", transform);
      return;
    }

    let applied = true;
    if (extracted.dx || extracted.dy) {
      applied = applyTranslate(el, extracted.dx, extracted.dy);
    }

    if (applied) {
      el.removeAttribute("transform");
    } else {
      el.setAttribute("transform", transform);
    }
  });

  return new XMLSerializer().serializeToString(doc);
}
