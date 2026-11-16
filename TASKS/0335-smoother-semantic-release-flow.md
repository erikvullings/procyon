# 0335 Smoother semantic and desktop release flow

Status: in_progress
Priority: high
Subsystem: release
Depends on: none

## Context
Since EmbeddingGemma 2 shipped, every desktop beta is gated on the signed Gemma candidate being
source-identical to the release commit (`scripts/check-desktop-gemma-release.mjs`,
`verifySemanticWorkerSource`). That check diffs the whole transitive workspace closure of
`fm-semantic-worker` (conversion, docling, components, ...) plus `Cargo.lock` closure and
`rust-toolchain.toml`. Any such change after the last signed candidate blocks the desktop release
until a four-target semantic qualification run, a manifest commit
(`docs/evaluations/semantic-gemma-experimental-v1.json`), and a new immutable `semantic-v*` tag
are produced. The model is fixed; what changes is the worker code around it. The v0.7.0-15 attempt
failed this way (candidate `4553da27` was 446 commits old), after two semantic runs had already
failed on a real bug (`open_upstream` dropped its staging directory; fixed in `b1441fab`).

The release owner has stated: Gemma stays experimental and needs no fresh approval decision; the
approval step is mechanical bookkeeping.

## Acceptance Criteria
- A written design (in Agent Notes) comparing: (1) decoupling worker releases from desktop
  releases by pinning a signed worker version; (2) narrowing the gate to inputs that change the
  built worker (exclude tests/docs; consider splitting `fm-semantic-conversion` out of the worker
  closure); (3) qualifying on merge to main when the closure changes so a recent signed candidate
  always exists; (4) automating the manifest/approval commit. With a recommendation.
- Implement the changes that do not weaken what the gate guarantees (at least 3 and 4, plus any
  part of 2 that is provably equivalent), with tests for the gate script.
- Changes that alter the guarantee (notably 1) are proposed, not implemented, pending the release
  owner's sign-off.
- Pre-flight command (script) that tells before tagging whether a desktop release would pass the
  gates, and why not.
- Release process section in `CLAUDE.md` updated to match.

## Implementation Notes
- `scripts/check-desktop-gemma-release.mjs`, `scripts/gemma-risk-acceptance.mjs`
- `.github/workflows/release-semantic-components.yml`, `.github/workflows/release-desktop.yml`
- Never dispatch workflows, push tags or push branches from this task without the user's say-so;
  semantic qualification run 43824195561 (tag `semantic-v0.7.8`) is already in flight on main.
- Related: 0334 (Zvec 0.8.1 re-qualification and the v0.7.0-15 beta).

## Agent Notes
- 2026-11-16: created. Root causes found so far: worker closure gate (above); earlier semantic
  runs failed on the dropped staging directory in `gemma_native.rs` (fixed in `b1441fab`).
- 2026-11-16 (design and implementation):

  **Facts found.** The active desktop Gemma mode is *risk-accepted* (repo variables:
  `SEMANTIC_GEMMA_RISK_ACCEPTED=true`, `SEMANTIC_GEMMA_DESKTOP_EXPERIMENTAL` unset), so the gate
  that bites is `docs/evaluations/semantic-gemma-risk-accepted-v1.json` (candidate `75b5cf97`,
  `semantic-v0.7.7`), not the stale experimental manifest (`4553da27`) named in Context. All three
  modes funnel through `verifySemanticWorkerSource`. `resign-gemma-candidate.mjs` already automated
  most of the risk-accepted bookkeeping, but always dispatched its own qualification and demanded
  `candidate source == HEAD`. On HEAD, `pnpm release:preflight` shows the gate failing on 17 crate
  files plus `Cargo.lock` closure entries (zvec-rust, tokio-util, ...), all genuine build inputs.

  **Options compared.**
  1. *Pin a signed worker version independent of desktop source.* Removes the coupling entirely
     (desktop tags never wait for a worker run). But it changes the guarantee: today a desktop
     build provably ships the host IPC client (`fm-semantic-protocol`, worker client) and a worker
     built from the same reviewed source. A pin needs a replacement proof: a worker/protocol
     version contract (explicit protocol version in the handshake, checked by the gate against the
     pinned worker's declared version, plus a compatibility test per supported pin), a rule for
     when the pin must advance (any `fm-semantic-protocol` change), and the owner accepting that
     worker-side fixes ship only when a new worker is signed. **Proposal only, not implemented;
     needs release-owner sign-off.**
  2. *Narrow the gate.* Already excludes `tests/`, `benches/`, `examples/`, the workspace version
     and member list, and workspace-member versions. What remains over-approximated: lock entries
     reached only through dev-dependencies, optional features or other targets' cfg deps, and
     `#[cfg(test)]` code in `src/`. Provably-equivalent narrowing of those needs
     `cargo metadata`/`cargo tree -e normal,build` at both revisions (two checkouts, toolchain in
     the gate), a different implementation of the gate rather than a path filter; no
     path-only exclusion is provably safe (`include_str!` can pull any file into a build). Not
     implemented; and it would not have helped v0.7.0-15, whose diff is real `src/` changes.
     Splitting `fm-semantic-conversion` out of the worker closure is not equivalent: the worker
     depends on it by design, so splitting just moves unreviewed-by-the-gate code into the host.
  3. *Qualify on merge to main.* **Implemented** (qualification only, never publication): a
     `plan` job in `release-semantic-components.yml` runs on pushes to main touching `crates/**`,
     `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`; it qualifies only when
     `semanticWorkerChanges(manifest.sourceRevision, HEAD)` is non-empty, i.e. exactly when the
     desktop gate would fail (same function, so the trigger cannot be looser than the gate).
     Concurrency group coalesces pushes (one running, one pending, no cancelling). Tag is
     `semantic-v<next>-auto.<run number>` (baked into catalog URLs, unique, valid regex; the next
     manual tag continues the series). Publishing is unreachable on push: `semantic-publish`
     needs `inputs.qualification_run_id`, empty on push; `check-semantic-qualification-workflow`
     now enforces push is main-only, plan-gated and non-publishing.
  4. *Automate the manifest/approval commit.* **Implemented for risk-accepted**: `resign` with
     `--qualification-run <id>` now takes the tag from the candidate (resolveReleaseTag), accepts a
     candidate older than HEAD when the gate's own condition holds (ancestor + no worker input
     changed, replacing `source == HEAD`), and so consumes the automatic run: no dispatch, no
     waiting, one command. Experimental and five-gate modes stay manual: their manifests carry
     reviewed evidence (PDF-vision run, evaluation hash) that is not mechanical.

  **Recommendation.** 3 + 4 now (done), preflight to tell the owner the exact command, and 1 as
  the follow-up only if the remaining wait (environment approval + one resign command after a
  worker change) is still too slow. 2 is not worth it.

  **Flow now.** worker change merges to main -> automatic four-target qualification (needs the
  `desktop-release` environment approval like any qualification run) -> `pnpm release:preflight`
  -> if it says so, `node scripts/resign-gemma-candidate.mjs --approve-risk-accepted
  --qualification-run <id>` (commits manifest, pushes main, publishes the tag) -> bump, tag desktop.

  **Changed.** `scripts/release-preflight.mjs` (+ `pnpm release:preflight`; runs the workflow's own
  gate functions per active mode, plus tag/version/immutability/on-main checks, GitHub variables,
  reuse of a covering qualification run); `scripts/semantic-qualification-plan.mjs`;
  `scripts/semantic-release-tags.mjs`; `resign-gemma-candidate.mjs`; push trigger, plan job and
  risk-accepted run check (accepts a push-on-main run besides workflow_dispatch) in
  `release-semantic-components.yml`; `check-semantic-qualification-workflow.mjs` and
  `assertQualificationDispatchEnvironment` (main push allowed only for the payload qualification,
  never with a run id); `checkDesktopGemmaRelease` takes an optional repository root. Tests:
  `scripts/release-preflight.test.mjs` plus updated workflow/qualification tests.

  **Unverified.** Nothing dispatched or pushed. The workflow changes are statically checked only
  (YAML parse, the proof script, the plan script run locally); the `!cancelled()` plus skipped-need
  conditions and the plan job's checkout have not run on Actions, so the first push to main that
  touches the closure is the real test (watch that the dispatch path of run 43824195561 style
  runs still works: `plan` is skipped there). Push-run approvals follow the `desktop-release`
  environment's reviewers rule.
