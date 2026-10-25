/**
 * Hook that lets the i18n layer localise backend error messages without the API layer
 * depending on it. Both transports (`ApiError` over HTTP, `TauriCommandError` over IPC) route
 * their `message` through here, so every consumer of `error.message` gets the same localised
 * text in browser and desktop hosts. Unknown codes keep the backend's own message.
 */
export type ErrorMessageLocaliser = (code: string, details: unknown) => string | undefined;

let localiser: ErrorMessageLocaliser | undefined;

/** Registers (or clears) the localiser; called once by the i18n module. */
export function setErrorMessageLocaliser(next: ErrorMessageLocaliser | undefined): void {
  localiser = next;
}

/** The localised text for a stable error code, or the backend `fallback` when none exists. */
export function localiseErrorMessage(
  code: string | undefined,
  details: unknown,
  fallback: string,
): string {
  if (code === undefined || localiser === undefined) return fallback;
  return localiser(code, details) ?? fallback;
}

/** Makes `error.message` resolve through the localiser at read time (so locale switches apply). */
export function bindLocalisedMessage(
  error: Error,
  code: string | undefined,
  details: unknown,
  original: string,
): void {
  Object.defineProperty(error, 'message', {
    configurable: true,
    enumerable: false,
    get: () => localiseErrorMessage(code, details, original),
  });
}
