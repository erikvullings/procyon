/**
 * Custom Orval Fetch mutator (task 0010, spec §2.3 and §8).
 *
 * Every generated request function delegates its entire HTTP call to
 * {@link fetchMutator}, so this module owns: base URL resolution, JSON
 * request/response handling, `AbortSignal` pass-through, an optional
 * auth/session header, and mapping non-2xx responses to {@link ApiError}.
 *
 * Kept free of Mithril imports so it stays testable in isolation.
 */

/** Machine-readable error details, per the structured error shape in spec §8. */
export interface ApiErrorDetails {
  readonly [key: string]: unknown;
}

interface ApiErrorPayload {
  code: string;
  message: string;
  requestId?: string;
  details?: ApiErrorDetails;
}

/** Typed error raised for every non-2xx response; raw `Response` objects never escape this module. */
export class ApiError extends Error {
  readonly code: string;
  readonly requestId: string | undefined;
  readonly details: ApiErrorDetails | undefined;
  readonly status: number;

  constructor(status: number, payload: ApiErrorPayload) {
    super(payload.message);
    this.name = 'ApiError';
    this.status = status;
    this.code = payload.code;
    this.requestId = payload.requestId;
    this.details = payload.details;
  }
}

/** An additional header to attach to every request, e.g. for auth/session tokens. */
export interface SessionHeader {
  readonly name: string;
  readonly value: string;
}

type SessionHeaderProvider = () => SessionHeader | undefined;

let sessionHeaderProvider: SessionHeaderProvider | undefined;

/** Lets bootstrap code attach an optional auth/session header to every request (spec §2.3). */
export function setSessionHeaderProvider(provider: SessionHeaderProvider | undefined): void {
  sessionHeaderProvider = provider;
}

type UnauthorizedHandler = () => void;

let unauthorizedHandler: UnauthorizedHandler | undefined;

/**
 * Lets bootstrap code react to a `401` response (e.g. clear a stored session
 * token and re-prompt) before {@link ApiError} propagates to the caller
 * (task 0064 frontend follow-up).
 */
export function setUnauthorizedHandler(handler: UnauthorizedHandler | undefined): void {
  unauthorizedHandler = handler;
}

let baseUrlOverride: string | undefined;

/** Overrides the resolved base URL; used by tests. Production reads `VITE_API_BASE_URL`. */
export function setBaseUrlOverride(url: string | undefined): void {
  baseUrlOverride = url;
}

function resolveBaseUrl(): string {
  if (baseUrlOverride !== undefined) {
    return baseUrlOverride;
  }
  const configured = import.meta.env.VITE_API_BASE_URL;
  return configured !== undefined && configured.length > 0 ? configured : '';
}

async function readBody(response: Response): Promise<unknown> {
  if (response.status === 204 || response.status === 205 || response.status === 304) {
    return undefined;
  }
  const contentType = response.headers.get('content-type') ?? '';
  if (contentType.includes('image/svg+xml')) {
    return response.text();
  }
  if (contentType.includes('image/')) {
    return response.blob();
  }
  const text = await response.text();
  if (text.length === 0) {
    return undefined;
  }
  if (!contentType.includes('application/json')) {
    return text;
  }
  return JSON.parse(text);
}

function isApiErrorPayload(value: unknown): value is ApiErrorPayload {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.code === 'string' && typeof candidate.message === 'string';
}

async function toApiError(response: Response): Promise<ApiError> {
  const body = await readBody(response).catch(() => undefined);
  if (isApiErrorPayload(body)) {
    return new ApiError(response.status, body);
  }
  return new ApiError(response.status, {
    code: 'unknownError',
    message: response.statusText || `Request failed with status ${response.status}`,
  });
}

/**
 * Orval fetch mutator: performs the request and returns `{ status, data,
 * headers }`, matching the generated fetch client's response shape.
 */
export async function fetchMutator<T>(url: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  if (!headers.has('accept')) {
    headers.set('accept', 'application/json');
  }

  const sessionHeader = sessionHeaderProvider?.();
  if (sessionHeader !== undefined) {
    headers.set(sessionHeader.name, sessionHeader.value);
  }

  const response = await fetch(`${resolveBaseUrl()}${url}`, {
    ...options,
    headers,
  });

  if (!response.ok) {
    if (response.status === 401) {
      unauthorizedHandler?.();
    }
    throw await toApiError(response);
  }

  const data = await readBody(response);
  return { status: response.status, data, headers: response.headers } as T;
}
