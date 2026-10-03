// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import {
  collapseTransforms,
  roundPathData,
  roundNumericValueFixed,
} from "../src/svgUtils";

describe("roundPathData", () => {
  it("separates adjacent numbers after rounding to 0 decimals", () => {
    const input = "M109.1 124.3V33.1l-69.3.3v90.9z";
    const output = roundPathData(input, 0);
    expect(output).toBe("M109 124V33l-69 0v91z");
  });

  it("does not collapse a non-zero relative line to a point", () => {
    const input = "m45.175923 125.37949-.0256-.04605";
    const output = roundPathData(input, 0);
    expect(output).toBe("m45 125 -.03 -.05");
    expect(roundPathData("m45.175923 125.37949-.0256-2.84605", 0)).toBe(
      "m45 125 0 -3",
    );
  });
});

describe("roundNumericValueFixed with fractional-sensitive safety", () => {
  it("excludes opacity and fill-opacity from rounding at precision 0", () => {
    expect(roundNumericValueFixed("0.3", 0, "opacity")).toBe("0.3");
    expect(roundNumericValueFixed("0.31", 0, "opacity")).toBe("0.31");
    expect(roundNumericValueFixed("0.05", 0, "opacity")).toBe("0.05");
    expect(roundNumericValueFixed("0.004", 0, "opacity")).toBe("0.004");
    expect(roundNumericValueFixed("0.0003", 0, "fill-opacity")).toBe("0.0003");
  });

  it("keeps thin stroke-width to 1 decimal when rounding to 0 decimals", () => {
    expect(roundNumericValueFixed("0.4", 0, "stroke-width")).toBe(".4");
    expect(roundNumericValueFixed("0.49", 0, "stroke-width")).toBe(".5");
    expect(roundNumericValueFixed("0.002", 0, "stroke-width")).toBe("0");
    expect(roundNumericValueFixed("0.6", 0, "stroke-width")).toBe("1");
  });

  it("still rounds geometry to whole numbers at 0 decimals", () => {
    expect(roundNumericValueFixed("0.25", 0, "width")).toBe("0");
    expect(roundNumericValueFixed("0.025", 0, "height")).toBe("0");
  });

  it("still rounds normal attributes (like coordinates) to 0", () => {
    expect(roundNumericValueFixed("0.3", 0, "x")).toBe("0");
    expect(roundNumericValueFixed("0.3", 0)).toBe("0");
  });
});

describe("collapseTransforms", () => {
  it("applies translation to an initial relative moveto", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg"><path d="m45.176 125.38-.026-2.847" transform="translate(-11.734 -108.769)"/></svg>';
    const output = collapseTransforms(input);
    const doc = new DOMParser().parseFromString(output, "image/svg+xml");
    const path = doc.querySelector("path");

    expect(path?.getAttribute("transform")).toBeNull();
    expect(path?.getAttribute("d")).toBe(
      "M 33.442 16.61099999999999 l -.026 -2.847",
    );
  });

  it("keeps transforms when userSpaceOnUse gradients are referenced", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      "<defs>" +
      '<linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100" y2="0">' +
      '<stop offset="0" stop-color="#000"/>' +
      '<stop offset="1" stop-color="#fff"/>' +
      "</linearGradient>" +
      "</defs>" +
      '<path fill="url(#g)" d="M0 0h10v10z" transform="translate(5 5)"/>' +
      "</svg>";
    const output = collapseTransforms(input);
    expect(output).toContain('transform="translate(5 5)"');
  });

  it("follows gradient hrefs when deciding to keep transforms", () => {
    const input =
      '<svg xmlns="http://www.w3.org/2000/svg">' +
      "<defs>" +
      '<linearGradient id="base" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100" y2="0">' +
      '<stop offset="0" stop-color="#000"/>' +
      '<stop offset="1" stop-color="#fff"/>' +
      "</linearGradient>" +
      '<linearGradient id="ref" href="#base"/>' +
      "</defs>" +
      '<path fill="url(#ref)" d="M0 0h10v10z" transform="translate(5 5)"/>' +
      "</svg>";
    const output = collapseTransforms(input);
    expect(output).toContain('transform="translate(5 5)"');
  });
});
