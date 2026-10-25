import { bindLocalisedMessage } from '../error-localiser';

/**
 * A rejected Tauri command. `invoke` rejects with whatever the Rust command returned as `Err`:
 * a plain `{ code, message, requestId, details }` object for application errors, or a bare
 * string. Neither is an `Error`, so every `error instanceof Error ? error.message : fallback`
 * in the UI would drop the reason and show a generic text. This wraps the value so the message
 * survives, and keeps the structured fields for callers that inspect them.
 */
export class TauriCommandError extends Error {
  readonly code: string | undefined;
  readonly requestId: string | undefined;
  readonly details: unknown;

  constructor(
    readonly command: string,
    message: string,
    fields: {
      code?: string | undefined;
      requestId?: string | undefined;
      details?: unknown;
    } = {},
  ) {
    super(message);
    this.name = 'TauriCommandError';
    this.code = fields.code;
    this.requestId = fields.requestId;
    this.details = fields.details;
    bindLocalisedMessage(this, fields.code, fields.details, message);
  }
}

/** Converts a rejection value from `invoke` into an `Error` that carries its message. */
export function toCommandError(command: string, rejection: unknown): Error {
  if (rejection instanceof Error) return rejection;
  if (typeof rejection === 'string') return new TauriCommandError(command, rejection);
  if (typeof rejection === 'object' && rejection !== null) {
    const record = rejection as Record<string, unknown>;
    const text = (value: unknown): string | undefined =>
      typeof value === 'string' && value.length > 0 ? value : undefined;
    const code = text(record.code);
    const message = text(record.message) ?? code ?? `${command} failed`;
    return new TauriCommandError(command, message, {
      code,
      requestId: text(record.requestId),
      details: record.details,
    });
  }
  return new TauriCommandError(command, `${command} failed`);
}
