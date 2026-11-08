/** Writes `value` into the text input, focuses it and selects it, so typing replaces the suggestion. */
export function focusAndSelect(id: string, value?: string): void {
  const input = document.getElementById(id);
  if (!(input instanceof HTMLInputElement)) return;
  if (value !== undefined) input.value = value;
  input.focus();
  input.select();
}
