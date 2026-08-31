# 0063 Desktop packaging, signing and notarization

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: desktop
Depends on: 0062

## Context
`file-manager-coding-agent-spec.md` §31 (signing only in protected release workflows), §33 step 10
and §37 (signed macOS and Windows installers).

## Acceptance Criteria
- `pnpm build:tauri` produces installable artefacts on macOS (`.dmg`/`.app`) and Windows
  (`.msi`/`.exe`).
- Product metadata, icons, bundle identifier and version are set from a single source shared with
  the Rust crate version.
- A protected release workflow signs and notarizes the macOS build and signs the Windows installer,
  using repository secrets; PR builds remain unsigned (§31).
- A packaging smoke test installs and launches the artefact in CI where feasible, otherwise the
  manual verification steps are documented.
- Linux packaging is explicitly out of scope for the first release but the build is not broken for
  Linux (§1).
- Release notes/versioning process documented in the README.

## Implementation Notes
- Keep signing credentials out of PR-triggered workflows entirely (not merely conditional).
- Auto-update is not in scope; note the decision.

## Agent Notes
- 2026-08-06 Codex: Added Cargo-owned desktop product metadata and a typed Tauri build wrapper that
  resolves the inherited Rust crate version into macOS `.app`/`.dmg` and Windows `.msi`/NSIS
  targets. Added credential-free PR packaging with disposable-runner install/launch smoke checks,
  plus a tag-only protected release workflow that imports repository certificates, notarizes and
  verifies macOS bundles, signs and verifies Windows installers, and publishes generated release
  notes. Documented versioning, environment secrets, manual platform verification, Linux scope,
  and the no-auto-update decision. Verified 7 task-specific tests via
  `node --test scripts/desktop-packaging.test.mjs`, clean frontend typecheck, `cargo check -p
  fm-desktop`, `pnpm run lint`, a local macOS `.app`/`.dmg` build, and the macOS install/launch smoke
  test. Windows packaging/signing remains platform-untested locally and is exercised by the Windows
  CI/release jobs. The full `pnpm test` run encountered an unrelated transient conflict-resolution
  timeout that passed in isolation; `pnpm run test:scripts` also retains the pre-existing failure
  that expects exactly 10 ADRs although the repository now contains ADR 0011.
- 2026-08-06 Codex follow-up: Extended tag releases with a universal macOS build, generated
  Homebrew cask publication to a configurable tap, and generated Chocolatey packaging/publication
  from the signed NSIS asset. Package metadata and checksums are derived from the exact GitHub
  release downloads; repository configuration and user install commands are documented in README.
  Verified all 9 desktop packaging tests, focused Biome checks, Node syntax, and whitespace checks.
  The external Homebrew push and Windows-only `choco pack`/push remain release-runner tested. The
  full suite reached an unrelated macOS mounted-volume test failure and was stopped after three
  other native-platform tests exceeded 60 seconds; repository-wide lint is also blocked by
  unrelated concurrent Rust formatting and multi-rename frontend changes.
- 2026-08-06 Codex follow-up: Replaced paid Apple Developer signing and notarization with an
  explicitly unsigned universal macOS DMG, while retaining signed Windows installers. Added `.deb`
  and `.AppImage` targets and an Ubuntu 22.04 release job, and documented the macOS Gatekeeper
  warning and Linux smoke checks. The Linux and Windows packages remain platform-tested by their
  release runners rather than this macOS development host. Verified all 9 packaging contract
  tests, whitespace checks, and repository lint (with pre-existing CSS `!important` warnings). The
  full test run reached three environment-dependent `fm-platform-macos` failures: mounted-volume
  enumeration, Launch Services application discovery, and Trash permission.
- 2026-08-06 Codex follow-up: Removed the remaining Windows certificate import, signing, and
  signature verification so the freeware release pipeline requires no paid platform certificates.
  Chocolatey now packages the unsigned NSIS installer, and the README documents the expected
  Microsoft Defender SmartScreen warning.
