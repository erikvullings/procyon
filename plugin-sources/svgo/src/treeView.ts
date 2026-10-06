import m from "mithril";
import type { Vnode, VnodeDOM } from "mithril";
import { optimizer } from "./optimizer";
import { procyonPlugin } from "./procyon";
import {
  canContainSvgElements,
  ELEMENT_TEMPLATES,
  type ElementPlacement,
  insertSvgElementFromTemplate,
} from "./elementTemplates";
import {
  formatNumberCompact,
  hasRoundableAttrs,
  NUMERIC_ATTRS,
  OPACITY_ATTRS,
  roundAttrsRecursive,
  stepElement,
} from "./svgUtils";
import { getElementByPath, getElementInSvgByPath } from "./treeUtils";

type UncontrolledInputAttrs = {
  value: string;
  onChange: (nextValue: string) => void;
  className?: string;
  onDoubleClick?: (e: MouseEvent) => void;
  onFocus?: (e: FocusEvent) => void;
  onInput?: (nextValue: string) => void;
  onKeyDown?: (e: KeyboardEvent, input: HTMLInputElement) => void;
  type?: string;
};

type UncontrolledInputElement = HTMLInputElement & {
  _uncontrolledAttrs?: UncontrolledInputAttrs;
  _uncontrolledBound?: boolean;
};

type TreeNodeAttrs = {
  node: Element;
  path: string;
  isRoot?: boolean;
  prefix?: string;
  isLast?: boolean;
};

type EditingField = "name" | "value";
type DropPlacement = "before" | "inside" | "after";

let propertiesOpen = !procyonPlugin;

export const TreeView: m.Component = {
  view() {
    if (!optimizer.getSourceSvg()) return m(".tree-view", "No SVG loaded");
    // Always update tree doc to ensure it reflects current state
    optimizer.updateTreeDoc();

    const svg = optimizer.options.treeDoc.querySelector("svg");

    if (!svg) return m(".tree-view", "Invalid SVG");
    const selectedPath = optimizer.options.selectedElementPath;
    const selectedTextNode = selectedPath
      ? getTextNodeByPath(optimizer.options.treeDoc, selectedPath)
      : null;
    const selectedElement =
      selectedTextNode?.parentElement ??
      (selectedPath
        ? getElementByPath(optimizer.options.treeDoc, selectedPath)
        : svg) ??
      svg;
    const inspectorPath = selectedPath
      ? selectedTextNode
        ? getParentPath(selectedPath)
        : selectedPath
      : "0";

    return m(".tree-view", [
      m(".tree-layout", { class: procyonPlugin && !propertiesOpen ? "inspector-collapsed" : "" }, [
        m(".tree-content", [
          m(TreeNode, {
            node: svg,
            path: "0",
            isRoot: true,
            prefix: "",
            isLast: true,
          }),
        ]),
        procyonPlugin ? m(".inspector-rail", [
          m("button.inspector-toggle", {
            title: propertiesOpen ? "Hide properties" : "Show properties",
            "aria-label": propertiesOpen ? "Hide properties" : "Show properties",
            "aria-expanded": String(propertiesOpen),
            onclick: () => { propertiesOpen = !propertiesOpen; },
          }, m("svg[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=2][aria-hidden=true]", [
            m("path", { d: propertiesOpen ? "m14 5-7 7 7 7" : "m10 5 7 7-7 7" }),
          ])),
        ]) : null,
        propertiesOpen ? renderPropertiesInspector(
          selectedElement,
          svg,
          inspectorPath,
          selectedTextNode,
        ) : null,
      ]),
    ]);
  },
};

let dragSourcePath: string | null = null;
let dragOverPath: string | null = null;
let dragOverPlacement: DropPlacement | null = null;
let dragOverValid = false;

const COMMON_ATTRIBUTE_SUGGESTIONS = [
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
  "paint-order",
  "pointer-events",
];

const TEXT_ATTRIBUTE_SUGGESTIONS = [
  "font-size",
  "font-family",
  "font-weight",
  "font-style",
  "text-anchor",
  "dominant-baseline",
  "letter-spacing",
  "word-spacing",
];

/** Attribute names are case-sensitive; stop browsers from "correcting" them. */
const PLAIN_TEXT_INPUT = {
  autocomplete: "off",
  autocorrect: "off",
  autocapitalize: "off",
  spellcheck: false,
} as const;

const ATTRIBUTE_SUGGESTIONS_BY_TAG: Record<string, string[]> = {
  svg: ["width", "height", "viewBox", "preserveAspectRatio"],
  g: ["transform", "opacity", ...TEXT_ATTRIBUTE_SUGGESTIONS, "xml:space"],
  path: ["d", "pathLength"],
  rect: ["x", "y", "width", "height", "rx", "ry"],
  circle: ["cx", "cy", "r"],
  ellipse: ["cx", "cy", "rx", "ry"],
  line: ["x1", "y1", "x2", "y2"],
  polyline: ["points"],
  polygon: ["points"],
  text: ["x", "y", "dx", "dy", ...TEXT_ATTRIBUTE_SUGGESTIONS],
  tspan: ["x", "y", "dx", "dy", ...TEXT_ATTRIBUTE_SUGGESTIONS],
  image: ["href", "x", "y", "width", "height", "preserveAspectRatio"],
  use: ["href", "x", "y", "width", "height"],
  stop: ["offset", "stop-color", "stop-opacity"],
  lineargradient: [
    "x1",
    "y1",
    "x2",
    "y2",
    "gradientUnits",
    "gradientTransform",
  ],
  radialgradient: [
    "cx",
    "cy",
    "r",
    "fx",
    "fy",
    "gradientUnits",
    "gradientTransform",
  ],
  clippath: ["clipPathUnits"],
  mask: ["maskUnits", "maskContentUnits", "x", "y", "width", "height"],
  pattern: [
    "patternUnits",
    "patternContentUnits",
    "x",
    "y",
    "width",
    "height",
    "patternTransform",
  ],
  marker: [
    "markerWidth",
    "markerHeight",
    "refX",
    "refY",
    "orient",
    "markerUnits",
    "viewBox",
    "preserveAspectRatio",
  ],
  symbol: ["viewBox", "preserveAspectRatio"],
};

const elementMenuState: {
  isOpen: boolean;
  path: string | null;
  placement: ElementPlacement;
} = {
  isOpen: false,
  path: null,
  placement: "child",
};

const editingState: {
  path: string | null;
  originalAttrName: string | null;
  field: EditingField;
  isNew: boolean;
  nameInput: string;
  valueInput: string;
  originalValue: string;
  suggestions: string[];
  filteredSuggestions: string[];
  selectedSuggestionIndex: number;
  showSuggestionList: boolean;
} = {
  path: null,
  originalAttrName: null,
  field: "name",
  isNew: false,
  nameInput: "",
  valueInput: "",
  originalValue: "",
  suggestions: [],
  filteredSuggestions: [],
  selectedSuggestionIndex: 0,
  showSuggestionList: false,
};

const ATTRIBUTE_VALUE_SUGGESTIONS: Record<string, string[]> = {
  "text-anchor": ["start", "middle", "end"],
  "dominant-baseline": ["auto", "middle", "central", "hanging", "text-before-edge", "text-after-edge"],
  "font-style": ["normal", "italic", "oblique"],
  "font-weight": ["normal", "bold", "lighter", "bolder"],
  "stroke-linecap": ["butt", "round", "square"],
  "stroke-linejoin": ["miter", "round", "bevel"],
  "fill-rule": ["nonzero", "evenodd"],
  display: ["inline", "none"],
};

function getValueSuggestions(attrName: string): string[] {
  return ATTRIBUTE_VALUE_SUGGESTIONS[attrName.toLowerCase()] ?? [];
}

function isColorAttribute(attrName: string): boolean {
  return ["fill", "stroke", "stop-color", "color"].includes(attrName.toLowerCase());
}

function clearDragTarget() {
  dragOverPath = null;
  dragOverPlacement = null;
  dragOverValid = false;
}

function getDragTargetClass(path: string, placement: DropPlacement) {
  if (dragOverPath !== path || dragOverPlacement !== placement) return "";
  return `drag-over-${placement} ${dragOverValid ? "drag-over-valid" : "drag-over-invalid"}`;
}

function getRowDropPlacement(
  event: DragEvent,
  canInsertChild: boolean,
  isRoot: boolean,
): DropPlacement {
  if (isRoot) return "inside";

  const target = event.currentTarget as HTMLElement;
  const rect = target.getBoundingClientRect();
  const offsetY = event.clientY - rect.top;
  const topZone = rect.height * 0.3;
  const bottomZone = rect.height * 0.7;

  if (offsetY < topZone) return "before";
  if (offsetY > bottomZone) return "after";
  return canInsertChild
    ? "inside"
    : offsetY < rect.height / 2
      ? "before"
      : "after";
}

function isPathInside(sourcePath: string, targetPath: string) {
  return targetPath.startsWith(`${sourcePath}.`);
}

function canDropElement(
  sourcePath: string,
  targetPath: string,
  placement: DropPlacement,
) {
  if (!sourcePath || sourcePath === targetPath) return false;
  if (isPathInside(sourcePath, targetPath)) return false;
  return true;
}

/** Maps a typed name such as `Font-size` onto a known attribute's exact casing. */
function canonicalAttributeName(element: Element, name: string): string {
  const lower = name.toLowerCase();
  const known = [
    ...getAttributeSuggestionsForElement(element),
    ...Object.values(ATTRIBUTE_SUGGESTIONS_BY_TAG).flat(),
  ];
  return known.find((candidate) => candidate.toLowerCase() === lower) ?? name;
}

function getAttributeSuggestionsForElement(element: Element) {
  const tagName = element.tagName.toLowerCase();
  const specific = ATTRIBUTE_SUGGESTIONS_BY_TAG[tagName] || [];
  const existing = Array.from(element.attributes).map((attr) => attr.name);
  const merged = [...existing, ...specific, ...COMMON_ATTRIBUTE_SUGGESTIONS];
  return Array.from(new Set(merged));
}

function openElementMenu(
  path: string,
  placement: ElementPlacement,
  event: MouseEvent,
) {
  event.stopPropagation();
  elementMenuState.isOpen =
    elementMenuState.path !== path ||
    elementMenuState.placement !== placement ||
    !elementMenuState.isOpen;
  elementMenuState.path = path;
  elementMenuState.placement = placement;
  m.redraw();
}

function closeElementMenu() {
  elementMenuState.isOpen = false;
  elementMenuState.path = null;
}

function getChildPath(parentPath: string, parent: Element, child: Element) {
  return `${parentPath}.${Array.from(parent.children).indexOf(child)}`;
}

function getSiblingPath(targetPath: string, child: Element) {
  const parent = child.parentElement;
  if (!parent) return targetPath;
  const parts = targetPath.split(".");
  parts[parts.length - 1] = String(Array.from(parent.children).indexOf(child));
  return parts.join(".");
}

function insertElementAtPath(
  path: string,
  tagName: string,
  placement: ElementPlacement,
) {
  const doc = optimizer.options.treeDoc;
  const target = getElementByPath(doc, path);
  const inserted = insertSvgElementFromTemplate(
    doc,
    target,
    tagName,
    placement,
  );
  if (!target || !inserted) {
    closeElementMenu();
    m.redraw();
    return;
  }

  const insertedPath =
    placement === "child"
      ? getChildPath(path, target, inserted)
      : getSiblingPath(path, inserted);
  optimizer.options.selectedElementPath = insertedPath;
  updateFromTree(doc);
  closeElementMenu();
  setTimeout(() => highlightElement(insertedPath), 0);
}

function renderAddElementMenu(path: string): m.Children {
  if (!elementMenuState.isOpen || elementMenuState.path !== path) return null;

  return m(
    ".element-menu",
    {
      onclick: (e: MouseEvent) => e.stopPropagation(),
    },
    Object.keys(ELEMENT_TEMPLATES).map((tagName) =>
      m(
        "button.element-menu-item",
        {
          type: "button",
          onclick: () =>
            insertElementAtPath(path, tagName, elementMenuState.placement),
        },
        [
          m("span", tagName),
          m(
            "small",
            elementMenuState.placement === "child" ? "append child" : "after",
          ),
        ],
      ),
    ),
  );
}

function resetEditingState() {
  editingState.path = null;
  editingState.originalAttrName = null;
  editingState.field = "name";
  editingState.isNew = false;
  editingState.nameInput = "";
  editingState.valueInput = "";
  editingState.originalValue = "";
  editingState.suggestions = [];
  editingState.filteredSuggestions = [];
  editingState.selectedSuggestionIndex = 0;
  editingState.showSuggestionList = false;
}

function inputId(path: string, attrName: string | null, field: EditingField) {
  const safe = `${path}-${attrName ?? "new"}-${field}`.replace(
    /[^a-z0-9_-]/gi,
    "-",
  );
  return `attr-edit-${safe}`;
}

function focusEditingInput(field: EditingField) {
  const path = editingState.path;
  if (!path) return;
  const id = inputId(path, editingState.originalAttrName, field);
  setTimeout(() => {
    const input = document.getElementById(id) as HTMLInputElement | null;
    input?.focus();
    input?.select();
  }, 0);
}

function startAttributeEditing(
  path: string,
  element: Element,
  attrName: string | null,
  field: EditingField,
) {
  editingState.path = path;
  editingState.originalAttrName = attrName;
  editingState.field = field;
  editingState.isNew = attrName === null;
  editingState.nameInput = attrName ?? "";
  editingState.valueInput = attrName
    ? (element.getAttribute(attrName) ?? "")
    : "";
  editingState.originalValue = editingState.valueInput;
  editingState.suggestions = getAttributeSuggestionsForElement(element);
  updateInlineSuggestionState();
  editingState.showSuggestionList = attrName === null;
  optimizer.options.selectedElementPath = path;
  focusEditingInput(field);
  m.redraw();
}

function isSubsequence(query: string, target: string): boolean {
  if (!query) return true;
  let qi = 0;
  for (let i = 0; i < target.length; i++) {
    if (target[i] === query[qi]) {
      qi++;
      if (qi === query.length) return true;
    }
  }
  return false;
}

function getSuggestionScore(query: string, candidate: string): number {
  const q = query.toLowerCase().trim();
  const c = candidate.toLowerCase();
  if (!q) return 0;
  if (c === q) return 1000;
  if (c.startsWith(q)) return 900 - (c.length - q.length);
  if (c.includes(q)) return 700 - c.indexOf(q);

  const queryTokens = q.split(/[^a-z0-9]+/).filter(Boolean);
  const candidateTokens = c.split(/[^a-z0-9]+/).filter(Boolean);

  if (queryTokens.length > 1 && candidateTokens.length >= queryTokens.length) {
    const positionalPrefixMatch = queryTokens.every((token, index) => {
      const candidateToken = candidateTokens[index];
      return Boolean(candidateToken && candidateToken.startsWith(token));
    });
    if (positionalPrefixMatch) {
      return 850 - (candidateTokens.length - queryTokens.length);
    }
  }

  if (queryTokens.length > 0) {
    const tokenMatch = queryTokens.every((token) =>
      candidateTokens.some(
        (candidateToken) =>
          candidateToken.startsWith(token) || candidateToken.includes(token),
      ),
    );
    if (tokenMatch) return 500 - queryTokens.length;
  }

  const compactQuery = q.replace(/[^a-z0-9]/g, "");
  const compactCandidate = c.replace(/[^a-z0-9]/g, "");
  if (compactQuery && isSubsequence(compactQuery, compactCandidate)) {
    return 300 - (compactCandidate.length - compactQuery.length);
  }

  return -1;
}

function getRankedAttributeSuggestions(
  query: string,
  suggestions: string[],
): string[] {
  const ranked = suggestions
    .map((suggestion) => ({
      suggestion,
      score: getSuggestionScore(query, suggestion),
    }))
    .filter((item) => query.trim() === "" || item.score >= 0)
    .sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return a.suggestion.localeCompare(b.suggestion);
    })
    .slice(0, 12)
    .map((item) => item.suggestion);

  return ranked;
}

function updateInlineSuggestionState(preferredSuggestion?: string): void {
  const ranked = getRankedAttributeSuggestions(
    editingState.nameInput,
    editingState.suggestions,
  );
  editingState.filteredSuggestions = ranked;

  if (ranked.length === 0) {
    editingState.selectedSuggestionIndex = -1;
    return;
  }

  if (preferredSuggestion) {
    const preferredIndex = ranked.indexOf(preferredSuggestion);
    if (preferredIndex >= 0) {
      editingState.selectedSuggestionIndex = preferredIndex;
      return;
    }
  }

  const current = Math.max(
    0,
    Math.min(editingState.selectedSuggestionIndex, ranked.length - 1),
  );
  editingState.selectedSuggestionIndex = current;
}

function syncInlineValueFromExistingAttribute(): void {
  const doc = optimizer.options.treeDoc;
  const element = getElementByPath(
    doc as Document,
    editingState.path as string,
  );
  if (!element) return;

  const existingValue = element.getAttribute(editingState.nameInput.trim());
  if (existingValue !== null && editingState.valueInput === "") {
    editingState.valueInput = existingValue;
  }
}

function acceptSelectedInlineSuggestion(): boolean {
  const { filteredSuggestions, selectedSuggestionIndex } = editingState;
  if (
    filteredSuggestions.length === 0 ||
    selectedSuggestionIndex < 0 ||
    selectedSuggestionIndex >= filteredSuggestions.length
  ) {
    return false;
  }

  const selected = filteredSuggestions[selectedSuggestionIndex];
  if (!selected || selected === editingState.nameInput) {
    return false;
  }

  editingState.nameInput = selected;
  updateInlineSuggestionState(selected);
  syncInlineValueFromExistingAttribute();
  editingState.showSuggestionList = false;
  return true;
}

function saveEditingAttribute(): void {
  const typedName = editingState.nameInput.trim();
  if (!typedName || !editingState.path) {
    resetEditingState();
    m.redraw();
    return;
  }

  const doc = optimizer.options.treeDoc;
  const element = getElementByPath(doc, editingState.path);
  if (!element) {
    resetEditingState();
    m.redraw();
    return;
  }
  const name = canonicalAttributeName(element, typedName);

  try {
    if (
      editingState.originalAttrName &&
      editingState.originalAttrName !== name
    ) {
      element.removeAttribute(editingState.originalAttrName);
    }
    element.setAttribute(name, editingState.valueInput);
    editingState.originalAttrName = name;
    editingState.isNew = false;
    updateFromTree(doc);
    resetEditingState();
  } catch {
    editingState.showSuggestionList = false;
    m.redraw();
  }
}

function cancelEditingAttribute(): void {
  const { path, originalAttrName, originalValue, isNew, nameInput } =
    editingState;
  const doc = optimizer.options.treeDoc;
  const element = path ? getElementByPath(doc, path) : null;

  if (element) {
    const currentName = nameInput.trim();
    if (isNew) {
      if (currentName) element.removeAttribute(currentName);
    } else if (originalAttrName) {
      if (currentName && currentName !== originalAttrName) {
        element.removeAttribute(currentName);
      }
      element.setAttribute(originalAttrName, originalValue);
    }
    updateFromTree(doc);
  } else {
    m.redraw();
  }

  resetEditingState();
}

function applyEditingValueLive(saveHistory = true): void {
  const typedName = editingState.nameInput.trim();
  if (!typedName || !editingState.path) return;

  const doc = optimizer.options.treeDoc;
  const element = getElementByPath(doc, editingState.path);
  if (!element) return;
  const name = canonicalAttributeName(element, typedName);

  try {
    if (
      editingState.originalAttrName &&
      editingState.originalAttrName !== name
    ) {
      element.removeAttribute(editingState.originalAttrName);
    }
    element.setAttribute(name, editingState.valueInput);
    editingState.originalAttrName = name;
    editingState.isNew = false;
    if (saveHistory) {
      updateFromTree(doc);
    }
  } catch {
    // Invalid attribute names are ignored until the user commits a valid one.
  }
}

function selectElement(path: string): void {
  optimizer.options.selectedElementPath = path;
  highlightElement(path);
}

function shouldUseFractionalStep(
  attrName?: string,
  value?: string | null,
): boolean {
  if (optimizer.options.precision !== 0 || !attrName) return false;

  const normalizedAttr = attrName.toLowerCase();
  if (OPACITY_ATTRS.has(normalizedAttr)) return true;

  const num = Number.parseFloat(value ?? "");
  return (
    normalizedAttr === "stroke-width" &&
    Number.isFinite(num) &&
    Math.abs(num) < 0.5
  );
}

export function getPrecisionStep(
  attrName?: string,
  value?: string | null,
): number {
  if (shouldUseFractionalStep(attrName, value)) return 0.1;

  const precision = optimizer.options.precision;
  return precision === 0 ? 1 : Math.pow(10, -precision);
}

export function incrementNumericAttributeValue(
  value: string,
  direction: 1 | -1,
  attrName?: string,
): string | null {
  const match = value.trim().match(/^(-?(?:\d+\.?\d*|\.\d+))(.*)$/);
  if (!match) return null;

  const step = getPrecisionStep(attrName, value);
  const nextValue = Number.parseFloat(match[1]) + direction * step;
  return `${formatIncrementedNumber(nextValue, step)}${match[2] ?? ""}`;
}

function getStepDecimals(step: number): number {
  if (step >= 1) return 0;
  return Math.max(0, Math.ceil(-Math.log10(step)));
}

function formatIncrementedNumber(value: number, step: number): string {
  const precision = optimizer.options.precision;
  const decimals = Math.max(0, precision, getStepDecimals(step));
  return formatNumberCompact(Number.parseFloat(value.toFixed(decimals)));
}

function incrementEditingValue(direction: 1 | -1): boolean {
  const nextValue = incrementNumericAttributeValue(
    editingState.valueInput,
    direction,
    editingState.nameInput,
  );
  if (nextValue === null) return false;

  editingState.valueInput = nextValue;
  applyEditingValueLive(true);
  focusEditingInput("value");
  return true;
}

function incrementAttributeValue(
  path: string,
  element: Element,
  attrName: string,
  direction: 1 | -1,
): boolean {
  const currentValue = element.getAttribute(attrName);
  if (currentValue === null) return false;

  const nextValue = incrementNumericAttributeValue(
    currentValue,
    direction,
    attrName,
  );
  if (nextValue === null) return false;

  selectElement(path);
  element.setAttribute(attrName, nextValue);
  updateFromTree(element.ownerDocument);
  return true;
}

function isDirectEditableNumericAttribute(attrName: string): boolean {
  return NUMERIC_ATTRS.has(attrName.toLowerCase());
}

function updateAttributeValueLive(
  path: string,
  element: Element,
  attrName: string,
  nextValue: string,
): void {
  selectElement(path);
  element.setAttribute(attrName, nextValue);
  updateFromTree(element.ownerDocument);
}

function renderDirectAttributeValue(
  path: string,
  element: Element,
  attr: Attr,
): m.Children {
  return m(".attr-value-container", [
    m(UncontrolledInput, {
      className: "attr-value-direct",
      value: attr.value,
      onFocus: () => selectElement(path),
      onDoubleClick: (e: MouseEvent) => {
        e.stopPropagation();
        startAttributeEditing(path, element, attr.name, "value");
      },
      onInput: (nextValue: string) => {
        updateAttributeValueLive(path, element, attr.name, nextValue);
      },
      onKeyDown: (e: KeyboardEvent, input: HTMLInputElement) => {
        if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;

        const nextValue = incrementNumericAttributeValue(
          input.value,
          e.key === "ArrowUp" ? 1 : -1,
          attr.name,
        );
        if (nextValue === null) return;

        e.preventDefault();
        input.value = nextValue;
        input.size = Math.max(1, Math.min(20, input.value.length));
        updateAttributeValueLive(path, element, attr.name, nextValue);
      },
      onChange: (nextValue: string) => {
        updateAttributeValueLive(path, element, attr.name, nextValue);
      },
    }),
  ]);
}

function renderInlineSuggestions(): m.Children {
  if (
    !editingState.showSuggestionList ||
    editingState.filteredSuggestions.length === 0
  ) {
    return null;
  }

  return m(
    ".attr-suggestion-list.inline",
    editingState.filteredSuggestions.map((suggestion, index) =>
      m(
        "button.attr-suggestion-item",
        {
          class:
            index === editingState.selectedSuggestionIndex ? "selected" : "",
          type: "button",
          onmouseenter: () => {
            editingState.selectedSuggestionIndex = index;
          },
          onmousedown: (e: MouseEvent) => e.preventDefault(),
          onclick: (e: MouseEvent) => {
            e.stopPropagation();
            editingState.nameInput = suggestion;
            updateInlineSuggestionState(suggestion);
            syncInlineValueFromExistingAttribute();
            editingState.showSuggestionList = false;
            editingState.field = "value";
            focusEditingInput("value");
          },
        },
        suggestion,
      ),
    ),
  );
}

function handleNameInputKeydown(e: KeyboardEvent): void {
  e.stopPropagation();

  if (e.key === "Escape") {
    e.preventDefault();
    cancelEditingAttribute();
    return;
  }

  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    if (editingState.filteredSuggestions.length > 0) {
      e.preventDefault();
      const count = editingState.filteredSuggestions.length;
      editingState.selectedSuggestionIndex =
        e.key === "ArrowDown"
          ? (editingState.selectedSuggestionIndex + 1) % count
          : (editingState.selectedSuggestionIndex - 1 + count) % count;
    }
    return;
  }

  if (e.key === "Tab") {
    e.preventDefault();
    acceptSelectedInlineSuggestion();
    editingState.field = "value";
    focusEditingInput("value");
    return;
  }

  if (e.key === "Enter") {
    e.preventDefault();
    if (acceptSelectedInlineSuggestion()) {
      editingState.field = "value";
      focusEditingInput("value");
      return;
    }
    if (editingState.valueInput === "") {
      editingState.field = "value";
      focusEditingInput("value");
      return;
    }
    saveEditingAttribute();
  }
}

function handleValueInputKeydown(e: KeyboardEvent): void {
  e.stopPropagation();

  if (e.key === "Escape") {
    e.preventDefault();
    cancelEditingAttribute();
    return;
  }

  if (e.key === "ArrowUp" || e.key === "ArrowDown") {
    if (incrementEditingValue(e.key === "ArrowUp" ? 1 : -1)) {
      e.preventDefault();
    }
    return;
  }

  if (e.key === "Enter") {
    e.preventDefault();
    saveEditingAttribute();
  }
}

function renderValueSuggestions(): m.Children {
  const suggestions = getValueSuggestions(editingState.nameInput);
  if (editingState.field !== "value" || suggestions.length === 0) return null;

  return m(
    ".attr-suggestion-list.inline.value-suggestions",
    suggestions.map((suggestion) =>
      m(
        "button.attr-suggestion-item",
        {
          type: "button",
          onmousedown: (e: MouseEvent) => e.preventDefault(),
          onclick: (e: MouseEvent) => {
            e.stopPropagation();
            editingState.valueInput = suggestion;
            applyEditingValueLive(true);
            focusEditingInput("value");
          },
        },
        suggestion,
      ),
    ),
  );
}

function renderAttributeEditor(
  path: string,
  attrName: string | null,
): m.Children {
  const isNameActive = editingState.field === "name";
  const nameId = inputId(path, attrName, "name");
  const valueId = inputId(path, attrName, "value");

  return m(".attribute.editing", [
    m(".inline-edit-wrap", [
      m("input.attr-name-input", {
        ...PLAIN_TEXT_INPUT,
        id: nameId,
        value: editingState.nameInput,
        placeholder: "attribute",
        oncreate: ({ dom }: VnodeDOM) => {
          if (isNameActive) (dom as HTMLInputElement).focus();
        },
        oninput: (e: Event) => {
          editingState.nameInput = (e.target as HTMLInputElement).value;
          editingState.showSuggestionList =
            editingState.nameInput.trim() !== "";
          updateInlineSuggestionState();
          syncInlineValueFromExistingAttribute();
        },
        onfocus: () => {
          editingState.field = "name";
          editingState.showSuggestionList =
            editingState.nameInput.trim() !== "";
        },
        onkeydown: handleNameInputKeydown,
        onclick: (e: MouseEvent) => e.stopPropagation(),
      }),
      editingState.field === "name" && renderInlineSuggestions(),
    ]),
    m("span.attr-separator", "="),
    m(".inline-edit-wrap", [
      isColorAttribute(editingState.nameInput) &&
        m("input[type=color].attr-color-input", {
          value: getColorInputValue(editingState.valueInput),
          oninput: (e: Event) => {
            editingState.valueInput = (e.target as HTMLInputElement).value;
            applyEditingValueLive(true);
          },
        }),
    m(`input.attr-value-input${NUMERIC_ATTRS.has(editingState.nameInput.toLowerCase()) ? "[type=number]" : ""}`, {
      ...PLAIN_TEXT_INPUT,
      id: valueId,
      value: editingState.valueInput,
      placeholder: "value",
      oncreate: ({ dom }: VnodeDOM) => {
        if (!isNameActive) (dom as HTMLInputElement).focus();
      },
      oninput: (e: Event) => {
        editingState.valueInput = (e.target as HTMLInputElement).value;
        applyEditingValueLive(true);
      },
      onfocus: () => {
        editingState.field = "value";
        editingState.showSuggestionList = false;
      },
      onkeydown: handleValueInputKeydown,
      onclick: (e: MouseEvent) => e.stopPropagation(),
    }),
      renderValueSuggestions(),
    ]),
  ]);
}

// Uncontrolled Input Component to prevent Mithril redraws from interfering with typing
const UncontrolledInput: m.Component<UncontrolledInputAttrs> = {
  oncreate({ dom, attrs }: VnodeDOM<UncontrolledInputAttrs>) {
    const input = dom as UncontrolledInputElement;
    input._uncontrolledAttrs = attrs;
    input.value = attrs.value;
    input.size = Math.max(1, Math.min(20, input.value.length));

    if (!input._uncontrolledBound) {
      input._uncontrolledBound = true;

      input.addEventListener("keydown", (e: KeyboardEvent) => {
        e.stopPropagation();
        const currentAttrs = input._uncontrolledAttrs;
        if (!currentAttrs) return;

        currentAttrs.onKeyDown?.(e, input);
        if (e.defaultPrevented) return;

        if (e.key === "Escape") {
          input.value = currentAttrs.value;
          input.blur();
        }
        if (e.key === "Enter") {
          input.blur();
        }
      });

      input.addEventListener("input", (e: Event) => {
        const target = e.target as HTMLInputElement;
        target.size = Math.max(1, Math.min(20, target.value.length));
        input._uncontrolledAttrs?.onInput?.(target.value);
      });

      input.addEventListener("change", () => {
        const currentAttrs = input._uncontrolledAttrs;
        if (!currentAttrs) return;

        currentAttrs.type = "change"; // Signal change
        if (input.value !== currentAttrs.value) {
          currentAttrs.onChange(input.value);
        }
      });
    }
  },
  onupdate({ dom, attrs }: VnodeDOM<UncontrolledInputAttrs>) {
    const input = dom as UncontrolledInputElement;
    input._uncontrolledAttrs = attrs;
    // Only update value from model if we are NOT currently editing/focused
    if (document.activeElement !== input) {
      input.value = attrs.value;
      input.size = Math.max(1, Math.min(20, input.value.length));
    }
  },
  view({ attrs }: Vnode<UncontrolledInputAttrs>) {
    return m("input.attr-value", {
      ...PLAIN_TEXT_INPUT,
      class: attrs.className,
      type: attrs.type,
      ondblclick: attrs.onDoubleClick,
      onfocus: attrs.onFocus,
    });
  },
};

const TreeNode: m.Component<TreeNodeAttrs> = {
  view({ attrs }: Vnode<TreeNodeAttrs>) {
    const node = attrs.node;
    const path = attrs.path;
    const isRoot = Boolean(attrs.isRoot);
    const prefix = attrs.prefix ?? "";
    const isLast = Boolean(attrs.isLast);
    if (!node || node.nodeType !== 1) return null;

    const isSelected = optimizer.options.selectedElementPath === path;
    const canInsertChild = canContainSvgElements(node);

    // For text and tspan elements, we want to include both children and text content
    let children: Array<Element | Text> = [];
    if (node.tagName === "text" || node.tagName === "tspan") {
      // Include both regular children and text nodes
      const allNodes = Array.from(node.childNodes) as Array<Element | Text>;
      children = allNodes.filter(
        (n) =>
          n.nodeType === 1 ||
          (n.nodeType === 3 && (n.textContent || "").trim() !== ""),
      );
    } else {
      // For other elements, just use regular children
      children = Array.from(node.children) as Element[];
    }

    const currentPrefix = prefix;
    const childPrefix = prefix + (isLast ? "   " : "│  ");
    const ornament = isRoot ? "" : isLast ? "└─ " : "├─ ";

    return m(".tree-node-wrapper", [
      m(
        ".tree-node-header",
        {
          id: `node-${path.replace(/\./g, "-")}`,
          class: [
            isSelected ? "selected" : "",
            getDragTargetClass(path, "before"),
            getDragTargetClass(path, "inside"),
            getDragTargetClass(path, "after"),
          ].join(" "),
          draggable: !isRoot,
          ondragstart: (e: DragEvent) => {
            dragSourcePath = path;
            e.dataTransfer.setData("text/plain", path);
            e.dataTransfer.effectAllowed = "move";
            document.body.classList.add("tree-dragging-active");
            (e as DragEvent & { redraw?: boolean }).redraw = false;
            e.stopPropagation();
          },
          ondragend: () => {
            dragSourcePath = null;
            clearDragTarget();
            document.body.classList.remove("tree-dragging-active");
          },
          ondragover: (e: DragEvent) => {
            e.preventDefault();
            e.stopPropagation();
            const placement = getRowDropPlacement(e, canInsertChild, isRoot);
            const sourcePath = dragSourcePath ?? "";
            const canDrop =
              canDropElement(sourcePath, path, placement) &&
              (placement !== "inside" || canInsertChild);
            if (e.dataTransfer) e.dataTransfer.dropEffect = canDrop ? "move" : "none";
            dragOverPath = path;
            dragOverPlacement = placement;
            dragOverValid = canDrop;
          },
          ondragleave: () => {
            if (dragOverPath === path) clearDragTarget();
          },
          ondrop: (e: DragEvent) => {
            e.preventDefault();
            e.stopPropagation();
            const sourcePath = e.dataTransfer
              ? e.dataTransfer.getData("text/plain")
              : "";
            const placement = dragOverPlacement ?? getRowDropPlacement(e, canInsertChild, isRoot);
            const canDrop =
              canDropElement(sourcePath, path, placement) &&
              (placement !== "inside" || canInsertChild);
            if (canDrop) {
              moveElementTo(sourcePath, path, placement);
            }
            clearDragTarget();
            dragSourcePath = null;
            document.body.classList.remove("tree-dragging-active");
          },
          onclick: (e: MouseEvent) => {
            e.stopPropagation();
            selectElement(path);
            m.redraw();
          },
        },
        [
          m("span.tree-prefix", currentPrefix + ornament),
          m("span.tag-name", node.tagName),
          m(".attributes", [
            ...Array.from(node.attributes)
              .filter((attr) => attr.name !== "xmlns")
              .map((attr) =>
              editingState.path === path &&
              editingState.originalAttrName === attr.name
                ? renderAttributeEditor(path, attr.name)
                : m(".attribute", [
                    m(
                      "button.attr-name",
                      {
                        type: "button",
                        tabindex: -1,
                        ondblclick: (e: MouseEvent) => {
                          e.stopPropagation();
                          startAttributeEditing(path, node, attr.name, "name");
                        },
                        onclick: (e: MouseEvent) => {
                          e.stopPropagation();
                          selectElement(path);
                        },
                        title: "Double-click to rename",
                      },
                      attr.name,
                    ),
                    m("span.attr-separator", "="),
                    isDirectEditableNumericAttribute(attr.name)
                      ? renderDirectAttributeValue(path, node, attr)
                      : m(
                          "button.attr-value-display",
                          {
                            type: "button",
                            ondblclick: (e: MouseEvent) => {
                              e.stopPropagation();
                              startAttributeEditing(
                                path,
                                node,
                                attr.name,
                                "value",
                              );
                            },
                            onclick: (e: MouseEvent) => {
                              e.stopPropagation();
                              selectElement(path);
                            },
                            onkeydown: (e: KeyboardEvent) => {
                              if (
                                e.key !== "ArrowUp" &&
                                e.key !== "ArrowDown"
                              ) {
                                return;
                              }
                              if (
                                incrementAttributeValue(
                                  path,
                                  node,
                                  attr.name,
                                  e.key === "ArrowUp" ? 1 : -1,
                                )
                              ) {
                                e.preventDefault();
                                e.stopPropagation();
                              }
                            },
                            title: "Double-click to edit",
                          },
                          attr.value,
                        ),
                    attr.value.length > 50 && m(".attr-value-full", attr.value),
                    m(
                      "button.attr-remove",
                      {
                        title: `Remove ${attr.name}`,
                        tabindex: -1,
                        onclick: (e) => {
                          e.stopPropagation();
                          node.removeAttribute(attr.name);
                          updateFromTree(node.ownerDocument);
                        },
                      },
                      "×",
                    ),
                  ]),
            ),
            editingState.path === path &&
            editingState.isNew &&
            editingState.originalAttrName === null
              ? renderAttributeEditor(path, null)
              : null,
          ]),
          m(".node-controls", [
            !(
              editingState.path === path &&
              editingState.isNew &&
              editingState.originalAttrName === null
            ) &&
              m(
                "button.control-btn",
                {
                  tabindex: -1,
                  title: "Add attribute",
                  onclick: (e: MouseEvent) => {
                    e.stopPropagation();
                    startAttributeEditing(path, node, null, "name");
                  },
                },
                "+ attr",
              ),
            canInsertChild &&
              m(
                "button.control-btn",
                {
                  tabindex: -1,
                  title: "Insert child element",
                  onclick: (e: MouseEvent) => openElementMenu(path, "child", e),
                },
                "+ child",
              ),

            !isRoot &&
              m(
                "button.control-btn",
                {
                  tabindex: -1,
                  title: "Insert sibling element",
                  onclick: (e: MouseEvent) =>
                    openElementMenu(path, "sibling", e),
                },
                "+ sib",
              ),

            renderAddElementMenu(path),

            // ⭕ round-to-zero button (only if useful)
            isSelected &&
              hasRoundableAttrs(node) &&
              m(
                "button.control-btn",
                {
                  tabindex: -1,
                  title: "Round numeric attributes to 0 decimals",
                  onclick: (e) => {
                    e.stopPropagation();
                    roundAttrsRecursive(node);
                    updateFromTree(node.ownerDocument);
                  },
                },
                "0",
              ),

            !isRoot &&
              m(
                "button.control-btn",
                {
                  tabindex: -1,
                  onclick: (e) => {
                    e.stopPropagation();
                    moveElement(path, -1);
                  },
                },
                "↑",
              ),

            !isRoot &&
              m(
                "button.control-btn",
                {
                  tabindex: -1,
                  onclick: (e) => {
                    e.stopPropagation();
                    moveElement(path, 1);
                  },
                },
                "↓",
              ),

            m(
              "button.control-btn.danger",
              {
                tabindex: -1,
                onclick: (e) => {
                  e.stopPropagation();
                  node.remove();
                  updateFromTree(node.ownerDocument);
                },
              },
              "×",
            ),
          ]),
        ],
      ),
      m(
        ".tree-node-children",
        children.map((child, index) => {
          // For text nodes, create a special representation
          if (child.nodeType === 3) {
            // Text node - create a special TreeNode-like representation
            const isLastChild = index === children.length - 1;
            const textOrnament = isLastChild ? "└─ " : "├─ ";
            const textPath = `${path}.[-${index + 1}]`;
            return m(".tree-node-wrapper", [
              m(
                ".tree-node-header",
                {
                  class: "text-node",
                  oncreate: (vnode) => {
                    const dom = vnode.dom as HTMLElement;
                    dom.onclick = (e) => {
                      e.stopPropagation();
                      console.log("Text node clicked:", {
                        path: textPath,
                        child,
                      });
                      optimizer.options.selectedElementPath = textPath;
                      highlightElement(textPath);
                      m.redraw();
                    };
                  },
                },
                [
                  m("span.tree-prefix", childPrefix + textOrnament),
                  m("span.tag-name", "[Text Node]"),
                  m(".attributes", [
                    m(".attribute", [
                      m("span.attr-name", "content"),
                      m("span", "="),
                      m(".attr-value-container", [
                        m(UncontrolledInput, {
                          value: child.textContent,
                          onChange: (newValue) => {
                            child.textContent = newValue;
                            updateFromTree(node.ownerDocument);
                          },
                        }),
                        child.textContent.length > 50 &&
                          m(".attr-value-full", child.textContent),
                      ]),
                    ]),
                  ]),
                ],
              ),
            ]);
          } else {
            // Regular element child
            const elementChild = child as Element;
            return m(TreeNode, {
              node: elementChild,
              path: `${path}.${index}`,
              prefix: childPrefix,
              isLast: index === children.length - 1,
            });
          }
        }),
      ),
      null,
    ]);
  },
};

function updateElementAttribute(
  element: Element,
  name: string,
  value: string | null,
): void {
  const normalizedName = name.toLowerCase();
  const normalizedValue =
    typeof value === "string" && NUMERIC_ATTRS.has(normalizedName)
      ? normalizeDecimalNotation(value)
      : value;

  if (normalizedValue === null || normalizedValue === "") {
    element.removeAttribute(name);
  } else {
    element.setAttribute(name, normalizedValue);
  }
  updateFromTree(element.ownerDocument);
}

function normalizeDecimalNotation(value: string): string {
  return value.replace(/,/g, ".");
}

function normalizeNumericAttrValue(attrName: string, value: string): string {
  return NUMERIC_ATTRS.has(attrName.toLowerCase())
    ? normalizeDecimalNotation(value)
    : value;
}

function getParentPath(path: string): string {
  const parts = path.split(".");
  return parts.length > 1 ? parts.slice(0, -1).join(".") : "0";
}

function getTextNodeByPath(doc: Document, path: string): Text | null {
  const parts = path.split(".");
  let current: Element | Text | null = doc.querySelector("svg");
  if (!current || parts[0] !== "0") return null;

  for (let i = 1; i < parts.length; i += 1) {
    const indexStr = parts[i];
    const textMatch = indexStr.match(/^\[-(\d+)\]$/);

    if (textMatch) {
      if (current.nodeType !== 1) return null;
      const nodeIndex = Number.parseInt(textMatch[1], 10) - 1;
      const childNodes = Array.from((current as Element).childNodes);
      const resolvedNode = childNodes[nodeIndex] ?? null;
      if (!resolvedNode || resolvedNode.nodeType !== 3) return null;
      current = resolvedNode;
      continue;
    }

    const childIndex = Number.parseInt(indexStr, 10);
    if (Number.isNaN(childIndex) || current.nodeType !== 1) return null;

    current = ((current as Element).children[childIndex] as Element) ?? null;
    if (!current) return null;
  }

  return current?.nodeType === 3 ? (current as Text) : null;
}

function getInspectorNumericAttrs(element: Element): string[] {
  const byTag =
    ATTRIBUTE_SUGGESTIONS_BY_TAG[element.tagName.toLowerCase()] || [];
  const numeric = [
    "x",
    "y",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "x1",
    "y1",
    "x2",
    "y2",
    "width",
    "height",
    "font-size",
    "stroke-width",
  ];
  return Array.from(
    new Set([
      ...byTag.filter((name) => numeric.includes(name)),
      ...numeric.filter((name) => element.hasAttribute(name)),
    ]),
  ).slice(0, 8);
}

function getColorInputValue(value: string | null): string {
  if (value && /^#[0-9a-f]{6}$/i.test(value)) return value;
  if (value && /^#[0-9a-f]{3}$/i.test(value)) {
    return `#${value
      .slice(1)
      .split("")
      .map((part) => part + part)
      .join("")}`;
  }
  return "#cccccc";
}

function getDirectTextNodes(element: Element): Text[] {
  return Array.from(element.childNodes).filter(
    (node) => node.nodeType === 3,
  ) as Text[];
}

function getDirectTextContent(element: Element): string {
  return getDirectTextNodes(element)
    .map((node) => node.textContent ?? "")
    .join("");
}

function setDirectTextContent(element: Element, value: string): void {
  const directTextNodes = getDirectTextNodes(element);
  if (directTextNodes.length > 0) {
    directTextNodes[0].textContent = value;
    for (let i = 1; i < directTextNodes.length; i += 1) {
      directTextNodes[i].remove();
    }
    return;
  }

  const textNode = element.ownerDocument.createTextNode(value);
  element.insertBefore(textNode, element.firstChild);
}

function renderRemoveAttributeButton(
  element: Element,
  attrName: string,
  title: string,
): m.Children {
  if (!element.hasAttribute(attrName)) return null;
  return m(
    "button.property-attr-remove",
    {
      type: "button",
      title,
      onclick: (e: MouseEvent) => {
        e.stopPropagation();
        updateElementAttribute(element, attrName, null);
      },
    },
    "x",
  );
}

function renderInspectorAttributeInput(element: Element, attr: Attr): m.Children {
  const save = (value: string) =>
    updateElementAttribute(element, attr.name, normalizeNumericAttrValue(attr.name, value));
  const values = getValueSuggestions(attr.name);

  if (isColorAttribute(attr.name)) {
    return m(".property-color-value", [
      m("input[type=color]", { value: getColorInputValue(attr.value), oninput: (e: Event) => save((e.target as HTMLInputElement).value) }),
      m("input.property-text", {
            ...PLAIN_TEXT_INPUT, value: attr.value, onchange: (e: Event) => save((e.target as HTMLInputElement).value) }),
    ]);
  }
  if (values.length > 0) {
    return m("select.property-text", { value: attr.value, onchange: (e: Event) => save((e.target as HTMLSelectElement).value) }, values.map((value) => m("option", { value }, value)));
  }
  return m(`input.property-text${NUMERIC_ATTRS.has(attr.name.toLowerCase()) ? "[type=number]" : ""}`, {
    value: normalizeNumericAttrValue(attr.name, attr.value),
    step: NUMERIC_ATTRS.has(attr.name.toLowerCase()) ? String(getPrecisionStep(attr.name, attr.value)) : undefined,
    onchange: (e: Event) => save((e.target as HTMLInputElement).value),
  });
}

function renderPropertiesInspector(
  selectedElement: Element | null,
  fallbackSvg: Element,
  selectedPath: string,
  selectedTextNode: Text | null,
): m.Children {
  const element = selectedElement ?? fallbackSvg;
  const path = selectedPath || "0";
  const visibility = element.getAttribute("display") !== "none";
  const opacity = normalizeNumericAttrValue(
    "opacity",
    element.getAttribute("opacity") ?? "1",
  );
  const canInsertChild = canContainSvgElements(element);
  const canInsertSibling = path !== "0";
  const hasBothInsertModes = canInsertChild && canInsertSibling;
  const placement: ElementPlacement = hasBothInsertModes
    ? elementMenuState.placement
    : canInsertChild
      ? "child"
      : "sibling";

  const geometryAttrs = getInspectorNumericAttrs(element);
  const quickControlAttrs = new Set(["display", "opacity", "fill", "stroke"]);
  const geometryAttrSet = new Set(
    geometryAttrs.map((name) => name.toLowerCase()),
  );
  const filteredAttributes = Array.from(element.attributes).filter((attr) => {
    const attrName = attr.name.toLowerCase();
    return !quickControlAttrs.has(attrName) && !geometryAttrSet.has(attrName);
  });

  const showTextQuickControl =
    (selectedTextNode !== null &&
      selectedTextNode.parentElement?.tagName.toLowerCase() !== "tspan") ||
    element.tagName.toLowerCase() === "text";
  const quickTextValue =
    selectedTextNode !== null
      ? (selectedTextNode.textContent ?? "")
      : element.tagName.toLowerCase() === "text"
        ? getDirectTextContent(element)
        : "";
  const hasQuickText = quickTextValue.length > 0;

  return m(".properties-inspector", [
    m(".inspector-header", [
      m("span", "Properties"),
      m("strong", `<${element.tagName.toLowerCase()}>`),
    ]),
    m(".inspector-section", [
      m(".inspector-label", "Quick controls"),
      m(".property-row.compact", [
        m("label", "Visible"),
        m("input[type=checkbox]", {
          checked: visibility,
          onchange: (e: Event) => {
            const checked = (e.target as HTMLInputElement).checked;
            updateElementAttribute(element, "display", checked ? null : "none");
          },
        }),
        renderRemoveAttributeButton(element, "display", "Remove display"),
      ]),
      m(".property-row.opacity-row", [
        m("label", "Opacity"),
        m("input[type=range][min=0][max=1][step=0.05]", {
          value: opacity,
          oninput: (e: Event) => {
            updateElementAttribute(
              element,
              "opacity",
              (e.target as HTMLInputElement).value,
            );
          },
        }),
        m("span.property-value", opacity),
        renderRemoveAttributeButton(element, "opacity", "Remove opacity"),
      ]),
      showTextQuickControl &&
        m(".property-row.text-row", [
          m("label", "Text"),
          m("input.property-text", {
            ...PLAIN_TEXT_INPUT,
            value: quickTextValue,
            onchange: (e: Event) => {
              const nextValue = (e.target as HTMLInputElement).value;
              if (selectedTextNode) {
                selectedTextNode.textContent = nextValue;
              } else if (element.tagName.toLowerCase() === "text") {
                setDirectTextContent(element, nextValue);
              }
              updateFromTree(element.ownerDocument);
            },
          }),
          hasQuickText &&
            m(
              "button.property-attr-remove",
              {
                type: "button",
                title: "Clear text",
                onclick: (e: MouseEvent) => {
                  e.stopPropagation();
                  if (selectedTextNode) {
                    selectedTextNode.textContent = "";
                  } else if (element.tagName.toLowerCase() === "text") {
                    setDirectTextContent(element, "");
                  }
                  updateFromTree(element.ownerDocument);
                },
              },
              "x",
            ),
        ]),
      ["fill", "stroke"].map((attrName) =>
        m(".property-row.color-row", [
          m("label", attrName),
          m("input[type=color]", {
            value: getColorInputValue(element.getAttribute(attrName)),
            oninput: (e: Event) => {
              updateElementAttribute(
                element,
                attrName,
                (e.target as HTMLInputElement).value,
              );
            },
          }),
          m("input.property-text", {
            ...PLAIN_TEXT_INPUT,
            value: element.getAttribute(attrName) ?? "",
            placeholder: "none",
            onchange: (e: Event) => {
              updateElementAttribute(
                element,
                attrName,
                (e.target as HTMLInputElement).value,
              );
            },
          }),
          renderRemoveAttributeButton(element, attrName, `Remove ${attrName}`),
        ]),
      ),
    ]),
    geometryAttrs.length > 0 &&
      m(".inspector-section", [
        m(".inspector-label", "Geometry"),
        geometryAttrs.map((attrName) =>
          m(".property-row", [
            m("label", attrName),
            m("input[type=number]", {
              lang: "en",
              step: String(
                getPrecisionStep(attrName, element.getAttribute(attrName)),
              ),
              value: normalizeNumericAttrValue(
                attrName,
                element.getAttribute(attrName) ?? "",
              ),
              onchange: (e: Event) => {
                updateElementAttribute(
                  element,
                  attrName,
                  normalizeNumericAttrValue(
                    attrName,
                    (e.target as HTMLInputElement).value,
                  ),
                );
              },
            }),
            renderRemoveAttributeButton(element, attrName, `Remove ${attrName}`),
          ]),
        ),
      ]),
    filteredAttributes.length > 0 &&
      m(".inspector-section.attr-grid", [
        m(".inspector-label", "Attributes"),
        filteredAttributes.map((attr) =>
          m(".property-row", [
            m("label", attr.name),
            renderInspectorAttributeInput(element, attr),
            m(
              "button.property-attr-remove",
              {
                type: "button",
                title: `Remove ${attr.name}`,
                onclick: (e: MouseEvent) => {
                  e.stopPropagation();
                  updateElementAttribute(element, attr.name, null);
                },
              },
              "x",
            ),
          ]),
        ),
      ]),
    m(".inspector-section", [
      hasBothInsertModes
        ? m(".inspector-inline-header", [
            m(".inspector-label", "Insert"),
            m(".segmented", [
              m(
                "button",
                {
                  class: placement === "child" ? "active" : "",
                  type: "button",
                  onclick: () => {
                    elementMenuState.placement = "child";
                  },
                },
                "Child",
              ),
              m(
                "button",
                {
                  class: placement === "sibling" ? "active" : "",
                  type: "button",
                  onclick: () => {
                    elementMenuState.placement = "sibling";
                  },
                },
                "Sibling",
              ),
            ]),
          ])
        : m(
            ".inspector-label",
            canInsertChild ? "INSERT CHILD" : "INSERT SIBLING",
          ),
      m(
        ".insert-bar",
        Object.keys(ELEMENT_TEMPLATES).map((tagName) =>
          m(
            "button",
            {
              type: "button",
              onclick: () => insertElementAtPath(path, tagName, placement),
            },
            tagName,
          ),
        ),
      ),
    ]),
  ]);
}

function updateFromTree(doc: Document): void {
  optimizer.options.isUpdatingFromTree = true;
  optimizer.originalSvg = new XMLSerializer().serializeToString(doc);
  if (optimizer.editor) {
    optimizer.editor.setValue(optimizer.originalSvg);
  }
  optimizer.updateTreeDoc();
  optimizer.optimizeSvg();
  optimizer.saveToHistory();
  optimizer.options.isUpdatingFromTree = false;
  m.redraw();
}

function getElementPath(root: Element, element: Element): string | null {
  if (root === element) return "0";
  const path: number[] = [];
  let current: Element | null = element;

  while (current && current !== root) {
    const parent = current.parentElement;
    if (!parent) return null;
    path.unshift(Array.from(parent.children).indexOf(current));
    current = parent;
  }

  return current === root ? `0.${path.join(".")}` : null;
}

function moveElementTo(
  sourcePath: string,
  targetPath: string,
  placement: DropPlacement,
): void {
  if (!canDropElement(sourcePath, targetPath, placement)) return;

  const doc = optimizer.options.treeDoc;
  const source = getElementByPath(doc, sourcePath);
  const target = getElementByPath(doc, targetPath);

  if (source && target) {
    if (
      placement === "inside" &&
      !canContainSvgElements(target)
    ) {
      return;
    }

    const targetParent = target.parentElement;

    if (placement === "inside") {
      target.appendChild(source);
    } else if (placement === "before" && targetParent) {
      targetParent.insertBefore(source, target);
    } else if (placement === "after" && targetParent) {
      targetParent.insertBefore(source, target.nextElementSibling);
    }

    const svg = doc.querySelector("svg");
    const movedPath = svg ? getElementPath(svg, source) : null;
    if (movedPath) optimizer.options.selectedElementPath = movedPath;
    updateFromTree(doc);
  }
}

function moveElement(path: string, direction: -1 | 1): void {
  const doc = optimizer.options.treeDoc;
  const element = getElementByPath(doc, path);
  if (!element || !stepElement(element, direction)) return;

  const svg = doc.querySelector("svg");
  const movedPath = svg ? getElementPath(svg, element) : null;
  if (movedPath) optimizer.options.selectedElementPath = movedPath;
  updateFromTree(doc);
}

function highlightElement(path: string): void {
  console.log("highlightElement called with:", path);
  const previewContainer = document.querySelector(
    ".preview-container",
  ) as HTMLElement | null;
  const previewSvg = previewContainer
    ? (previewContainer.querySelector("svg") as SVGGraphicsElement | null)
    : null;
  if (!previewSvg) {
    console.log("No preview SVG found!");
    return;
  }

  previewSvg
    .querySelectorAll(".highlighted-preview")
    .forEach((el) => el.classList.remove("highlighted-preview"));

  const targetEl = getElementInSvgByPath(
    previewSvg,
    path,
  ) as SVGGraphicsElement | null;
  console.log("Target element:", targetEl);

  if (targetEl && typeof targetEl.getBBox === "function") {
    try {
      targetEl.classList.add("highlighted-preview");
      console.log("Element highlighted");
    } catch (e) {
      console.warn("Could not highlight element:", e);
    }
  } else {
    console.log(
      "Could not highlight element: targetEl not found or no getBBox",
    );
  }
}
