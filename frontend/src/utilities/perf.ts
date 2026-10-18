/**
 * Thin wrapper over the User Timing API for navigation/refresh profiling (task 0258). Entries
 * appear as `fm:*` measures in the devtools Performance panel and via
 * `performance.getEntriesByType('measure')`. Never throws: timing must not affect behaviour.
 */

const PREFIX = 'fm:';

/** Returns a start timestamp to pass to {@link perfEnd}. */
export function perfStart(): number {
  try {
    return performance.now();
  } catch {
    return 0;
  }
}

/** Records a measure named `fm:<name>` from `start` to now, with optional structured detail. */
export function perfEnd(name: string, start: number, detail?: Record<string, unknown>): void {
  try {
    performance.measure(`${PREFIX}${name}`, { start, detail });
  } catch {
    // Timing is best-effort.
  }
}

/** Like {@link perfEnd}, but measured to the next animation frame, i.e. until the UI has painted. */
export function perfEndAfterPaint(
  name: string,
  start: number,
  detail?: Record<string, unknown>,
): void {
  try {
    requestAnimationFrame(() => perfEnd(name, start, detail));
  } catch {
    perfEnd(name, start, detail);
  }
}
