# 0217 Base alpha release 0.1.0-24

Status: done
Priority: high
Subsystem: release
Depends on: none

## Context

Publish the current main branch as the next numeric alpha/prerelease while semantic search,
Structured Knowledge Search, and Ask remain excluded. The production semantic distribution is
still a NO-GO under 0198 and has no qualified package.

The release must use the existing fail-closed gates rather than weakening or bypassing semantic
qualification. Main CI initially had one stale frontend assertion and one Linux timing-sensitive
ingestion test that had to be made green before tagging.

## Acceptance Criteria

- Bump the workspace version from `0.1.0-23` to `0.1.0-24`.
- Keep `SEMANTIC_RELEASE_QUALIFIED` and `KNOWLEDGE_SEARCH_RELEASE_QUALIFIED` absent or false.
- Restore green main CI without changing production behavior.
- Publish tag and GitHub prerelease `v0.1.0-24`.
- Confirm base desktop installers are published and no semantic worker, runtime, model, or catalog
  assets are attached.

## Implementation Notes

- Numeric prerelease identifiers are required by MSI/WiX.
- Follow the repository release sequence: bump, commit, push, wait for main CI, then tag and push.
- Do not mark 0198 complete or enable either semantic release gate.

## Agent Notes

- 2026-09-10 Copilot: Both protected semantic qualification variables are absent. The current main
  CI failure is limited to a stale native-menu UI assertion and a Linux ingestion test whose
  500 ms polling window expired under CI load; macOS/Windows builds and the remaining checks pass.
- 2026-09-10 Copilot: Prepared `0.1.0-24` in `a4c12ba`, refreshed the deterministic NO-GO
  retrieval report after the Cargo.lock version change in `b6d40e2`, and obtained green main CI in
  [run 34466254625](https://github.com/erikvullings/procyon/actions/runs/34466254625).
  Published [prerelease v0.1.0-24](https://github.com/erikvullings/procyon/releases/tag/v0.1.0-24)
  from that exact commit. Release
  [run 34469526756](https://github.com/erikvullings/procyon/actions/runs/34469526756) published the
  macOS universal DMG, Linux x86-64 DEB and AppImage, and Windows x86-64 MSI and NSIS installers.
  Both qualification variables remained absent; semantic payload, catalog, and publication jobs
  were skipped, and the release has no semantic worker, Zvec runtime, model, or catalog assets.
