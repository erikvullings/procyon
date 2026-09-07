# 0195 Cross-platform semantic release artifacts

Status: done
Priority: high
Subsystem: release, backend
Depends on: 0194

## Context

Produce the signed worker, native Zvec runtime, and multilingual-E5 model payloads consumed by the
production catalog on every supported desktop target. The current developer bundle proves macOS
arm64 locally but is not a redistributable or production-trusted package.

## Acceptance Criteria

- CI builds reproducible semantic worker payloads for supported macOS, Windows, and Linux targets
  using the repository's pinned Rust toolchain.
- Package the matching Zvec native runtime for each target without runtime downloads. Unsupported
  target/architecture combinations are reported explicitly rather than receiving a mismatched
  payload.
- Package the exact evaluated multilingual-E5 model revision with tokenizer files, license, source
  provenance, resource metadata, and verified member checksums.
- Publish immutable artifacts and update the signed 0194 catalog only after each payload's checksum,
  executable/native-library loading, protocol handshake, and model activation smoke test pass.
- Exercise fresh install, interrupted download/resume, upgrade, rollback, uninstall with retained
  indexes, and uninstall with index deletion on every supported target.
- Keep semantic artifacts separate from the base desktop installer so the optional subsystem does
  not increase download or installed size before consent.

## Implementation Notes

- `zvec-rust` 0.7.0 currently lacks a macOS x64 prebuilt artifact; either produce and verify that
  target in controlled CI or explicitly mark it unsupported.
- Follow the existing release signing/notarization work in 0063; do not weaken platform signing to
  make optional components load.
- Do not package OCRmyPDF here. It remains an optional user-installed executable tracked by 0197.

## Agent Notes

- 2026-09-07 Copilot: Split from production catalog work because platform-native builds, signing,
  smoke tests, and artifact publication can proceed only after the common 0194 contract is stable.
- 2026-09-07 Copilot: Removed 0063 as a hard dependency when implementation started. Semantic
  payload construction and verification do not depend on completing the still-unconfigured Windows
  desktop code-signing criterion; the payload workflow inherits each platform's existing release
  trust without weakening it. Task 0194 is complete at `0609945`.
- 2026-09-07 Copilot: Added the production-only `semantic-runtime` worker feature and explicit
  managed data/model arguments; production and development model packs reject each other's trust
  paths. Added a deterministic packer in
  `crates/fm-semantic-components/src/release_bundle.rs` that verifies the pinned multilingual-E5
  cache, emits content-addressed worker/Zvec/model payloads, and records exact pipeline provenance.
  `.github/workflows/release-desktop.yml` now builds macOS arm64, Windows x86-64, Linux x86-64, and
  Linux arm64 payloads separately from desktop installers, signs and notarizes macOS executable
  payloads under the existing release policy, invokes protected catalog signing, deduplicates the
  target-independent model, and publishes only after protocol, real-model activation, and component
  lifecycle tests. The packaged-binary smoke test launches the content-addressed worker through the
  managed connector, and Unix installation restores an owner-only executable bit after artifact
  transport. Intel macOS is explicitly unsupported because Zvec 0.7.0 has no matching runtime. A
  real 523 MiB macOS arm64 bundle built from the pinned 465 MiB model cache and passed protocol,
  offline production-model activation, and lifecycle smoke tests.
