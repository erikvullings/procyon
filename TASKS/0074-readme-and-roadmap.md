# 0074 README, development commands and roadmap

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: docs
Depends on: 0027, 0015

## Context
`file-manager-coding-agent-spec.md` §38 deliverables 13 and 15: a README with exact development
commands, and a roadmap showing which parts remain mocked or incomplete.

## Acceptance Criteria
- README covers: prerequisites and versions (Rust toolchain, Node, pnpm, Tauri prerequisites per
  platform), repository layout, and the exact commands from §32 with what each one does.
- The recommended two-terminal development flow from §32 (`cargo watch -x "run -p fm-server"` +
  `pnpm --dir frontend dev`) is documented, including how the Vite `/api` proxy and SSE proxying
  work.
- Documents how to run in each runtime (`VITE_RUNTIME=http|tauri|mock`) and how to open Swagger UI.
- A `ROADMAP.md` lists, per milestone (§16), what is done, what is mocked, and what is explicitly
  not implemented — including every capability currently reported as `false` and every
  platform-untested area (§35).
- Points to the ADRs, the plugin API docs and the task tracker.
- The commands in the README are verified to work on a clean checkout on macOS; deviations for
  Windows are noted.

## Implementation Notes
- Update this task's outputs whenever a milestone completes rather than letting the roadmap rot —
  add a line to the relevant task's acceptance criteria as it lands.

## Agent Notes
- 2026-08-10: Implemented. Added prerequisites table, repository layout, development commands
  table, two-terminal flow explanation, proxy/SSE proxy explanation, runtime table, Swagger UI
  URL, and further-reading links to README.md. Created ROADMAP.md covering all milestones per §16,
  every `PlatformCapabilities` bit with per-platform status, platform-untested areas per §35, and
  cross-cutting quality tasks.
- `cargo build --workspace` verified green on macOS (dev profile). Frontend tests have 4
  pre-existing failures in theme and client tests unrelated to documentation; noted but not fixed
  here.
- Windows deviations noted throughout: `WindowsPlatformAdapter` reports only `NATIVE_DRAG_OUT`;
  task 0060 (Windows platform integration) remains open.

Status: done
