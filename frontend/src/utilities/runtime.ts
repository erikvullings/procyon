/**
 * Which transport the frontend talks to.
 *
 * The frontend itself is transport-neutral (specification §12): components
 * depend only on `FileManagerClient`, and this value decides once, at
 * bootstrap, which implementation is constructed. The client factory that
 * consumes it arrives in task 0011.
 */
export const RUNTIME_KINDS = ['http', 'tauri', 'mock'] as const;

/** A supported transport. */
export type RuntimeKind = (typeof RUNTIME_KINDS)[number];

/**
 * Used when `VITE_RUNTIME` is unset.
 *
 * Browser/server mode is the default because it is the mode `pnpm dev` starts
 * and the one the Vite proxy is configured for.
 */
export const DEFAULT_RUNTIME_KIND: RuntimeKind = 'http';

/** Raised when the build is configured with a transport that does not exist. */
export class RuntimeConfigurationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RuntimeConfigurationError';
  }
}

/**
 * Validates the `VITE_RUNTIME` build variable.
 *
 * An unset or blank value falls back to {@link DEFAULT_RUNTIME_KIND}. An
 * unrecognised value throws rather than falling back: silently running against
 * the wrong transport is far harder to diagnose than a startup failure.
 *
 * @throws {RuntimeConfigurationError} if `raw` is neither blank nor a known runtime.
 */
export function resolveRuntimeKind(raw: string | undefined): RuntimeKind {
  const normalized = raw?.trim().toLowerCase() ?? '';
  if (normalized === '') {
    return DEFAULT_RUNTIME_KIND;
  }

  const match = RUNTIME_KINDS.find((kind) => kind === normalized);
  if (match !== undefined) {
    return match;
  }

  throw new RuntimeConfigurationError(
    `VITE_RUNTIME="${raw}" is not a supported runtime; expected one of ${RUNTIME_KINDS.join(', ')}.`,
  );
}
