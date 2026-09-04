/** A drawer is visible only while its terminal still belongs to the active tab. */
export function isTerminalVisible(
  openTabKeys: ReadonlySet<string>,
  activeTabKey: string | undefined,
): boolean {
  return activeTabKey !== undefined && openTabKeys.has(activeTabKey);
}

/** Makes xterm mount during the redraw before asking its registered callback to focus it. */
export function focusOpenedTerminal(redraw: () => void, focus: () => boolean): boolean {
  redraw();
  return focus();
}
