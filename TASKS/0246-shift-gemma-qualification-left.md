# 0246 Shift Gemma qualification failures left

Status: open
Priority: high
Subsystem: ci
Depends on: 0244

## Context

Retro on the TASKS/0244 release push (PR #115, 10–14 Oct 2026). Releasing the experimental Gemma
package took days of multi-hour cycles. 9 of 10 failed runs on `gemma-release-integration` were the
signed-candidate `release-semantic-components.yml` installed jobs, and most failures were defects
in the tests or CI config, not in Procyon:

- Windows host test failed to compile: `ResolveRagCitationRequestDto` import gated `#[cfg(unix)]`.
- Worker test demanded one globally monotonic `completed_units`; progress legitimately resets
  between ingestion stages.
- Host test counted top-level search rows as page citations; a PDF is one file-primary row with
  per-page citations in `semantic.evidence`.
- Linux arm64 missing `xdg-utils`; Biome formatting reached CI despite the pre-commit hook.
- Windows `\\?\` verbatim paths from `std::fs::canonicalize` rejected by the Zvec SDK.

Root cause: normal PR CI (`ci.yml`) builds without `gemma-native`/`experimental-pdf-render`, and the
installed tests are `#[ignore]`d, so they first compile and run inside the expensive signed
four-target candidate. Brittle assertions then turn every small mistake into another full cycle.

Decision (user, 14 Oct 2026): implement after the current Gemma release passes. If the current
release attempt fails again, implement this BEFORE starting another candidate round.

## Acceptance Criteria

- **Dress rehearsal in PR CI**: on macOS arm64, Linux x64, Linux arm64 and Windows x64,
  `cargo clippy --all-targets` and `cargo test --no-run` succeed for `fm-semantic-worker` and
  `fm-application` with `gemma-native` and `experimental-pdf-render`; the ignored installed
  worker/host tests run against an unsigned locally built package on a ≤2-page synthetic fixture.
  `release-semantic-components.yml` candidate dispatch documents (or enforces) a green rehearsal
  on the exact SHA first.
- **Outcome assertions**: installed/host semantic tests assert observable outcomes (every page has
  a citation, progress reaches completion, current content after replace/delete) through shared
  helpers, e.g. `pdf_page_citations(results)`, not internal counters or row counts. A judgement
  rule for this is added to `CODING_STANDARDS.md` for the reviewer.
- **Windows paths**: `clippy.toml` `disallowed-methods` bans `std::fs::canonicalize` (and
  `Path::canonicalize`) in favour of `dunce::canonicalize`; existing call sites migrated or
  explicitly allowed with a reason.
- **Workflow change-detector tests**: the workflow-text assertions (38 in
  `scripts/desktop-packaging.test.mjs`, 13 in `scripts/zvec-runtime-qualification.test.mjs`) are
  reduced to genuine invariants (no publish without approval inputs, exact-SHA binding, required
  targets) plus `actionlint`; text-matching of incidental steps (e.g. apt package lists) is removed.
- **Agent process**: one line in `CLAUDE.md`: commit through the hooks (never `--no-verify`) and
  batch fixes into one push per CI cycle (three CI runs were cancelled by pushes minutes apart).

## Implementation Notes

- Workflows: `.github/workflows/ci.yml`, `.github/workflows/release-semantic-components.yml`
  ("Build existing offline media smoke" / "Build installed application PDF smoke" steps show the
  exact feature/test invocations to mirror).
- Tests: `crates/fm-semantic-worker/tests/packaged_production.rs`,
  `crates/fm-application/tests/semantic_indexing.rs` (2,639 lines).
- Hooks: `scripts/pre-commit.mjs` already covers `scripts/**/*.mjs` with Biome.
- Keep rehearsal runtime small; CPU Gemma image embedding measured 80–126 s/page on shared runners.

## Agent Notes

- 2026-10-14: Created from retro; evidence gathered from `gh run list` failures and branch commit
  history (`f771a25f`, `1e620684`, `190f3ac3`, `ce22745a`).
