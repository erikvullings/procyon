# 0002 Axum REST plus SSE

Status: accepted

## Context
The browser host needs a request/response API for commands and queries, and a push channel for
filesystem change notifications, operation progress, and other backend-originated events (spec
§3, §14 event model).

## Decision
The Axum server (`apps/fm-server`) exposes a REST API described by an OpenAPI document (see ADR
[0003](0003-openapi-source-of-truth-and-generated-client.md)) for commands and queries, plus a
single Server-Sent Events endpoint that streams the backend event bus to connected browser
clients. Handlers stay thin: they parse the request, call into `fm-application` services, and map
the result to a transport DTO (rule 2, spec §3).

## Alternatives
- **WebSockets** instead of SSE: rejected for the browser transport — bidirectional messaging isn't
  needed (commands already go over REST), and SSE gives auto-reconnect and plain-HTTP
  infrastructure compatibility for free.
- **Long polling**: rejected — higher latency and more server-side connection bookkeeping than SSE
  for no benefit here.
- **GraphQL** instead of REST: rejected — the operation set is a fixed, well-known set of file
  operations and queries, not an ad-hoc query surface; REST plus OpenAPI keeps the generated client
  simple.

## Consequences
- Every event type the backend can emit must be representable as a JSON SSE message, and the
  frontend event-stream abstraction (`event-stream.ts`) must accept both `sse-event-stream.ts` and
  `tauri-event-stream.ts` implementations behind the same interface (ADR 0001).
- SSE is one-directional; any future need for server-initiated request/response beyond events would
  require a separate mechanism.
- Because handlers stay thin, most backend logic and its tests live in `fm-application`, not in
  Axum route modules — route-level tests only need to check status codes and DTO mapping.

## Revisit conditions
Revisit if the browser host needs bidirectional low-latency messaging (e.g. collaborative editing)
that SSE cannot express, or if the event volume grows enough that per-client SSE fan-out becomes a
scaling bottleneck.
