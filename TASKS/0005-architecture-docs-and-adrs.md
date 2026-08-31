# 0005 Architecture documentation and initial ADRs

Status: done
Priority: medium
Owner: unassigned
Agent: claude
Area: docs
Depends on: 0001, 0002

## Context
`file-manager-coding-agent-spec.md` §34 requires ten short architecture decision records, and §33
step 1 requires architecture documentation as part of the bootstrap.

## Acceptance Criteria
- `docs/architecture/overview.md` describes the layering diagram from §3 and restates the ten
  mandatory rules.
- `docs/decisions/` contains one ADR per item in §34:
  1. browser + Tauri dual-host architecture;
  2. Axum REST plus SSE;
  3. OpenAPI source of truth and generated TypeScript client;
  4. VFS provider abstraction;
  5. operation scheduler and conflict handling;
  6. plugin runtime selection;
  7. frontend state management;
  8. virtualized table implementation;
  9. settings persistence;
  10. native platform adapters.
- Every ADR includes: context, decision, alternatives, consequences, revisit conditions.
- ADRs are numbered `0001-*.md` within `docs/decisions/` and marked `Status: accepted|proposed`.
- `docs/plugin-api/` and `docs/screenshots/` directories exist with a placeholder README.

## Implementation Notes
- The crate layering is already enforced in code by `crates/fm-test-support/src/architecture.rs`
  (task 0001). `docs/architecture/overview.md` should describe and link to it rather than restating
  the layer map, so the prose and the test cannot drift apart.
- ADRs are short (roughly one page). They record intent, not implementation detail.
- ADR 7 (frontend state) should record the decision to use a small explicit state model
  (Meiosis-style patch updates) rather than a generic state framework (§13, §35).
- Later tasks that contradict an ADR must supersede it rather than edit history.

## Agent Notes
- 2026-07-29 claude: TDD-first, per scripts/ci-workflow.test.mjs's precedent from task 0004:
  wrote `scripts/architecture-docs.test.mjs` before any docs existed (14 tests, all initially
  failing) asserting every acceptance-criteria line literally — the ten mandatory rules verbatim
  in overview.md, exactly 10 ADR files numbered `0001-*.md`..`0010-*.md` each with a `Status:
  accepted|proposed` line and `## Context`/`## Decision`/`## Alternatives`/`## Consequences`/
  `## Revisit conditions` sections, ADR 0007 mentioning both "Meiosis" and "explicit state model",
  and non-empty placeholder READMEs under `docs/plugin-api/` and `docs/screenshots/`. Then wrote the
  docs to make all 14 pass.
- 2026-07-29 claude: `docs/architecture/overview.md` restates the §3 layering diagram and all ten
  mandatory rules verbatim, but explicitly defers to
  `crates/fm-test-support/src/architecture.rs` for the crate-level layer map per the Implementation
  Notes, and calls out which of the ten rules that file actually enforces mechanically (rule 4 and
  the anyhow/thiserror split) versus which are reviewed by hand.
- 2026-07-29 claude: Ten ADRs added under `docs/decisions/`, one per §34 item, each ~1 page with all
  five required sections and `Status: accepted` (nothing here was left merely proposed — every
  decision reflects a choice already made in tasks 0001-0004 or implied by the spec). ADR 0007
  records the Meiosis-style explicit state model decision, cross-referencing AGENTS.md's ban on
  generic state frameworks. ADR 0010 explicitly notes no `fm-platform-linux` crate is added
  speculatively (§35) since no Linux native-integration requirement exists yet.
- 2026-07-29 claude: `docs/plugin-api/README.md` and `docs/screenshots/README.md` added as short
  placeholders explaining what will eventually live there and linking back to ADR 0006 where
  relevant.
- 2026-07-29 claude: Verified — `node --test scripts/architecture-docs.test.mjs`: 14/14 passing.
  Full suite via `pnpm run test`: `cargo test --workspace` unaffected (docs-only change, 0 Rust
  tests broken), frontend Vitest 14/14 unchanged, `node --test scripts/*.test.mjs` 28/28 (14 new +
  14 pre-existing from tasks 0003/0004). `pnpm run lint:frontend` (biome) clean after an autofix
  pass on the new test file's import order/formatting; the one remaining info-level `vite.config.ts`
  suggestion is pre-existing from task 0003, unrelated to this change. No Rust files were touched
  so `cargo fmt`/`clippy` were not re-run beyond the full `pnpm run test` pass above.
- 2026-07-29 claude: Known gaps: no ADR is marked `Status: proposed` since none of the ten decisions
  is genuinely undecided at this point in the project; a future task that wants to revisit one
  should supersede it explicitly (per the Implementation Notes) rather than edit it in place.
