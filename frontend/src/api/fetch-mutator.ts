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

import { bindLocalisedMessage } from './error-localiser';

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
    bindLocalisedMessage(this, payload.code, payload.details, payload.message);
    this.status = status;
    this.code = payload.code;
    this.requestId = payload.requestId;
    this.details = payload.details;
  }
}

/**
 * Error for a successful-but-unexpected response status. Non-2xx responses already throw an
 * {@link ApiError}; this covers the remaining 2xx shapes a generated client does not model.
 */
export function unexpectedResponseError(operation: string, status: number): ApiError {
  return new ApiError(status, {
    code: 'unexpectedResponse',
    message: `Unexpected ${operation} response status: ${status}`,
  });
}

/** Error for a feature only the desktop host provides; the browser/server adapter cannot offer it. */
export function desktopHostRequiredError(): ApiError {
  return new ApiError(0, {
    code: 'desktopHostRequired',
    message: 'This action is only available in the desktop app.',
  });
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

/** The API base URL requests are resolved against (empty for same-origin). */
export function resolveBaseUrl(): string {
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
    code: 'httpError',
    message: response.statusText || `Request failed with status ${response.status}`,
    details: { status: response.status },
  });
}

/** Sends one request to the API with the session header; failures become `ApiError`s. */
async function send(url: string, options: RequestInit): Promise<Response> {
  const headers = new Headers(options.headers);
  const sessionHeader = sessionHeaderProvider?.();
  if (sessionHeader !== undefined) {
    headers.set(sessionHeader.name, sessionHeader.value);
  }

  let response: Response;
  try {
    response = await fetch(`${resolveBaseUrl()}${url}`, {
      ...options,
      headers,
    });
  } catch (error) {
    // `fetch` rejects with a TypeError when the server cannot be reached at all.
    if (error instanceof TypeError && options.signal?.aborted !== true) {
      throw new ApiError(0, { code: 'networkUnavailable', message: error.message });
    }
    throw error;
  }

  if (!response.ok) {
    if (response.status === 401) {
      unauthorizedHandler?.();
    }
    throw await toApiError(response);
  }
  return response;
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
  const response = await send(url, { ...options, headers });
  const data = await readBody(response);
  return { status: response.status, data, headers: response.headers } as T;
}

/**
 * POSTs a JSON body and hands each newline-delimited JSON value of the response to `onItem` as it
 * arrives (task 0321: streamed assistant replies). Errors before the body starts reject like any
 * other request; the stream itself reports its outcome as items.
 */
export async function postNdjson<T>(
  url: string,
  body: unknown,
  onItem: (item: T) => void,
  signal?: AbortSignal,
): Promise<void> {
  const response = await send(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/x-ndjson' },
    body: JSON.stringify(body),
    ...(signal === undefined ? {} : { signal }),
  });
  if (response.body === null) return;
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffered = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffered += value;
    for (let newline = buffered.indexOf('\n'); newline !== -1; newline = buffered.indexOf('\n')) {
      const line = buffered.slice(0, newline).trim();
      buffered = buffered.slice(newline + 1);
      if (line !== '') onItem(JSON.parse(line) as T);
    }
  }
  const rest = buffered.trim();
  if (rest !== '') onItem(JSON.parse(rest) as T);
}
