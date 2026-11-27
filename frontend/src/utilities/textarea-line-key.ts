/** Moves to the current newline-delimited line's edge, optionally extending the selection. */
export function handleTextareaLineKey(event: KeyboardEvent, input: HTMLTextAreaElement): boolean {
  const { key, shiftKey } = event;
  if (
    (key !== 'Home' && key !== 'End') ||
    event.isComposing ||
    event.ctrlKey ||
    event.altKey ||
    event.metaKey
  ) {
    return false;
  }

  const { value, selectionStart: start, selectionEnd: end, selectionDirection } = input;
  const backward = selectionDirection === 'backward';
  const caret = backward ? start : end;
  const anchor = shiftKey ? (backward ? end : start) : caret;
  const lineEnd = value.indexOf('\n', caret);
  const lineStart = caret > 0 ? value.lastIndexOf('\n', caret - 1) + 1 : 0;
  const edge = key === 'Home' ? lineStart : lineEnd === -1 ? value.length : lineEnd;
  const from = shiftKey ? Math.min(anchor, edge) : edge;
  const to = shiftKey ? Math.max(anchor, edge) : edge;
  input.setSelectionRange(from, to, edge < anchor ? 'backward' : 'forward');
  event.preventDefault();
  event.stopPropagation();
  return true;
}
