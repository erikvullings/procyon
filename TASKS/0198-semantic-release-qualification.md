# 0198 Semantic release qualification

Status: blocked
Priority: high
Subsystem: quality, release
Depends on: 0195, 0196

## Context

Qualify the complete production semantic distribution before including it in a Procyon desktop
release. Feature-level tests and the macOS arm64 developer corpus are not substitutes for installed
artifact, upgrade, cross-platform, retrieval-quality, privacy, and failure-mode evidence.

## Acceptance Criteria

- Run the task-0188 labelled evaluation suite against the exact production model, converter,
  chunker, retrieval policy, and index identity; record file/chunk recall, MRR, nDCG, negative
  controls, and grounded-answer citation correctness.
- Calibrate absolute and relative Ask similarity thresholds from the labelled report. Do not lower
  the current `0.84` absolute floor merely to increase result count; document before/after quality
  and storage/migration impact for any threshold or pipeline change.
- Complete installed/absent, first-run, upgrade, rollback, corruption, offline, low-disk,
  cancellation, crash/restart, and deletion/retention tests on supported macOS, Windows, and Linux
  release builds.
- Complete manual keyboard, screen-reader, consent, progress, error, citation-opening, and data
  deletion passes on each supported desktop platform.
- Confirm default logs and crash reports contain no query, excerpt, filename, prompt, response,
  credential, token, or model payload content.
- Publish an operator-readable qualification report identifying exact artifact/catalog versions,
  known limitations, unsupported targets, and rollback instructions.
- Only after all release gates pass, enable the production catalog in the normal desktop release
  workflow and verify the produced installers against the published catalog.

## Implementation Notes

- This is the gate for a user-facing semantic release, not for merging the implementation branch.
- Deterministic Docling needs no separate package, but its exact converter identity belongs in the
  evaluation fingerprint.
- OCRmyPDF qualification is added when 0197 ships and does not block semantic search/Ask for
  searchable documents.

## Agent Notes

- 2026-09-07 Copilot: Current TRIZ calibration observed `0.905` for `Su-fields`, `0.881` for the
  full cup/hot-liquid question, and `0.848`-`0.850` for hard unrelated controls. This supports
  keeping the `0.84` floor unchanged until a larger labelled evaluation, not lowering it.
- 2026-09-08 Copilot: Release decision is **NO-GO**. The repository has no signed production
  catalogs/installers or production-run task-0188 observations, and cross-platform installed,
  accessibility, privacy, and failure-mode evidence has not been collected. Added
  `docs/semantic-release-qualification.md` as the operator record and made release publication
  fail closed behind the protected `SEMANTIC_RELEASE_QUALIFIED == 'true'` repository variable.
  Base desktop releases continue without a production semantic catalog while this task is blocked.
- 2026-09-10 Copilot: The Linux x86-64 Ubuntu 22.04 production-link blocker found during task 0218
  was traced to pyke's ONNX Runtime 1.28.0 static archive, not Zvec. The replacement is Microsoft's
  official matching shared CPU loader, pinned by release asset, archive, source revision, loader,
  license, and third-party-notice digests. Packaging rejects native inputs above Ubuntu 22.04's
  glibc/libstdc++/CXXABI ceilings and installs the loader only as a separate optional semantic
  component. This closes one production-payload construction gap only. The task remains blocked
  and **NO-GO** until the exact production evaluation, installed lifecycle, accessibility,
  privacy, and failure-mode criteria above all pass.
- 2026-09-10 Copilot: Private run `34509441435` passed payload construction and isolated packaged
  smoke on macOS arm64, Windows x86-64, Linux x86-64 Ubuntu 22.04, and Linux arm64. This closes the
  native Linux x86-64 construction blocker only. No signed aggregate catalog, public semantic
  asset, or catalog-embedded installer was produced, and all installed-app, task-0188 quality,
  accessibility, privacy, and failure-mode rows remain outstanding.
- 2026-09-11 Copilot: Task 0219 added a dispatch-only, read-only installed qualification
  continuation for the exact private four-target payload/catalog matrix. It builds catalog-enabled
  packages without publication, crosses DMG/MSI/DEB/AppImage boundaries, exercises the signed
  component lifecycle and packaged worker crash/restart, and scans retained evidence with unique
  sensitive canaries. Reports distinguish pass, manual-required, unsupported, blocked, and fail.
  This automation does not unblock 0198: exact production retrieval/Ask evaluation, an exact
  preceding-candidate upgrade/rollback, native VoiceOver/Narrator/Orca and keyboard UX passes, a
  completed private matrix run, and release-owner approval are still required.
- 2026-09-11 Copilot: Private run `34624618891` passed the exact packaged worker/model/runtime
  lifecycle and fail-closed privacy scan for all supported targets on commit
  `1fb6f144eb977468ea0335de8e3f0ab4421a5ae3`. During qualification, Windows exposed and fixed a
  named-pipe verification defect: Windows may normalize owner `GENERIC_ALL` to
  `FILE_ALL_ACCESS`; the verifier now accepts either full-control representation while still
  rejecting empty ACLs, deny/foreign ACEs, and foreign owners. Signed catalog and installed
  package qualification then failed closed because the protected signing secret and verifying-key
  variable are not configured. No release, public asset, base installer, Homebrew artifact, or
  Chocolatey artifact was produced. Task 0198 remains **NO-GO**.
- 2026-09-11 Copilot: Task 0220 adds the exact-production task-0188 runner and fail-closed report
  validator. The private payload matrix now ingests the repository-owned generated corpus through
  the packaged worker/runtime/model/converter/chunker/Zvec path and retains opaque per-case and
  aggregate evidence. The checked-in report remains an explicit NO-GO template, both release
  variables remain unchanged, and this task stays blocked on reviewed four-target metrics,
  generated-answer grounding, installed lifecycle, accessibility, privacy, failure-mode, and
  release-owner evidence.
- 2026-09-11 Copilot: The production candidate now applies versioned Unicode default case folding
  symmetrically to passage and query embeddings. Existing derived indexes are reset and rebuilt;
  original display/full-text content is preserved and the `0.84`/`0.02` Ask thresholds are
  unchanged. Release remains NO-GO until task 0220 records reviewed before/after production
  evidence for this embedding-space migration.
