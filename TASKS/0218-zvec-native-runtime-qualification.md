# 0218 Zvec native-runtime qualification preparation

Status: done
Priority: high
Subsystem: semantic, release, quality
Depends on: 0195

## Context

Turn the optional Zvec 0.7.0 native runtime produced by task 0195 into an auditable production
qualification input. The release bundle already keeps Zvec separate from the desktop installer,
but its Cargo build trusted the SDK's unchecked auto-download and did not retain target,
architecture, native-dependency, license, signing, or notarization evidence for the exact packed
bytes.

This task prepares evidence for blocked task 0198. It does not qualify or publish the semantic
subsystem and must not change either release gate.

## Acceptance Criteria

- Pin the zvec-rust and native Zvec source commits, crates.io checksums, GitHub release asset IDs,
  archive sizes and SHA-256 digests, extracted loader sizes and SHA-256 digests, and redistribution
  license/NOTICE provenance for Zvec 0.7.0.
- Support only macOS arm64, Windows x86-64, Linux x86-64, and Linux arm64; reject macOS x86-64 and
  every unknown target before building.
- Fetch the native archive into a content-verified build cache, validate its exact member set and
  `TARGET` marker, and build with `ZVEC_AUTO_BUILD=0` plus the verified `ZVEC_LIB_DIR`.
- Require `libzvec_c_api.dylib`, `zvec_c_api.dll`, or `libzvec_c_api.so` as appropriate and reject
  wrong-format or wrong-architecture binaries.
- Inspect the exact upstream and packed runtime with `otool`, `dumpbin`, or `readelf`; fail on
  missing or unexpected dynamic dependencies.
- Emit a retained per-target qualification record linked to the catalog artifact ID with target,
  loader, artifact length/checksum, upstream provenance, build revision, licensing, dependencies,
  and honest signing/notarization status.
- Smoke the content-addressed worker against the content-addressed runtime copied under its native
  loader name while the original build cache is unavailable.
- Require Developer ID signing and accepted Apple notarization evidence for the macOS arm64
  qualification artifact. Record Windows artifacts as unsigned under the existing release policy.
- Prove that `workflow_dispatch` cannot invoke public release, Homebrew, or Chocolatey publication
  before using the workflow. Manual qualification outputs remain private Actions artifacts.
- Retain task 0198 as NO-GO until its complete cross-platform installed, quality, accessibility,
  privacy, and failure-mode matrix passes.

## Implementation Notes

- Do not bundle the runtime into the base installer or add runtime downloads.
- Do not select a model, change retrieval thresholds, or enable either semantic release gate.
- The upstream runtime archives contain only `TARGET` and the native loader, plus the Windows
  import library. Native Zvec's Apache NOTICE still applies and is recorded from the immutable
  native source commit.

## Agent Notes

- 2026-09-10 Copilot: Upstream audit fixed zvec-rust at
  `733e0bc82e02a0c63202bff594a7f4530520dfd0` and its native Zvec submodule at
  `8321c1314a559fd5f909e92498f43e5194bf9b99`. The GitHub v0.7.0 release exposes four assets and no
  macOS x86-64 asset. The macOS arm64 archive and extracted loader were independently downloaded
  and matched the pinned upstream archive digest and calculated loader digest; `file`/`otool`
  confirmed an arm64 Mach-O with only CoreFoundation, libc++, and libSystem dependencies.
- 2026-09-10 Copilot: Static workflow proof found and closed three manual-dispatch publication
  paths: unguarded Linux and Windows GitHub Release actions and the Chocolatey reusable workflow.
  Manual dispatch now retains installers and semantic inputs only as private Actions artifacts.
  Its catalogs use `qualification.invalid` and are not embedded in dispatch installers, preventing
  an installed build from pointing at deliberately unpublished bytes.
- 2026-09-10 Copilot: The unsigned local macOS arm64 production bundle built from the pinned 465 MiB
  multilingual model and verified runtime, hid the build-time Zvec cache while launching the exact
  content-addressed worker/runtime, reached the worker argument parser, completed the protocol
  handshake, and activated the production model offline. The runtime artifact was
  `procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.c9e4bf9387ef7261`; this local artifact is not
  production evidence because it was intentionally unsigned and built from a dirty development
  checkout.
- 2026-09-10 Copilot: All 17 focused Node tests, 13 release-bundle tests, 18 real Zvec storage tests,
  packaged protocol/model/component smoke tests, and full `pnpm run lint` passed. The broader script
  suite passed 64/67; its three failures are pre-existing task-0005/task-0074 documentation
  assertions for the already-absent ADR/README sections. Task 0198 remains NO-GO because signed
  native-host artifacts and its installed quality/accessibility/privacy/failure matrix are still
  outstanding.
