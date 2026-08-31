/// <reference types="vite/client" />

/**
 * Build-time configuration injected by Vite.
 *
 * `VITE_RUNTIME` selects the transport the frontend talks to. It is validated
 * by `resolveRuntimeKind` and consumed by the client factory in task 0011.
 *
 * `VITE_API_BASE_URL` overrides the origin the generated API client requests
 * against (task 0010). Left unset, requests are relative, which is what the
 * Vite dev proxy and same-origin Axum static hosting both expect.
 */
interface ImportMetaEnv {
  readonly VITE_RUNTIME?: string;
  readonly VITE_API_BASE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
