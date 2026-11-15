/**
 * Tab, decided per keystroke: a tool pane (image generation, chat, checksums) is a form like any
 * other, and the person filling it in expects Tab to walk its controls. The shell binds Tab to
 * "switch pane", which is right while reading a directory table and wrong in the middle of a form.
 * This answers which of the two the keystroke belongs to, so the layout handler can stay out of the
 * browser's way while the walk stays inside the pane, and hand the key back at the edges - where
 * switching panes is what a person is looking for.
 */

/** What the browser would stop on, in DOM order. */
const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type=hidden])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
  '[contenteditable]:not([contenteditable="false"])',
].join(',');

/** Whether `element` is a stop the browser can actually reach: hidden subtrees are not. */
function isReachable(element: Element): boolean {
  return element.closest('[hidden]') === null && element.closest('[aria-hidden="true"]') === null;
}

/** The controls `root` offers, in the order Tab would visit them. */
export function tabStops(root: Element): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((element) =>
    isReachable(element),
  );
}

/**
 * Whether a Tab pressed on `event.target` belongs to the controls of `root` rather than to the
 * shell that hops between panes: true while the next stop is still inside it. At the first control
 * Shift+Tab, and at the last one Tab, go back to the shell - the pane cycle stays one keystroke
 * away without anyone having to leave a half-typed prompt to reach it.
 */
export function tabStaysWithin(event: KeyboardEvent, root: Element): boolean {
  if (event.key !== 'Tab' || event.ctrlKey || event.metaKey || event.altKey) return false;
  const target = event.target;
  if (!(target instanceof Element)) return false;
  const control = target.closest<HTMLElement>(FOCUSABLE);
  if (control === null || !root.contains(control)) return false;
  const stops = tabStops(root);
  const index = stops.indexOf(control);
  if (index < 0) return false;
  return event.shiftKey ? index > 0 : index < stops.length - 1;
}
