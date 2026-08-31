/** A drawer is visible only while its terminal still belongs to the active tab. */
export function isTerminalVisible(
  openTabKeys: ReadonlySet<string>,
  activeTabKey: string | undefined,
): boolean {
  return activeTabKey !== undefined && openTabKeys.has(activeTabKey);
}
