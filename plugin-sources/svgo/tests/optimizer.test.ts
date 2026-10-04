// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { readFileSync } from "fs";
import { SVGOptimizer } from "../src/optimizer";

describe("xlink href handling", () => {
  it("adds href alongside xlink:href", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">' +
      "<defs>" +
      '<linearGradient id="prefix__a"><stop offset="0" stop-color="#0297ff"/></linearGradient>' +
      '<linearGradient id="prefix__c" xlink:href="#prefix__a"/>' +
      "</defs>" +
      "</svg>";
    const optimizer = new SVGOptimizer();
    const converted = optimizer.convertXlinkHrefs(input);
    const cleaned = optimizer.removeUnusedXlinkNamespace(converted);

    expect(cleaned).toContain('href="#prefix__a"');
    expect(cleaned).toContain("xlink:href");
  });

  it("adds xmlns:xlink when missing", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      "<defs>" +
      '<linearGradient id="prefix__a"><stop offset="0" stop-color="#0297ff"/></linearGradient>' +
      '<linearGradient id="prefix__c" xlink:href="#prefix__a"/>' +
      "</defs>" +
      "</svg>";
    const optimizer = new SVGOptimizer();
    const normalized = optimizer.normalizeNamespaces(input);

    expect(normalized).toContain('xmlns:xlink="http://www.w3.org/1999/xlink"');
    expect(normalized).toContain('href="#prefix__a"');
    expect(normalized).toContain("xlink:href");
  });
});

describe("fixInvalidHexColors", () => {
  it("collapses out-of-range hex digits to f on import", () => {
    const optimizer = new SVGOptimizer();
    expect(optimizer.fixInvalidHexColors('<path fill="#pf5ccc"/>')).toBe(
      '<path fill="#ff5ccc"/>',
    );
  });

  it("preserves casing when clamping", () => {
    const optimizer = new SVGOptimizer();
    expect(optimizer.fixInvalidHexColors('<stop stop-color="#G0Z"/>')).toBe(
      '<stop stop-color="#F0F"/>',
    );
  });

  it("fixes colours inside style attributes and css", () => {
    const optimizer = new SVGOptimizer();
    expect(
      optimizer.fixInvalidHexColors('<rect style="fill:#pf5ccc;stroke:#0g0"/>'),
    ).toBe('<rect style="fill:#ff5ccc;stroke:#0f0"/>');
  });

  it("leaves id references and valid colours untouched", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<path fill="#0297ff" marker-end="url(#p)" clip-path="url(#gradient)"/>';
    expect(optimizer.fixInvalidHexColors(input)).toBe(input);
  });
});

describe("removeDefaultValues", () => {
  it("removes overflow, enable-background, and xml:space defaults", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 192" overflow="visible" enable-background="new 0 0 300 192" xml:space="preserve">' +
      '<path d="M0 0h10v10z"/>' +
      "</svg>";
    const optimizer = new SVGOptimizer();
    const output = optimizer.removeDefaultValues(input);
    expect(output).not.toContain('overflow="visible"');
    expect(output).not.toContain("enable-background");
    expect(output).not.toContain('xml:space="preserve"');
  });

  it("removes namespaced Illustrator metadata attrs and unused xmlns", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg" xmlns:ns2="http://ns.adobe.com/AdobeIllustrator/10.0/" ns2:viewOrigin="0 0" ns2:rulerOrigin="0 0" ns2:pageBounds="0 360 360 0">' +
      '<path d="M0 0h10v10z"/>' +
      "</svg>";
    const optimizer = new SVGOptimizer();
    const output = optimizer.removeDefaultValues(input);
    expect(output).not.toContain("ns2:viewOrigin");
    expect(output).not.toContain("ns2:rulerOrigin");
    expect(output).not.toContain("ns2:pageBounds");
    expect(output).not.toContain("xmlns:ns2");
  });

  it("removes marker refX/refY when explicitly zero", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><defs><marker id="m" refX="0" refY="0"/></defs></svg>';
    const optimizer = new SVGOptimizer();
    const output = optimizer.removeDefaultValues(input);
    expect(output).not.toContain('refX="0"');
    expect(output).not.toContain('refY="0"');
  });

  it("keeps marker refX/refY when non-zero", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><defs><marker id="m" refX="1" refY="0.5"/></defs></svg>';
    const optimizer = new SVGOptimizer();
    const output = optimizer.removeDefaultValues(input);
    expect(output).toContain('refX="1"');
    expect(output).toContain('refY="0.5"');
  });
});

describe("text and namespace cleanup", () => {
  it("removes empty xmlns declarations", () => {
    const optimizer = new SVGOptimizer();
    expect(optimizer.normalizeNamespaces('<svg xmlns="http://www.w3.org/2000/svg"><g xmlns=""/></svg>')).not.toContain('xmlns=""');
  });

  it("trims text and tspan content", () => {
    const optimizer = new SVGOptimizer();
    expect(optimizer.trimTextContent('<svg xmlns="http://www.w3.org/2000/svg"><text>  Hello  <tspan> world </tspan>  </text></svg>')).toContain('<text>Hello<tspan>world</tspan></text>');
  });

  it("removes xml:space=preserve from text with no whitespace", () => {
    const optimizer = new SVGOptimizer();
    const output = optimizer.removeDefaultValues(
      '<svg xmlns="http://www.w3.org/2000/svg"><text xml:space="preserve">Hello</text></svg>',
    );
    expect(output).not.toContain("xml:space");
  });

  it("keeps xml:space=preserve on text with meaningful whitespace", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><text xml:space="preserve">Hello  World</text></svg>';
    const output = optimizer.removeDefaultValues(input);
    expect(output).toContain('xml:space="preserve"');
  });

  it("does not trim text with xml:space=preserve", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><text xml:space="preserve">  Hello  World  </text></svg>';
    expect(optimizer.trimTextContent(input)).toContain(">  Hello  World  <");
  });

  it("leaves multi-tspan text elements untouched", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><text xml:space="preserve">' +
      '<tspan>  Hi  </tspan><tspan>  There  </tspan></text></svg>';
    expect(optimizer.trimTextContent(input)).toContain(input.match(/<text[^]*<\/text>/)![0]);
    const withoutPreserve = optimizer.removeDefaultValues(input);
    expect(withoutPreserve).toContain('xml:space="preserve"');
  });
});

describe("groupSimilarElementsByType for text", () => {
  it("groups consecutive text elements sharing font-size and fill", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      '<text font-size="12" fill="#333">A</text>' +
      '<text font-size="12" fill="#333">B</text>' +
      '<text font-size="12" fill="#333">C</text>' +
      "</svg>";
    const output = optimizer.groupSimilarElementsByType(input);
    expect(output).toContain("<g");
    expect(output).toContain('font-size="12"');
    expect(output).toContain('fill="#333"');
    expect(output).toContain("<text>A</text><text>B</text><text>C</text>");
    expect((output.match(/font-size="12"/g) || []).length).toBe(1);
  });

  it("reorders non-adjacent text elements sharing attributes into one group", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      '<text font-size="12" fill="#333">A</text>' +
      '<rect width="1" height="1"/>' +
      '<text font-size="10" fill="#000">X</text>' +
      '<text font-size="12" fill="#333">B</text>' +
      '<text font-size="12" fill="#333">C</text>' +
      "</svg>";
    const output = optimizer.groupSimilarElementsByType(input);
    expect(output).toContain("<text>A</text><text>B</text><text>C</text>");
    expect((output.match(/<g/g) || []).length).toBe(1);
    expect(output).toContain('<text font-size="10" fill="#000">X</text>');
  });

  it("does not group text elements with differing attributes", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      '<text font-size="12" fill="#333">A</text>' +
      '<text font-size="14" fill="#000">B</text>' +
      "</svg>";
    const output = optimizer.groupSimilarElementsByType(input);
    expect(output).not.toContain("<g");
  });

  it("groups text elements on shared attributes even when other attributes differ", () => {
    const optimizer = new SVGOptimizer();
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      '<text font-size="12" fill="#333" stroke="#000">A</text>' +
      '<text font-size="12" fill="#333" stroke-width="2">B</text>' +
      '<text font-size="12" fill="#333">C</text>' +
      "</svg>";
    const output = optimizer.groupSimilarElementsByType(input);
    expect(output).toContain("<g");
    expect(output).toContain('font-size="12"');
    expect(output).toContain('fill="#333"');
    expect((output.match(/<g/g) || []).length).toBe(1);
    expect(output).toContain('<text stroke="#000">A</text>');
    expect(output).toContain('<text stroke-width="2">B</text>');
    expect(output).toContain("<text>C</text>");
  });
});


describe("moveTextElementsToEnd", () => {
  it("moves all text elements into a trailing group in original order", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><g><text>A</text></g><rect/><text>B</text><g><text>C</text></g></svg>';
    const optimizer = new SVGOptimizer();
    const output = optimizer.moveTextElementsToEnd(input);

    const doc = new DOMParser().parseFromString(output, "image/svg+xml");
    const root = doc.querySelector("svg");
    expect(root).toBeTruthy();

    const children = Array.from(root!.children);
    expect(children[children.length - 1].tagName.toLowerCase()).toBe("g");

    const textValues = Array.from(children[children.length - 1].children).map(
      (el) => (el.textContent || "").trim(),
    );
    expect(textValues).toEqual(["A", "B", "C"]);
  });

  it("preserves inherited font-size when moving text", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><g font-size="18"><text>A</text></g><text>B</text></svg>';
    const optimizer = new SVGOptimizer();
    const output = optimizer.moveTextElementsToEnd(input);

    const doc = new DOMParser().parseFromString(output, "image/svg+xml");
    const root = doc.querySelector("svg");
    expect(root).toBeTruthy();

    const group = root!.lastElementChild as Element;
    expect(group.tagName.toLowerCase()).toBe("g");
    const movedTexts = Array.from(group.querySelectorAll("text"));
    expect(movedTexts.length).toBe(2);
    expect(movedTexts[0].getAttribute("font-size")).toBe("18");
    expect(movedTexts[1].hasAttribute("font-size")).toBe(false);
  });

  it("removes empty source groups and avoids duplicated shared font-size", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><g font-size="3"><text x="1" y="2" font-size="3">A</text></g><text x="3" y="4" font-size="3">B</text></svg>';
    const optimizer = new SVGOptimizer();
    const output = optimizer.moveTextElementsToEnd(input);

    const doc = new DOMParser().parseFromString(output, "image/svg+xml");
    const root = doc.querySelector("svg");
    expect(root).toBeTruthy();

    const groups = Array.from(root!.children).filter(
      (el) => el.tagName.toLowerCase() === "g",
    );
    expect(groups.length).toBe(1);

    const group = groups[0];
    expect(group.getAttribute("font-size")).toBe("3");
    const movedTexts = Array.from(group.querySelectorAll("text"));
    expect(movedTexts.length).toBe(2);
    expect(movedTexts.every((textEl) => !textEl.hasAttribute("font-size"))).toBe(
      true,
    );
  });

  it("does nothing when fewer than two text elements exist", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><rect/><g><text>A</text></g></svg>';
    const optimizer = new SVGOptimizer();
    const output = optimizer.moveTextElementsToEnd(input);
    expect(output).toBe(input);
  });
});

describe("autocropSvg", () => {
  it("crops a vertical path whose geometric bounding box has zero width", () => {
    const svgPrototype = SVGElement.prototype as typeof SVGElement.prototype & {
      getBBox: () => DOMRect;
    };
    const originalGetBBox = svgPrototype.getBBox;
    svgPrototype.getBBox = () =>
      ({ x: 33.4, y: 13.8, width: 0, height: 2.8 }) as DOMRect;

    try {
      const optimizer = new SVGOptimizer();
      const input =
        '<svg viewBox="0 0 156.8 16.9"><path stroke="red" stroke-width=".6" d="m33.4 16.6 0 -2.8"/></svg>';
      const output = optimizer.autocropSvg(input, 3);
      const doc = new DOMParser().parseFromString(output, "image/svg+xml");

      expect(doc.documentElement.getAttribute("viewBox")).toBe("30 10 7 10");
    } finally {
      svgPrototype.getBBox = originalGetBBox;
    }
  });

  it("includes referenced marker geometry in the cropped viewBox", () => {
    const svgPrototype = SVGElement.prototype as typeof SVGElement.prototype & {
      getBBox: () => DOMRect;
    };
    const originalGetBBox = svgPrototype.getBBox;
    svgPrototype.getBBox = function () {
      if (this.hasAttribute("data-autocrop-marker-content")) {
        return { x: -2.5, y: -2.5, width: 8.8, height: 5 } as DOMRect;
      }
      return { x: 33.4, y: 13.8, width: 0, height: 2.8 } as DOMRect;
    };

    try {
      const optimizer = new SVGOptimizer();
      const input =
        '<svg viewBox="0 0 156.8 16.9">' +
        '<defs><marker id="a" markerWidth="1" markerHeight="1" viewBox="0 0 1 1">' +
        '<path d="m0 0 -2.5 2.5L6.3 0 -2.5 -2.5z"/></marker></defs>' +
        '<path stroke="red" stroke-width=".6" marker-end="url(#a)" d="m33.4 16.6 0 -2.8"/>' +
        "</svg>";
      const output = optimizer.autocropSvg(input, 3);
      const doc = new DOMParser().parseFromString(output, "image/svg+xml");

      expect(doc.documentElement.getAttribute("viewBox")).toBe("26 6 15 18");
    } finally {
      svgPrototype.getBBox = originalGetBBox;
    }
  });
});

describe("optimizeSvg", () => {
  it("treats zero precision as an enabled optimization", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><path d="M10.4 20.6l2.2 3.3"/></svg>';
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.precision = 0;
    optimizer.options.pathPrecision = 0;
    optimizer.options.trimText = false;
    optimizer.options.convertSodipodiArcs = false;
    optimizer.options.removeDefaultValues = false;
    optimizer.options.removeFontFamily = false;
    optimizer.options.removeFontSize = false;
    optimizer.options.removeTspan = false;
    optimizer.options.removeStyling = false;
    optimizer.options.groupSimilarElements = false;
    optimizer.options.groupTextElementsAtEnd = false;
    optimizer.options.autoAutocrop = false;
    optimizer.options.useCustomDimensions = false;

    expect(optimizer.isOptimizationEnabled()).toBe(true);
    await optimizer.optimizeSvg();
    expect(optimizer.optimizedSvg).toContain('d="m10 21 2 3"');
  });

  it("does not change an SVG without sodipodi arcs when arc conversion is toggled", async () => {
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = readFileSync(
      "tests/examples/marker-arrow.svg",
      "utf8",
    );
    optimizer.options.precision = 0;
    optimizer.options.pathPrecision = 0;
    optimizer.options.trimText = false;
    optimizer.options.convertSodipodiArcs = false;
    optimizer.options.removeDefaultValues = false;
    optimizer.options.removeFontFamily = false;
    optimizer.options.removeFontSize = false;
    optimizer.options.removeTspan = false;
    optimizer.options.removeStyling = false;
    optimizer.options.groupSimilarElements = false;
    optimizer.options.groupTextElementsAtEnd = false;
    optimizer.options.autoAutocrop = false;
    optimizer.options.useCustomDimensions = false;

    await optimizer.optimizeSvg();
    const withoutArcConversion = optimizer.optimizedSvg;
    optimizer.options.convertSodipodiArcs = true;
    await optimizer.optimizeSvg();

    expect(optimizer.optimizedSvg).toBe(withoutArcConversion);
  });

  it("removes ns2 namespace after optimization", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg" xmlns:ns2="http://ns.adobe.com/AdobeIllustrator/10.0/" ns2:viewOrigin="0 0" ns2:rulerOrigin="0 0" ns2:pageBounds="0 360 360 0">' +
      '<path d="M0 0h10v10z"/>' +
      "</svg>";
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.removeDefaultValues = true;
    await optimizer.optimizeSvg();
    const output = optimizer.optimizedSvg;
    expect(output).not.toContain("xmlns:ns2");
    expect(output).not.toContain("ns2:viewOrigin");
    expect(output).not.toContain("ns2:rulerOrigin");
    expect(output).not.toContain("ns2:pageBounds");
    expect(output).not.toContain("parsererror");
    expect(output).toContain('<path d="M0 0h10v10z"/>');
  });
  it("removes ns2 namespace without namespace declaration after optimization", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg" ns2:viewOrigin="0 0" ns2:rulerOrigin="0 0" ns2:pageBounds="0 360 360 0">' +
      '<path d="M0 0h10v10z"/>' +
      "</svg>";
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.removeDefaultValues = true;
    await optimizer.optimizeSvg();
    const output = optimizer.optimizedSvg;
    expect(output).not.toContain("xmlns:ns2");
    expect(output).not.toContain("ns2");
    expect(output).not.toContain("ns2:viewOrigin");
    expect(output).not.toContain("ns2:rulerOrigin");
    expect(output).not.toContain("ns2:pageBounds");
    expect(output).not.toContain("parsererror");
    expect(output).toContain('<path d="M0 0h10v10z"/>');
  });

  it("removes overflow and enable-background after optimization", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120.7 120.7" overflow="visible" enable-background="new 0 0 300 192">' +
      '<path d="M0 0h10v10z"/>' +
      "</svg>";
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.removeDefaultValues = true;
    await optimizer.optimizeSvg();
    const output = optimizer.optimizedSvg;
    expect(output).not.toContain('overflow="visible"');
    expect(output).not.toContain("enable-background");
    expect(output).not.toContain("parsererror");
  });

  it("preserves non-zero opacity and stroke-width at 0 precision", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      '<circle cx="10.4" cy="20.5" r="5.2" stroke="red" opacity="0.3" stroke-width="0.4" stop-opacity="0.25"/>' +
      "</svg>";
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.precision = 0;
    optimizer.options.pathPrecision = 0;
    await optimizer.optimizeSvg();
    const output = optimizer.optimizedSvg;

    expect(output).toContain('cx="10"');
    expect(output).toContain('cy="21"');
    expect(output).toContain('opacity=".3"');
    expect(output).toContain('stroke-width=".4"');
    expect(output).toContain('stop-opacity=".3"');
    expect(output).toContain('r="5"');
  });

  it("preserves a non-zero path segment at 0 path precision", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><path d="m 45.175923,125.37949 -0.0256,-0.04605"/></svg>';
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.precision = 0;
    optimizer.options.pathPrecision = 0;
    await optimizer.optimizeSvg();

    const doc = new DOMParser().parseFromString(
      optimizer.optimizedSvg,
      "image/svg+xml",
    );
    const pathData = doc.querySelector("path")?.getAttribute("d");
    expect(pathData).toBeTruthy();
    expect(pathData).toBe("m45 125 -.03 -.05");
  });

  it("preserves marker arrowheads for arrows example", async () => {
    const input = readFileSync("tests/examples/arrows.svg", "utf8");
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    await optimizer.optimizeSvg();
    const output = optimizer.optimizedSvg;

    expect(output).toContain("<marker");
    expect(output).toContain('overflow="visible"');
    expect(output).toContain('marker-end="url(#');
    expect(output).not.toContain('marker-end="url(#i)"');
    const markerIds = Array.from(
      output.matchAll(/<marker\b[^>]*\bid="([^"]+)"/g),
    ).map((match) => match[1]);
    const markerIdSet = new Set(markerIds);
    const markerRefs = Array.from(
      output.matchAll(/marker-end="url\(#([^"]+)\)"/g),
    ).map((match) => match[1]);
    expect(markerRefs.length).toBeGreaterThan(0);
    expect(markerRefs.every((id) => markerIdSet.has(id))).toBe(true);
    expect(output).not.toContain("parsererror");
  });

  it("keeps a translated marker arrow inside its viewBox", async () => {
    const input = readFileSync("tests/examples/marker-arrow.svg", "utf8");
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.precision = 4;
    optimizer.options.pathPrecision = 4;
    optimizer.options.trimText = false;
    optimizer.options.convertSodipodiArcs = false;
    optimizer.options.removeDefaultValues = false;
    optimizer.options.removeTspan = false;
    optimizer.options.removeStyling = false;
    optimizer.options.groupSimilarElements = false;
    await optimizer.optimizeSvg();

    const doc = new DOMParser().parseFromString(
      optimizer.optimizedSvg,
      "image/svg+xml",
    );
    const path = doc.querySelector("path[marker-end]");
    expect(path?.hasAttribute("transform")).toBe(false);
    expect(path?.getAttribute("d")).toMatch(/^M33\.442 16\.611/);
    expect(path?.getAttribute("d")).toMatch(/l-?\.026\s+-2\.847/);
  });

  it("preserves marker arrowheads when re-optimizing already-optimized SVG", async () => {
    const input = readFileSync("tests/examples/measurements.svg", "utf8");
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    await optimizer.optimizeSvg();
    const firstPass = optimizer.optimizedSvg;

    // Second pass: re-optimize the already-optimized output
    optimizer.originalSvg = firstPass;
    await optimizer.optimizeSvg();
    const secondPass = optimizer.optimizedSvg;

    // All paths that carry marker refs must still be separate (not merged)
    const markerPaths = Array.from(
      secondPass.matchAll(/marker-end="url\(#([^"]+)\)"/g),
    );
    expect(markerPaths.length).toBeGreaterThan(0);

    // No merged multi-subpath (space-separated M commands) on a marker-bearing path
    const mergedMarkerPath =
      /<path\b[^>]*marker-(?:start|end)="[^"]*"[^>]*\bd="[^"]*M[^"]*M/;
    expect(secondPass).not.toMatch(mergedMarkerPath);
  });

  it("moves text elements into trailing group when option is enabled", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><g><text>A</text></g><rect/><text>B</text></svg>';
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.groupTextElementsAtEnd = true;
    await optimizer.optimizeSvg();

    const doc = new DOMParser().parseFromString(
      optimizer.optimizedSvg,
      "image/svg+xml",
    );
    const root = doc.querySelector("svg");
    expect(root).toBeTruthy();

    const children = Array.from(root!.children);
    expect(children[children.length - 1].tagName.toLowerCase()).toBe("g");

    const textValues = Array.from(children[children.length - 1].children).map(
      (el) => (el.textContent || "").trim(),
    );
    expect(textValues).toEqual(["A", "B"]);
  });

  it("preserves grouped text attributes when moving text to the end", async () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      '<text font-size="12" fill="#333">A</text>' +
      '<rect width="1" height="1"/>' +
      '<text font-size="14" fill="#000">X</text>' +
      '<text font-size="12" fill="#333">B</text>' +
      '<text font-size="12" fill="#333">C</text>' +
      "</svg>";
    const optimizer = new SVGOptimizer();
    optimizer.originalSvg = input;
    optimizer.options.groupTextElementsAtEnd = true;
    await optimizer.optimizeSvg();

    const doc = new DOMParser().parseFromString(
      optimizer.optimizedSvg,
      "image/svg+xml",
    );
    const root = doc.querySelector("svg");
    const trailingGroup = root?.lastElementChild;
    expect(trailingGroup?.tagName.toLowerCase()).toBe("g");

    const sharedAttributeGroup = trailingGroup?.querySelector(
      'g[font-size="12"][fill="#333"]',
    );
    const groupedTexts = Array.from(
      sharedAttributeGroup?.children ?? [],
    ).map((el) => el.textContent);
    expect(groupedTexts).toEqual(["A", "B", "C"]);
    expect(sharedAttributeGroup).toBeTruthy();
    expect((optimizer.optimizedSvg.match(/font-size="12"/g) || []).length).toBe(
      1,
    );
  });
});
