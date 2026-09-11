# 0219 Semantic installed lifecycle and privacy qualification

Status: in_progress
Priority: high
Subsystem: quality, release, semantic
Depends on: 0218

## Context

Task 0198 remains NO-GO because exact production semantic artifacts have not been exercised through
an installed desktop lifecycle or inspected for privacy leakage under representative failures.
Task 0218 established immutable, private payload construction and native smoke evidence for all
four supported targets. This task adds the strongest safe automated portion of the remaining gate
without publishing artifacts, changing either release variable, or claiming manual accessibility
evidence.

## Acceptance Criteria

- A deterministic scanner checks collected application, worker, installer, diagnostic, crash, and
  workflow evidence for category-labelled canaries without writing canary values to its report.
  It detects raw and transformed text, UTF-16, encoded path variants, matches split across read
  boundaries, binary payloads, and tampered evidence, and fails closed on unreadable inputs.
- An exact-artifact lifecycle harness verifies the signed production catalog and payload set and
  covers absent state, first install, offline/local artifact use, low disk, interruption/resume,
  catalog/payload/installed-component corruption, restart, concurrent lifecycle access, uninstall
  retention, explicit deletion, and reinstall/recovery. Unsupported or unavailable scenarios are
  recorded honestly rather than reported as passing.
- The packaged desktop smoke runs against private catalog-embedded packages on macOS arm64,
  Windows x86-64, Linux x86-64 on Ubuntu 22.04, and Linux arm64 where the CI runner supports the
  boundary. It records artifact digests, host identity, stage, command, redacted evidence paths,
  result classification, and rollback instructions.
- A private-only manual-dispatch workflow proves before qualification work that it has no
  publication path and that both semantic release variables are absent or false. It never creates
  a release/public asset, enables a release gate, or adds optional semantic payloads to the base
  installer by default.
- The evidence schema distinguishes `pass`, `manual-required`, `unsupported`, and `blocked`.
  VoiceOver, Narrator, Orca, keyboard, consent, progress, error, citation, and deletion checks are
  retained as an operator checklist and are never inferred from automation.
- Task 0198 and `docs/semantic-release-qualification.md` report the automated coverage and
  remaining exact-evaluation, manual-accessibility, preceding-candidate, and release-owner
  blockers honestly.

## Implementation Notes

- Reuse `release-desktop.yml`, the signed production catalog, `ComponentManager`, packaged worker
  smoke, and desktop packaging smoke. Do not add another installer or semantic lifecycle.
- Qualification artifacts remain private GitHub Actions artifacts with short retention. Reports
  contain canary category names and SHA-256 fingerprints only.
- A production-candidate upgrade/rollback row requires an exact preceding private candidate. Do
  not synthesize production evidence when no preceding candidate exists.
- macOS x86-64 remains unsupported by the Zvec 0.7.0 catalog. Windows signing remains explicitly
  unsigned, and Linux x86-64 keeps the Ubuntu 22.04 ABI baseline.

## Agent Notes

- 2026-09-10 Copilot: Started from latest `origin/main` at `518a9d0`, including merged PRs #38 and
  #40. Scope is limited to automated lifecycle/failure/privacy qualification; task 0198 remains
  blocked for exact retrieval evaluation, native assistive-technology review, a real preceding
  production candidate, and release-owner approval.
- 2026-09-11 Copilot: Implemented the exact signed-catalog lifecycle harness, cross-platform
  packaged worker crash/restart test, DMG/MSI/DEB/AppImage launch smoke, category-only privacy
  scanner and evidence integrity verifier, and the private four-target workflow continuation.
  The workflow repeats its non-publication and disabled-gate proofs before building any private
  catalog-enabled package.
