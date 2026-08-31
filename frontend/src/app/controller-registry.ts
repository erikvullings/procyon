/**
 * AppShell's controller composition seam (task 0153).
 *
 * `AppShell` owns a handful of shell-lifetime controllers/helpers (navigation, tabs, workspace,
 * settings, checksums, comparison, find-files, action commands, the global keydown handler, the
 * pane-content builder) that are each constructed exactly once in `oninit` and live until the
 * shell unmounts. Before this seam, each one was wired by hand: a `let` declaration, an
 * individually-named `create*Controller(...)` call, and (for the one that needed it) a matching
 * teardown call hand-placed in `onremove` dozens of lines away from its construction. Every new
 * shell-lifetime feature (checksums, comparison, ...) repeated that same by-hand ceremony,
 * regrowing the file the same way each time.
 *
 * `buildControllers` replaces that per-controller ceremony with one declarative spec: each entry
 * names a controller, says how to construct it, and optionally how to tear it down. Adding a new
 * shell-lifetime controller means adding one entry to that spec, not new bespoke
 * construction/teardown code inside `AppShell`'s closure.
 */

/** One entry in a {@link buildControllers} spec: how to construct a controller, and - if it holds
 * a resource that must be released - how to tear it back down. */
export interface ControllerEntry<T> {
  create(): T;
  dispose?(instance: T): void;
}

/**
 * Constructs every entry in `spec`, in declaration order, and returns the resulting instances
 * alongside a single `dispose()` that tears every entry that declared one back down, in reverse
 * construction order.
 *
 * `T` (the resulting `instances` shape) is inferred from `spec` itself - each property's
 * `ControllerEntry<...>` pins down that property's instance type, so callers get a fully typed
 * `instances.<name>` per entry without spelling `T` out.
 *
 * Declaration order only matters for `dispose()` - entries may freely reference each other's
 * *eventual* instances (e.g. a controller context's `getWorkspaceController: () => workspaceController`
 * getter) the same way the hand-wired construction this replaces already did, because none of
 * these controllers read another controller's instance synchronously during their own
 * construction - only later, once their methods actually run.
 */
export function buildControllers<T extends Readonly<Record<string, unknown>>>(
  spec: { readonly [K in keyof T]: ControllerEntry<T[K]> },
): { readonly instances: T; dispose(): void } {
  const instances: Record<string, unknown> = {};
  const teardowns: (() => void)[] = [];
  for (const [key, entry] of Object.entries(spec) as [string, ControllerEntry<unknown>][]) {
    const instance = entry.create();
    instances[key] = instance;
    if (entry.dispose !== undefined) {
      const { dispose } = entry;
      teardowns.push(() => dispose(instance));
    }
  }
  return {
    instances: instances as T,
    dispose(): void {
      for (const teardown of [...teardowns].reverse()) teardown();
    },
  };
}
