# 0177 Semantic worker and versioned IPC contract

Status: open
Priority: high
Subsystem: backend, desktop, architecture
Depends on: 0176

## Context

The optional semantic subsystem needs process isolation, independent component updates, one owner
for Zvec's writer, and memory reclamation when embedding inference is idle. A normal Lua plugin
cannot host this workload, while embedding it directly into every Procyon process would create
writer contention and duplicate model memory.

Build the pure-Rust semantic worker boundary before choosing converters or a concrete embedding
runtime. Procyon must retain all filesystem authority and stream provider-neutral content to the
worker; the worker must not receive paths it can open independently.

## Acceptance Criteria

- A Rust worker binary and narrow host-side capability interface exist without making semantic
  components a startup dependency of `fm-server`, Tauri, or the frontend.
- A versioned protobuf contract supports capability/version negotiation, authenticated sessions,
  health, cancellation, bounded file-content streaming, ingestion jobs, query requests, result
  streams, progress/events, and graceful shutdown.
- Transport uses Unix-domain sockets on macOS/Linux and named pipes on Windows; it never exposes a
  TCP listener. Socket/pipe ownership and an ephemeral session secret prevent another local user or
  process from issuing requests accidentally.
- One per-user worker is discovered or started on demand and safely shared by multiple Procyon
  windows/processes. Concurrent startup elects one owner without corrupting state.
- The protocol applies explicit message, stream, concurrency, and time limits; cancellation
  propagates promptly in both directions and a disconnected client cannot leave unbounded work.
- The worker accepts opaque document/library/tenant identifiers, structured metadata, and byte
  streams. It receives no arbitrary filesystem API, absolute-path authority, LLM credentials,
  Procyon action registry, or network permission.
- Protocol compatibility distinguishes supported rolling upgrades from required worker updates and
  returns actionable typed errors rather than crashing either process.
- Desktop lifecycle may let the worker finish its current atomic document, persist queued state,
  and exit after the last Procyon process closes. Persistent login-service operation is out of
  scope.
- `fm-server` can connect to an administrator-provisioned worker through the same host interface;
  mock mode has a deterministic fake implementation.
- Tests cover negotiation, authentication rejection, size limits, cancellation, concurrent
  startup, client disconnect, incompatible versions, idle shutdown, and worker crash/restart.

## Implementation Notes

- Add dedicated worker/protocol crates rather than putting process management in
  `FileManagerService`. Add an application capability service and keep the facade thin.
- Generate Rust types from the schema on both sides; do not expose internal domain structs as an
  accidental cross-process ABI.
- Keep requests tenant/library scoped from the first protocol version even though desktop starts
  with one local library. Server-side filtering must happen before result data leaves the worker.
- Do not add Zvec or an embedding model in this task. Use a fake engine to prove lifecycle and IPC.

## Agent Notes

- 2026-09-04: Split from 0176. This task owns only the process and protocol boundary; component
  download belongs to 0178 and storage/retrieval belongs to 0181.
