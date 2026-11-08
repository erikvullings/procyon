import { errorText } from '../../i18n';

/** Error codes whose message names something the user can fix (model missing, service down, ...). */
const ACTIONABLE_CODES: ReadonlySet<string> = new Set([
  'invalidRequest',
  'platformOperationFailed',
]);

/**
 * The text to show when generating an answer failed. A rejection with a stable reason (e.g. "the
 * configured AI model is not available") is shown as localised; any other failure keeps the generic
 * `fallback`, so internal details never leak into the UI.
 */
export function generationErrorText(cause: unknown, fallback: string): string {
  const code =
    typeof cause === 'object' && cause !== null && 'code' in cause && typeof cause.code === 'string'
      ? cause.code
      : undefined;
  return code !== undefined && ACTIONABLE_CODES.has(code) ? errorText(cause) : fallback;
}
