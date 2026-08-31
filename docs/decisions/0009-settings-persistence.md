# 0009 Settings persistence

Status: accepted

## Context
User settings and workspace layout (panes, sort order, theme, column widths) must persist across
sessions in both the browser and Tauri hosts, which have different native storage mechanisms (spec
§30 settings and workspace persistence; §3 rule 10 explicit capabilities).

## Decision
`fm-settings` defines the settings schema and persistence trait on the backend side; each host
persists through its own natural mechanism behind that trait — a settings file on disk for the
Tauri/native host, and a backend-served settings endpoint (backed by the same `fm-settings` crate)
for the browser host, rather than the frontend reading/writing browser storage (e.g.
`localStorage`) directly. This keeps rule 7 (backend owns authoritative state) true for settings
just as it is for filesystem state.

## Alternatives
- **`localStorage`/`IndexedDB` in the frontend**: rejected — violates rule 7 (backend must own
  authoritative state) and rule 1 (frontend must not reach into host-specific browser APIs
  directly); also wouldn't sync between browser and Tauri hosts for the same user.
- **Cloud-synced settings service**: rejected as a first cut — no such backend exists yet and it's
  not required by the spec; would be a speculative abstraction (spec §35).
- **Per-host settings format with no shared schema**: rejected — would require the frontend to
  special-case settings shape per host, reintroducing the transport leakage rule 1 forbids.

## Consequences
- Settings changes go through the same `FileManagerClient`-mediated path as other backend state,
  keeping browser/Tauri parity (rule 9) rather than diverging into host-specific storage code.
- The Tauri host still needs a concrete on-disk format (`fm-settings` persistence trait
  implementation) distinct from the browser host's server-side store, but both expose the same
  schema to the frontend.
- Settings schema changes are versioned in `fm-settings`, so both persistence backends evolve
  together instead of drifting.

## Revisit conditions
Revisit if genuine cross-device settings sync becomes a requirement (would need a real backend
service, not just per-host persistence), or if the on-disk format for the Tauri host needs to
support external editing/migration tooling the current schema doesn't provide for.
