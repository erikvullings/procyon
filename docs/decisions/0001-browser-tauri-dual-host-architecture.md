# 0001 Browser + Tauri dual-host architecture

Status: accepted

## Context
The application must run both as a plain web app served over HTTP and as a native desktop app
packaged with Tauri, sharing one frontend codebase (spec §3). The two hosts differ in how they
reach the backend: the browser only has `fetch`/`EventSource` against an Axum server, while Tauri
can also invoke Rust commands and receive events in-process without HTTP.

## Decision
The Mithril frontend depends only on a `FileManagerClient` interface (spec §3, §11) that abstracts
requests and event streams. Two adapters implement it: an HTTP adapter (generated REST client plus
SSE) and a Tauri adapter (invoke commands plus Tauri channels/events). A `create-client.ts` factory
selects the adapter at startup based on the runtime environment. No frontend component, feature or
state module may import `fetch`, `EventSource`, or `@tauri-apps/api` directly (rule 1, spec §3).

## Alternatives
- **Two separate frontends** (one per host): rejected — doubles UI maintenance and risks behaviour
  drift between hosts.
- **Conditional code paths inside components** (`if (isTauri) ... else ...`): rejected — leaks
  transport concerns into UI code and violates rule 1; also makes components untestable without a
  real transport.
- **Tauri-only, drop the browser target**: rejected — a pure web deployment (no install step, works
  on Linux/mobile browsers) is a stated goal.

## Consequences
- Every new capability needs an equivalent implementation in both adapters before it can ship,
  which is slower than adding it to a single transport.
- The `FileManagerClient` interface becomes the single contract both hosts must honour; changing it
  is a two-adapter change plus regenerating the OpenAPI-derived types where relevant.
- A mock adapter (`mock-file-manager-client.ts`) becomes a natural third implementation for tests
  and Storybook-style development, since the interface is small and side-effect-free to fake.

## Revisit conditions
Revisit if a future host (e.g. mobile) cannot be expressed as a third `FileManagerClient` adapter,
or if maintaining behavioural parity between the HTTP and Tauri adapters (rule 9) proves
consistently more expensive than the value of a shared frontend.
