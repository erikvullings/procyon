import type { DiagnosticErrorDto } from '../api/generated/models/diagnosticErrorDto';

export type FrontendDiagnostic = DiagnosticErrorDto;

/** Mirrors the host-side privacy boundary for the in-process mock runtime. */
export function sanitizeFrontendDiagnostic(error: FrontendDiagnostic): FrontendDiagnostic {
  return {
    timestamp: truncate(error.timestamp, 64),
    code: sanitizeCode(error.code),
    message: truncate(redact(error.message), 4_096),
    ...(error.context == null ? {} : { context: truncate(redact(error.context), 8_192) }),
  };
}

function sanitizeCode(code: string): string {
  const sanitized = [...code]
    .slice(0, 64)
    .map((character) => (/^[a-z0-9_]$/i.test(character) ? character.toUpperCase() : '_'))
    .join('');
  return sanitized || 'FRONTEND_ERROR';
}

function redact(value: string): string {
  return value
    .replace(/Bearer\s+\S+/gi, '******')
    .replace(
      /(apikey|api_key|secret_key|private_key|access_key|sk_live_|pk_live_|sk-|pk-|sk_)[\s:=]*[\w._-]{4,}/gi,
      '$1 [REDACTED]',
    )
    .replace(
      /(token|session|sessionid|session_id|auth|x-auth-token)\s*[:=]\s*[\w._-]+/gi,
      '$1 [REDACTED]',
    )
    .replace(/(password|passwd|pwd)\s*[:=]\s*["']?[^"'\s,}:]+["']?/gi, '$1 [REDACTED]')
    .replace(/file:\/\/\/[^\s)\]}]+/g, 'file:///[PATH]')
    .replace(/(^|[\s([])\/(?:[^/\s:()[\]]+\/){1,}[^/\s:()[\]]+/gm, '$1[PATH]')
    .replace(/[A-Za-z]:\\(?:[^\\\s]+\\)+[^\\\s)]+/g, '[PATH]');
}

function truncate(value: string, maxCharacters: number): string {
  return [...value].slice(0, maxCharacters).join('');
}
