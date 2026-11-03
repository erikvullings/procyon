/**
 * Thin wrapper over the User Timing API for navigation/refresh profiling (task 0258). Entries
 * appear as `fm:*` measures in the devtools Performance panel and via
 * `performance.getEntriesByType('measure')`. Never throws: timing must not affect behaviour.
 */

const PREFIX = 'fm:';

/** Receives every finished measure, so a host can persist slow ones (see the Tauri adapter). */
export type PerfSink = (name: string, durationMs: number, detail?: Record<string, unknown>) => void;

let sink: PerfSink | undefined;

/** Installs (or with `undefined`, removes) the sink that sees every finished measure. */
export function setPerfSink(next: PerfSink | undefined): void {
  sink = next;
}

/** True when a host installed a sink, i.e. periodic statistics are worth collecting. */
export function hasPerfSink(): boolean {
  return sink !== undefined;
}

/** Reports a point-in-time statistic (for example cache sizes) to the sink, never as a measure. */
export function perfStat(name: string, values: Record<string, number>): void {
  try {
    sink?.(`stat-${name}`, 0, values);
  } catch {
    // Statistics are best-effort.
  }
}

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
    sink?.(name, performance.now() - start, detail);
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

/**
 * Like {@link perfEndAfterPaint}, but waits two frames. A Mithril redraw triggered by the same event
 * may run after the first frame callback, so one frame can finish before the UI has re-rendered.
 */
export function perfEndAfterRender(
  name: string,
  start: number,
  detail?: Record<string, unknown>,
): void {
  try {
    requestAnimationFrame(() => requestAnimationFrame(() => perfEnd(name, start, detail)));
  } catch {
    perfEnd(name, start, detail);
  }
}

/** Records a point-in-time `fm:<name>` mark (ms since page navigation start). */
export function perfMark(name: string, detail?: Record<string, unknown>): void {
  try {
    performance.mark(`${PREFIX}${name}`, { detail });
  } catch {
    // Timing is best-effort.
  }
}

const firstMarks = new Set<string>();

/** Like {@link perfMark}, but only the first call per name records. */
export function perfMarkOnce(name: string, detail?: Record<string, unknown>): void {
  if (firstMarks.has(name)) return;
  firstMarks.add(name);
  perfMark(name, detail);
}
