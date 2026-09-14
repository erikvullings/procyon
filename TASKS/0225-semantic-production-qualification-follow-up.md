# 0225 Semantic production qualification follow-up

Status: open
Priority: medium
Subsystem: quality, release, semantic
Depends on: 0198

## Context

The first public semantic distribution may ship as an explicitly experimental, opt-in OSS alpha
after task 0198's automated four-platform and manual macOS gates pass. A production or stable claim
requires broader native manual evidence and a real semantic-to-semantic upgrade history that do not
reasonably block the first alpha.

## Acceptance Criteria

- Complete keyboard-only and Narrator semantic install, consent, progress, error, citation,
  retention, deletion, and recovery passes on Windows x86-64.
- Complete the equivalent keyboard-only and Orca passes on Linux x86-64 at the Ubuntu 22.04 ABI
  baseline and on Linux arm64.
- Repeat critical consent/error flows at 200% zoom, relevant high-contrast themes, and reduced
  motion on every supported platform.
- Qualify upgrade and rollback between two exact, signed public semantic candidates on all
  supported targets while preserving consent/enrolment and rebuilding incompatible derived indexes.
- Complete exact generated-answer and specialized generated-summary, unavailable-source, and
  concept-label corpus evidence on every supported target.
- Attach reviewed before/after production evidence for the case-folded embedding migration and
  document quality, storage, and rebuild effects.
- Inspect native failure/crash evidence and default logs on every supported platform for semantic
  privacy leakage.
- Replace the experimental-alpha qualification with a fail-closed production/stable GO report
  containing no deferred or waived supported-platform rows.

## Implementation Notes

- Do not retroactively describe alpha evidence as production evidence.
- macOS x86-64 remains unsupported until the selected Zvec runtime publishes a matching artifact.
- Windows code-signing remains a separately documented distribution-policy limitation unless the
  project adopts signing before stable qualification.

## Agent Notes

- 2026-09-14 Copilot: Split from task 0198 when the release owner selected a proportionate OSS
  alpha gate: automated evidence on all supported targets plus a manual macOS arm64 pass. This task
  owns the stricter stable-release work and starts only after the first semantic alpha ships.
