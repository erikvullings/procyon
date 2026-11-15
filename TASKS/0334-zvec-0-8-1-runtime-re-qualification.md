# 0334 Re-qualify the Zvec 0.8.1 runtime for production semantic releases

Status: in_progress
Priority: high
Subsystem: semantic
Depends on: 0181, 0195, 0198, 0201, 0202, 0218

## Context

`11297f61` ("Upgrade dependencies and prevent upload cancellation livelock") moved the production
Zvec pin from `0.7.0` to `0.8.1` in `Cargo.toml` and in the worker's packaging matrix, and updated
parts of `docs/architecture/zvec-rust-sdk.md`. It did not update the release-qualification contract
that audits those bytes, so the pinned audit trail still described `0.7.0`. The consequence is
concrete: semantic component qualification run `43732457072` failed identically on all four targets
with `Error: Zvec qualification report disagrees with the catalog or packed artifact` at
`scripts/smoke-semantic-production-bundle.mjs:61` →
`scripts/zvec-runtime-qualification.mjs:717`, which blocks the risk-accepted Gemma candidate
re-signature and therefore the `0.7.0-15` desktop beta.

Four separate drifts, found one at a time:

1. `scripts/zvec-runtime-qualification.mjs` still pinned the `0.7.0` version, upstream revisions,
   release-asset IDs, archive/loader/import-library byte lengths and SHA-256 values, and crate
   checksums. The worker requires `0.8.1`, so the audit and the shipped artifact disagreed by
   construction.
2. `PRODUCTION_ZVEC_NATIVE_SOURCE_REVISION` in `crates/fm-semantic-components/src/release_bundle.rs`
   was `9c1a7f95b0ddbf1e478c4fac2d6735903d6e6120` — a commit that exists in neither upstream
   repository. It is not cosmetic: that constant is interpolated into the runtime artifact's
   Apache-2.0 license notice and provenance record, and a test asserts the notice contains it.
3. `docs/architecture/zvec-rust-sdk.md` carried `0.8.1` version strings above the `0.7.0` tag
   commits, asset IDs, digests, and dependency lists, and still repeated `0.7.0`-specific claims.
4. Intel macOS unavailability was attributed to upstream publishing no runtime. The `v0.8.1`
   release does publish `zvec-prebuilt-x86_64-apple-darwin.tar.gz` (asset `742882583`,
   12,340,298 B), so the remaining reason is Procyon's own missing x86-64 macOS worker build.

## Acceptance Criteria

- Every `0.8.1` input is pinned to bytes verified against the real upstream release, and the
  `prepare`/`report` contract passes on all four GitHub-hosted targets.
- The production bundle records upstream revisions that exist, for both the SDK and the vendored
  native tree.
- Documentation states the pinned identity accurately and no longer attributes Intel macOS
  unavailability to a missing upstream artifact.
- The `0.7.0`-era API/FTS audit that 0201 recorded in `docs/architecture/zvec-rust-sdk.md` is
  re-run against the `0.8.1` source, and any changed semantic is reflected in that document and in
  the adapter.
- `node scripts/gemma-risk-acceptance.mjs --desktop --target <target>
  --release-source-revision <sha>` passes for all four desktop targets, i.e. `verifySemanticWorker-
  Source` finds the signed candidate an ancestor of the release commit with no changed semantic
  worker build inputs.
- A risk-accepted Gemma v2 candidate is re-signed against the refreshed contract,
  `docs/evaluations/semantic-gemma-risk-accepted-v1.json` records it, and the `0.7.0-15` desktop
  beta builds from it.

## Implementation Notes

- Upstream identity, verified directly against GitHub and crates.io:
  - `zvec-ai/zvec-rust` `v0.8.1` → commit `668f8270d3023bda0f3ed2ba650a508dca78d648`
  - `alibaba/zvec` `v0.8.1` → commit `30d662db4d2467937d843443074b34743bcc9baa`, which is exactly
    the commit `zvec-ai/zvec-rust` records as its `vendor/zvec` submodule at `v0.8.1` (and
    `8321c1314a559fd5f909e92498f43e5194bf9b99` at `v0.7.0`, the revision the old documentation
    pinned). The SDK's release commit message states the same: "chore(vendor): track zvec v0.8.1;
    feat: adapt to grouped search and IVF PQ (#25)".
  - crates.io: `zvec-rust` 72,344 B `2c784dd9…`, `zvec-rust-sys` 20,641 B `48ce89de…`, both
    matching the pinned `Cargo.lock` checksums
  - Apache-2.0 `LICENSE` (11,356 B) and native `NOTICE` (5,020 B) hash identically to `0.7.0`
- `0.8.1` adds `libdl.so.2`, `libpthread.so.0`, and `librt.so.1` to the Linux arm64 loader, which
  the `0.7.0` matrix did not list. Linux x86-64 and Windows x86-64 dependency sets are unchanged.
- macOS x64 stays unsupported deliberately: `SUPPORTED_TARGETS`, the worker's packaging matrix, and
  this script keep four targets. Qualifying a fifth is its own change (worker build, runner, model
  packs, installed-state qualification), not a pin edit.
- Release gating is driven by repository variables, currently `SEMANTIC_RELEASE_QUALIFIED=true`,
  `SEMANTIC_GEMMA_RISK_ACCEPTED=true`, `SEMANTIC_COMPONENTS_RELEASE_QUALIFIED=true`, and
  `KNOWLEDGE_SEARCH_RELEASE_QUALIFIED=true`. With risk-accepted Gemma on, every desktop job in
  `.github/workflows/release-desktop.yml` runs the Gemma gate before packaging, so a tag push fails
  until the candidate is re-signed.
- Related: `docs/architecture/semantic-component-releases.md`,
  `docs/architecture/zvec-rust-sdk.md`, `scripts/zvec-runtime-qualification.mjs`,
  `scripts/smoke-semantic-production-bundle.mjs`, `scripts/qualify-semantic-installed.mjs`,
  `scripts/check-desktop-gemma-release.mjs`, `crates/fm-semantic-components/src/release_bundle.rs`,
  `crates/fm-semantic-worker/src/zvec_storage.rs`.

## Agent Notes

- 2026-11-15: Re-pinned `scripts/zvec-runtime-qualification.mjs` to `0.8.1` (version, both
  revisions, four asset IDs, archive/loader/import-library bytes and digests, both crate
  checksums, and the Linux arm64 dependency list) and corrected the dangling native revision in
  `release_bundle.rs` to `30d662db…`. All four `v0.8.1` archives were downloaded and checked with
  the script's own `verifyPinnedFile` / `verifyArchiveMembers` / `verifyNativeArchitecture`, and
  each loader's dynamic dependencies were read independently (`otool -L`, `llvm-objdump -p`,
  `llvm-readelf -d`) and match the pinned sets. `node scripts/zvec-runtime-qualification.mjs
  prepare` succeeds for `aarch64-apple-darwin`.
- The Windows import library matches the digest published in upstream's own archive. The Windows
  dependency set was derived with `llvm-objdump` because no MSVC host is available here; it matches
  the pinned set exactly, and the `win32-x64` runner still enforces it with `dumpbin`, so a
  disagreement fails that job rather than passing silently.
- Functional evidence on the new runtime: `cargo test -p fm-semantic-worker --features zvec` passes
  184 tests including the whole `zvec_storage` suite against the real `0.8.1` native library (real
  SDK schema CRUD, FTS, HNSW with concurrent readers, single-writer enforcement, crash recovery,
  v1→v2 migrations). `cargo check -p fm-semantic-worker --features zvec` builds
  `zvec-rust`/`zvec-rust-sys` `0.8.1` cleanly. `cargo test -p fm-semantic-components`, its clippy,
  and `cargo fmt --check` pass with the corrected provenance constants.
- `node --test scripts/*.test.mjs` passes 203 tests. `scripts/zvec-runtime-qualification.test.mjs`
  now derives its expectations from `ZVEC_VERSION` instead of a stale `0.7.0` literal and asserts
  the pinned revisions are 40-hex.
- Still open, in order: (1) run the four-target qualification for this pin via the Gemma re-sign
  flow, (2) re-audit 0201's API/FTS claims against `0.8.1` source, (3) land the refreshed
  `docs/evaluations/semantic-gemma-risk-accepted-v1.json`, (4) build `0.7.0-15`.
- Verified blocker for step 4 today: the real desktop gate
  `SEMANTIC_RELEASE_QUALIFIED=true SEMANTIC_GEMMA_RISK_ACCEPTED=true node
  scripts/gemma-risk-acceptance.mjs --desktop --target macos-aarch64
  --release-source-revision "$(git rev-parse HEAD)"` fails with `semantic worker build inputs
  changed after signed candidate`, listing the `Cargo.lock` upgrades from `11297f61` (including
  `zvec-rust 0.7.0` → `0.8.1` and the tokenizer stack) relative to `75b5cf97`. The candidate must
  therefore be re-signed from a commit that will also be the release commit, or a descendant with
  no further semantic-worker build-input changes.
- Do not loosen `verifyZvecRuntimeQualification` or the four-target set to make a run pass; the
  mismatch it reported was the contract doing its job.
