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
