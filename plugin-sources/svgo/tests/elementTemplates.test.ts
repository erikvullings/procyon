// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import {
  canContainSvgElements,
  createSvgElementFromTemplate,
  insertSvgElementFromTemplate,
} from "../src/elementTemplates";
import { optimizer } from "../src/optimizer";
import {
  getPrecisionStep,
  incrementNumericAttributeValue,
} from "../src/treeView";

function parseSvg(input: string): Document {
  return new DOMParser().parseFromString(input, "image/svg+xml");
}

describe("SVG element templates", () => {
  it("creates visible defaults for primitive elements", () => {
    const doc = parseSvg('<svg xmlns="http://www.w3.org/2000/svg"></svg>');
    const circle = createSvgElementFromTemplate(doc, "circle");
    const text = createSvgElementFromTemplate(doc, "text");

    expect(circle?.tagName).toBe("circle");
    expect(circle?.getAttribute("cx")).toBe("25");
    expect(circle?.getAttribute("r")).toBe("20");
    expect(circle?.getAttribute("fill")).toBe("#ccc");
    expect(text?.textContent).toBe("Text Content");
    expect(text?.getAttribute("font-size")).toBe("16");
  });

  it("inserts a template as a child of the target element", () => {
    const doc = parseSvg(
      '<svg xmlns="http://www.w3.org/2000/svg"><g id="target"></g></svg>',
    );
    const target = doc.querySelector("#target");
    const inserted = insertSvgElementFromTemplate(doc, target, "rect", "child");

    expect(inserted?.tagName).toBe("rect");
    expect(target?.children.length).toBe(1);
    expect(target?.firstElementChild).toBe(inserted);
  });

  it("does not insert child elements into leaf elements", () => {
    const doc = parseSvg(
      '<svg xmlns="http://www.w3.org/2000/svg"><path id="target" d="M0 0"/></svg>',
    );
    const target = doc.querySelector("#target");
    const inserted = insertSvgElementFromTemplate(doc, target, "rect", "child");

    expect(canContainSvgElements(target as Element)).toBe(false);
    expect(inserted).toBeNull();
    expect(target?.children.length).toBe(0);
  });

  it("inserts a template as the next sibling of the target element", () => {
    const doc = parseSvg(
      '<svg xmlns="http://www.w3.org/2000/svg"><circle id="a"/><path id="b"/></svg>',
    );
    const target = doc.querySelector("#a");
    const inserted = insertSvgElementFromTemplate(
      doc,
      target,
      "line",
      "sibling",
    );
    const children = Array.from(doc.querySelector("svg")?.children ?? []);

    expect(inserted?.tagName).toBe("line");
    expect(children.map((child) => child.tagName)).toEqual([
      "circle",
      "line",
      "path",
    ]);
  });

  it("returns null for unsupported tag names without changing the document", () => {
    const doc = parseSvg(
      '<svg xmlns="http://www.w3.org/2000/svg"><g id="target"></g></svg>',
    );
    const target = doc.querySelector("#target");
    const inserted = insertSvgElementFromTemplate(
      doc,
      target,
      "script",
      "child",
    );

    expect(inserted).toBeNull();
    expect(target?.children.length).toBe(0);
  });

  it("uses whole-number inspector steps at 0 precision", () => {
    const previousPrecision = optimizer.options.precision;
    try {
      optimizer.options.precision = 0;
      expect(getPrecisionStep()).toBe(1);
      expect(getPrecisionStep("stroke-width", "0.3")).toBe(0.1);
      expect(getPrecisionStep("stroke-width", "0.6")).toBe(1);
      expect(getPrecisionStep("opacity", "0.3")).toBe(0.1);
    } finally {
      optimizer.options.precision = previousPrecision;
    }
  });

  it("increments numeric tree attribute values with the active precision", () => {
    const previousPrecision = optimizer.options.precision;
    try {
      optimizer.options.precision = 1;
      expect(incrementNumericAttributeValue("42", 1)).toBe("42.1");
      expect(incrementNumericAttributeValue("42.1px", -1)).toBe("42px");

      optimizer.options.precision = 2;
      expect(incrementNumericAttributeValue("42", 1)).toBe("42.01");

      optimizer.options.precision = 0;
      expect(incrementNumericAttributeValue("42", 1)).toBe("43");
      expect(incrementNumericAttributeValue("42px", -1)).toBe("41px");
      expect(incrementNumericAttributeValue("0.3", -1, "stroke-width")).toBe(
        ".2",
      );
      expect(incrementNumericAttributeValue("0.3", 1, "stroke-width")).toBe(
        ".4",
      );
      expect(incrementNumericAttributeValue("0.3", -1, "opacity")).toBe(".2");
    } finally {
      optimizer.options.precision = previousPrecision;
    }
  });
});
