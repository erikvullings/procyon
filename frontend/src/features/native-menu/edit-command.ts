/** Standard Edit-menu commands carried by the macOS menu bar as app-dispatched items. */
export type EditCommand = 'cut' | 'copy' | 'paste' | 'selectAll';

const EDIT_COMMAND_KEYS: Record<EditCommand, string> = {
  cut: 'x',
  copy: 'c',
  paste: 'v',
  selectAll: 'a',
};

/**
 * Replays an Edit-menu command as its Cmd+<key> keydown on the focused element, so the page's
 * ordinary key path (file clipboard, quick filter, editors) decides what it means.
 *
 * On macOS a menu item's key equivalent consumes the chord before WKWebView delivers it as a
 * keydown, so without this replay Cmd+C in a pane never reached the file-clipboard handler and
 * a role-based Copy item beeped for lack of a text selection. When no page handler claims the
 * replayed event (focus in a text field, plain text selection), `fallback` performs the native
 * text command through the responder chain instead.
 */
export function replayEditCommand(
  command: EditCommand,
  target: Element | null,
  fallback: (command: EditCommand) => void,
): void {
  const key = EDIT_COMMAND_KEYS[command];
  const event = new KeyboardEvent('keydown', {
    key,
    code: `Key${key.toUpperCase()}`,
    metaKey: true,
    bubbles: true,
    cancelable: true,
    composed: true,
  });
  const notCancelled = (target ?? document.body).dispatchEvent(event);
  if (notCancelled) fallback(command);
}
