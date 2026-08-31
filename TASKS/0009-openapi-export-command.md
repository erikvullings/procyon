# 0009 Deterministic OpenAPI export command

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: backend
Depends on: 0008

## Context
`file-manager-coding-agent-spec.md` §9: the OpenAPI document must be exportable without starting the
web server, and the output must be deterministic so CI does not produce ordering-only diffs.

## Acceptance Criteria
- `cargo run -p fm-server -- export-openapi frontend/openapi/openapi.json` writes the document and
  exits 0 without binding a port.
- Output is byte-for-byte stable across runs: sorted keys, stable operation id order, fixed
  indentation, trailing newline.
- Re-running the command twice produces no diff; a Rust test asserts this.
- `frontend/openapi/openapi.json` is checked into git.
- `scripts/export-openapi.sh` and the root `api:export` script call this command.
- A test asserts every route registered on the Axum router appears in the exported document.

## Implementation Notes
- Use a small CLI arg parser (`clap`) in `apps/fm-server`; keep the server and export paths sharing
  one `ApiDoc` definition.
- Serialize through `serde_json::Value` with a `BTreeMap`-backed object representation if
  `utoipa`'s ordering is not already deterministic.

## Agent Notes
- 2026-07-29 claude: Implemented the `export-openapi` CLI subcommand.
  - `apps/fm-server`: `Cli` gained `#[command(subcommand)] command: Option<Command>`
    with one variant, `ExportOpenapi { path: PathBuf }`. `main` uses
    `std::mem::take(&mut cli.command)` to pull the subcommand out without a
    partial-move conflict with the later `cli.into::<ServerConfig>()`, then
    returns immediately after writing the document — no listener is ever
    created on that path.
  - `fm_server::lib.rs`: factored the route registration (previously inline
    in `build_router`) into a private `api_router() -> OpenApiRouter<AppState>`
    and a new `pub fn openapi_document() -> utoipa::openapi::OpenApi` built
    from it via `into_openapi()`. `build_router` now calls the same
    `api_router()` and applies `.with_state(...)` itself, so the served
    `/api/v1/openapi.json` and the exported document can never drift apart
    (one source of truth, per the Implementation Notes). Building the
    document this way needs no `FileManagerService`/`AppState` value at all,
    only the `AppState` type parameter, so exporting never binds a port or
    constructs application state.
  - New `apps/fm-server/src/openapi_export.rs`: `canonical_json()` converts
    the document through `serde_json::to_value` (this workspace never enables
    serde_json's `preserve_order` feature, so `Value`'s object map is
    `BTreeMap`-backed, giving alphabetically sorted keys independent of any
    `HashMap` iteration order) and serializes with
    `serde_json::ser::PrettyFormatter::with_indent(b"  ")`, appending exactly
    one trailing `\n`. `write_to_file(path)` creates parent directories and
    writes the bytes; it never binds a socket.
  - `scripts/export-openapi.sh` and the root `api:export` script already
    called `cargo run -p fm-server -- export-openapi <path>` (written ahead
    of this task); no changes were needed there. Ran
    `bash scripts/export-openapi.sh` twice and diffed the output to confirm
    determinism, then committed the generated `frontend/openapi/openapi.json`.
  - Fixed a pre-existing stale test in `scripts/scripts.test.mjs`
    (`scripts/export-openapi.sh fails clearly until task 0009 lands`, which
    was already failing on `main` before this task — clap's own "unexpected
    argument" error was never going to contain the string it asserted on) —
    replaced it with a test that runs the script twice against a temp output
    path and asserts the bytes are identical.
  - Tests added in `apps/fm-server/tests/openapi_export.rs` (5 new tests,
    verified via `cargo test -p fm-server --test openapi_export`):
    `canonical_json_is_byte_for_byte_stable_across_runs` (re-running produces
    no diff), `canonical_json_has_sorted_keys_fixed_indentation_and_trailing_newline`,
    `write_to_file_creates_parent_directories_and_matches_canonical_json`,
    `exported_document_matches_the_document_served_by_the_running_router`
    (spawns the real Axum host on an ephemeral port and asserts the served
    `/api/v1/openapi.json` body is exactly equal, as a `serde_json::Value`, to
    `canonical_json()` — the strongest form of "every route registered on the
    router appears in the exported document", since it proves full document
    equivalence rather than a hand-maintained path list), and
    `export_openapi_subcommand_exits_zero_without_binding_a_port` (spawns the
    compiled `fm-server` binary via `CARGO_BIN_EXE_fm-server` with an
    explicit `--port` pointed at a port the test itself holds open, proving
    the export path never tries to bind it).
  - Verified: `cargo test -p fm-server` — 10/10 (5 existing + 5 new) in
    `tests/openapi_export.rs` and `tests/integration.rs` combined. Also ran
    `cargo test --workspace` (all green, including the `fm-test-support`
    architecture fitness tests) and `node --test scripts/*.test.mjs` (28/28,
    including the fixed export-openapi script test).
    `cargo fmt --all --check` and
    `cargo clippy -p fm-server --all-targets -- -D warnings` both clean; the
    new `pub fn openapi_document`/`pub mod openapi_export` items are
    documented so `missing_docs` stays clean too.
  - Known gaps: none against this task's literal acceptance criteria. A
    pre-existing, unrelated uncommitted formatting change to the root
    `Cargo.toml` (from a prior `rustup update`/toolchain bump, not touched by
    this task) was left as-is and is not part of this commit.
