# 0195 Cross-platform semantic release artifacts

Status: open
Priority: high
Subsystem: release, backend
Depends on: 0063, 0194

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
