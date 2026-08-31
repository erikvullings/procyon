# 0001 Cargo workspace skeleton and crate stubs

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: backend
Depends on: none

## Context
Bootstrap the Rust side of the monorepo described in `file-manager-coding-agent-spec.md` §4
(repository structure) and §33 step 1. Nothing exists yet except the spec.

Crate dependencies must stay directional and acyclic, per the preferred direction in §4:
domain → events / vfs traits / plugin API → operations / providers / metadata / search →
application services → Axum and Tauri hosts.

## Acceptance Criteria
- Root `Cargo.toml` defines a workspace with resolver 2 and shared `[workspace.dependencies]`
  (tokio, serde, tracing, thiserror, utoipa, uuid, chrono, async-trait, ...).
- The crates from §4 exist under `crates/` as compiling stubs with a `lib.rs` and a doc comment
  stating the crate's responsibility: `fm-domain`, `fm-application`, `fm-events`, `fm-operations`,
  `fm-vfs`, `fm-vfs-local`, `fm-search`, `fm-metadata`, `fm-archive`, `fm-settings`,
  `fm-plugin-api`, `fm-plugin-runtime`, `fm-platform`, `fm-transport-dto`, `fm-test-support`.
- `crates/fm-platform-macos` and `crates/fm-platform-windows` exist and are only built on their
  target platform (target-specific workspace members or cfg-gated deps).
- `apps/fm-server` and `apps/fm-cli` exist as binary stubs.
- `cargo build --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `rust-toolchain.toml` pins a stable toolchain; `rustfmt.toml` and `clippy.toml` are checked in.

## Implementation Notes
- Do not create `apps/fm-desktop` here; that is task 0015.
- Empty crates that are not needed until later milestones (`fm-archive`, `fm-search`) may be
  placeholders, but must not accumulate speculative abstractions (§35).
- `anyhow` only in `apps/*` binaries; libraries use `thiserror` (§2.2).

## Agent Notes
- 2026-07-29 claude: Implemented. Root `Cargo.toml` (edition 2024, `[workspace.package]`, shared
  `[workspace.dependencies]`, `[workspace.lints]`), 17 library crates under `crates/`, two binary
  stubs under `apps/`, plus `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml` and a minimal
  `.gitignore` (task 0003 extends the latter). Every stub is a documented empty `lib.rs` naming the
  task that fills it in, with no dependencies — deliberately no speculative abstractions.
- 2026-07-29 claude: The §4 layering is enforced mechanically rather than only documented.
  `crates/fm-test-support/src/architecture.rs` assigns every crate a layer in `CRATE_LAYERS` and
  `crates/fm-test-support/tests/workspace_architecture.rs` checks the real `cargo metadata` graph
  against it. It also rejects `axum`/`hyper`/`tauri`/`tower-http`/`utoipa-axum`/`utoipa-swagger-ui`
  in any non-`apps/` crate (§3 rule 4) and `anyhow` outside executables (§2.2). Dev-dependencies are
  excluded, so test-only edges may point upwards. **Adding a new crate without adding it to
  `CRATE_LAYERS` fails the test** — that is intentional, layer placement should be a decision.
- 2026-07-29 claude: Verified — `cargo test --workspace`: 8 unit tests in `architecture.rs` plus 1
  integration test in `workspace_architecture.rs`, all passing (re-run via
  `cargo test -p fm-test-support`). `cargo build --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings` and `cargo fmt --all --check` are clean.
  Two checks that the test suite alone would not have caught, both done by temporary edit and
  reverted: (a) injecting `anyhow`, `axum` and a `fm-application` dependency into `fm-domain`
  produced all three expected violations, proving the fitness test is not vacuous against real
  metadata; (b) adding all 16 `[workspace.dependencies]` entries to `fm-cli` and running
  `cargo check` proved every version and feature name resolves — cargo never resolves *unused*
  workspace dependencies, so a typo there would otherwise have surfaced only in task 0007.
- 2026-07-29 claude: **Deviation from the acceptance criteria** — the workspace uses
  `resolver = "3"`, not `resolver = "2"`. Edition 2024 makes resolver 3 the default and it is
  MSRV-aware (`rust-version = "1.97"` is set); pinning resolver 2 would be a downgrade with no
  benefit. Flagged rather than silently substituted — revert if you disagree.
- 2026-07-29 claude: Known gaps and environment notes for the next agent:
  - `rust-toolchain.toml` pins `1.97.1`, but on this machine Homebrew's Rust in `/opt/homebrew/bin`
    shadows rustup on `PATH`, so the pin is ignored locally (rustup's own stable here is 1.95.0).
    All verification above ran on Homebrew cargo 1.97.1. CI (task 0004) will honour the pin.
  - The repository was not actually a git repository at the start of the session; ran `git init`.
  - `fm-platform-windows` is `#![cfg(target_os = "windows")]` and compiles to nothing on macOS, so
    the Windows build is untested here; the same applies to `fm-platform-macos` elsewhere (§35).
  - `unsafe_code = "deny"` is set workspace-wide. Tasks 0059/0060 will need a per-crate `[lints]`
    table in the platform crates once they call native APIs.
  - No `README.md` or `AGENTS.md` was written: those belong to tasks 0074 and 0003 respectively.
    `Cargo.lock` is committed (the workspace produces binaries).
