import { technicalTextInputAttrs } from './technical-text-input';

/** Input types where autocomplete, capitalisation and spell-check make no sense or are irrelevant. */
const SKIPPED_TYPES = new Set(['checkbox', 'radio', 'range', 'color', 'file', 'hidden', 'button']);

type TextControl = HTMLInputElement | HTMLTextAreaElement;

const isTextControl = (node: Node): node is TextControl =>
  node instanceof HTMLTextAreaElement ||
  (node instanceof HTMLInputElement && !SKIPPED_TYPES.has(node.type));

/** Sets the technical-text attributes the element does not already carry itself. */
export function applyTechnicalTextDefaults(control: TextControl): void {
  for (const [name, value] of Object.entries(technicalTextInputAttrs)) {
    if (!control.hasAttribute(name)) {
      control.setAttribute(name, String(value));
    }
  }
}

function applyWithin(root: ParentNode): void {
  for (const control of root.querySelectorAll<TextControl>('input, textarea')) {
    if (isTextControl(control)) {
      applyTechnicalTextDefaults(control);
    }
  }
}

/**
 * Every text input and textarea in the app, including those rendered by library components,
 * gets `autocomplete`, `autocapitalize`, `autocorrect` off and `spellcheck` false: the browser must
 * not offer saved values, capitalise the first letter or correct commands, paths and queries.
 * An element that sets one of these attributes itself (e.g. a password field) keeps its value.
 * Returns a function that stops observing.
 */
export function installGlobalInputDefaults(root: Document = document): () => void {
  applyWithin(root);
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      for (const added of record.addedNodes) {
        if (isTextControl(added)) {
          applyTechnicalTextDefaults(added);
        } else if (added instanceof Element) {
          applyWithin(added);
        }
      }
      if (record.type === 'attributes' && isTextControl(record.target)) {
        applyTechnicalTextDefaults(record.target);
      }
    }
  });
  observer.observe(root, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['type'],
  });
  return () => observer.disconnect();
}
