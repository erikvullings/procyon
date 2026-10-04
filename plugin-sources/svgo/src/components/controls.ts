import m from "mithril";
import { SVGOptimizer } from "../optimizer";

export type ControlsAttrs = {
  optimizer: SVGOptimizer;
};

export const Controls: m.Component<ControlsAttrs> = {
  view({ attrs }) {
    const { optimizer } = attrs;
    return m(".controls", [
      m(".option-section", [
        m(".section-title", "Precision"),
        m(".checkbox-group.precision-row", [
          m("label", "General:"),
          m("input.number-input[type=number]", {
            value: optimizer.options.precision,
            min: 0,
            max: 5,
            onchange: (e: Event) => {
              optimizer.options.precision = parseInt(
                (e.target as HTMLInputElement).value,
                10,
              );
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
        ]),
        m(".checkbox-group.precision-row", [
          m("label", "Path:"),
          m("input.number-input[type=number]", {
            value: optimizer.options.pathPrecision,
            min: 0,
            max: 5,
            onchange: (e: Event) => {
              optimizer.options.pathPrecision = parseInt(
                (e.target as HTMLInputElement).value,
                10,
              );
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
        ]),
      ]),
      m(".option-section", [
        m(".section-title", "Cleanup"),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "trim-text",
            checked: optimizer.options.trimText,
            onchange: (e: Event) => {
              optimizer.options.trimText = (e.target as HTMLInputElement).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m(
            "label",
            { for: "trim-text", title: "Removes leading and trailing whitespace. Disable this if that whitespace is intentional." },
            "Trim text content",
          ),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "convert-sodipodi",
            checked: optimizer.options.convertSodipodiArcs,
            onchange: (e: Event) => {
              optimizer.options.convertSodipodiArcs = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("label", { for: "convert-sodipodi" }, "Convert sodipodi arcs"),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "remove-defaults",
            checked: optimizer.options.removeDefaultValues,
            onchange: (e: Event) => {
              optimizer.options.removeDefaultValues = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("label", { for: "remove-defaults" }, "Remove default values"),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "remove-font-family",
            checked: optimizer.options.removeFontFamily,
            onchange: (e: Event) => {
              optimizer.options.removeFontFamily = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("label", { for: "remove-font-family" }, "Remove font-family"),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "remove-font-size",
            checked: optimizer.options.removeFontSize,
            onchange: (e: Event) => {
              optimizer.options.removeFontSize = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("label", { for: "remove-font-size" }, "Remove font-size"),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "remove-tspan",
            checked: optimizer.options.removeTspan,
            onchange: (e: Event) => {
              optimizer.options.removeTspan = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("label", { for: "remove-tspan" }, "Remove tspan"),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "remove-styling",
            checked: optimizer.options.removeStyling,
            onchange: (e: Event) => {
              optimizer.options.removeStyling = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("label", { for: "remove-styling" }, "Remove styling"),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "group-similar-elements",
            checked: optimizer.options.groupSimilarElements,
            onchange: (e: Event) => {
              optimizer.options.groupSimilarElements = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m(
            "label",
            { for: "group-similar-elements" },
            "Group similar elements",
          ),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "group-text-elements-end",
            checked: optimizer.options.groupTextElementsAtEnd,
            onchange: (e: Event) => {
              optimizer.options.groupTextElementsAtEnd = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m(
            "label",
            { for: "group-text-elements-end" },
            "Move text to end",
          ),
        ]),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "auto-autocrop",
            checked: optimizer.options.autoAutocrop,
            onchange: (e: Event) => {
              optimizer.options.autoAutocrop = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("label", { for: "auto-autocrop" }, "Auto-autocrop viewBox"),
        ]),
      ]),
      m(".option-section", [
        m(".section-title", "Custom Size"),
        m(".checkbox-group", [
          m("input[type=checkbox]", {
            id: "custom-dimensions",
            checked: optimizer.options.useCustomDimensions,
            onchange: (e: Event) => {
              optimizer.options.useCustomDimensions = (
                e.target as HTMLInputElement
              ).checked;
              optimizer.optimizeSvg();
              optimizer.saveToHistory();
            },
          }),
          m("input.dimension-input[type=number]", {
            value: optimizer.options.customWidth,
            placeholder: "Width",
            onchange: (e: Event) => {
              optimizer.options.customWidth = parseInt(
                (e.target as HTMLInputElement).value,
                10,
              );
              if (optimizer.options.useCustomDimensions) {
                optimizer.optimizeSvg();
                optimizer.saveToHistory();
              } else {
                optimizer.persistSessionState();
              }
            },
          }),
          m("span", "×"),
          m("input.dimension-input[type=number]", {
            value: optimizer.options.customHeight,
            placeholder: "Height",
            onchange: (e: Event) => {
              optimizer.options.customHeight = parseInt(
                (e.target as HTMLInputElement).value,
                10,
              );
              if (optimizer.options.useCustomDimensions) {
                optimizer.optimizeSvg();
                optimizer.saveToHistory();
              } else {
                optimizer.persistSessionState();
              }
            },
          }),
        ]),
      ]),
    ]);
  },
};
