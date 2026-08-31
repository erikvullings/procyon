/**
 * Session-token storage for the HTTP runtime (task 0064 frontend follow-up).
 *
 * `fm-server` requires a session token on every `/api/v1` route except
 * health and docs. The token is kept in `sessionStorage` — never
 * `localStorage` — per `docs/architecture/security.md`, so it doesn't
 * outlive the browser tab/session it was entered in.
 */

const STORAGE_KEY = 'fm.sessionToken';

let memoryToken: string | undefined;

function readStorage(): Storage | undefined {
  try {
    return globalThis.sessionStorage;
  } catch {
    // Some embedding contexts (e.g. sandboxed iframes) throw on access.
    return undefined;
  }
}

/** Returns the stored session token, if one has been entered this session. */
export function getSessionToken(): string | undefined {
  if (memoryToken !== undefined) return memoryToken;
  const stored = readStorage()?.getItem(STORAGE_KEY);
  return stored === null || stored === undefined || stored.length === 0 ? undefined : stored;
}

/** Stores (or clears, when `token` is `undefined` or empty) the session token. */
export function setSessionToken(token: string | undefined): void {
  const normalized = token === undefined || token.length === 0 ? undefined : token;
  memoryToken = normalized;
  const storage = readStorage();
  if (storage === undefined) return;
  if (normalized === undefined) {
    storage.removeItem(STORAGE_KEY);
  } else {
    storage.setItem(STORAGE_KEY, normalized);
  }
}

/** Test-only reset so token state doesn't leak between test files. */
export function resetSessionTokenForTests(): void {
  memoryToken = undefined;
  readStorage()?.removeItem(STORAGE_KEY);
}
