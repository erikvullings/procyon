/**
 * Marks an in-progress action button. A busy button is not unavailable: the
 * shared `fm-busy-button` style keeps its label at full text contrast while it
 * stays non-clickable through the caller's `disabled` attribute.
 */
export function busyButtonAttrs(isBusy: boolean): {
  readonly 'aria-busy'?: 'true';
  readonly class?: string;
} {
  return isBusy ? { 'aria-busy': 'true', class: 'fm-busy-button' } : {};
}
