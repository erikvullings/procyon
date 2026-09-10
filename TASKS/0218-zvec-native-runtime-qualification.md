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
  Manual dispatch now runs only the private semantic payload/catalog path. Its catalogs use
  `qualification.invalid`; desktop installer jobs are push-only, preventing an installed build
  from pointing at deliberately unpublished bytes.
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
- 2026-09-10 Copilot: Private workflow run `34475811458` proved that prerelease, semantic
  publication, Homebrew, and Chocolatey stayed skipped. All four semantic payload jobs failed
  before packaging: Linux x86-64, Linux arm64, and Windows x86-64 exposed a shared CLI defect where
  pnpm forwarded a literal `--`; macOS acquired no `macos-14-xlarge` runner and executed no steps.
  The base macOS, Windows, and Linux installer jobs succeeded without semantic catalogs. The
  follow-up accepts and tests the package-manager separator, switches the arm64 payload to the
  upstream-proven `macos-15` label, and removes installer jobs from manual dispatch.
- 2026-09-10 Copilot: Private run `34480698440` retained clean, fully smoked Windows x86-64 and
  Linux arm64 payloads. Runtime IDs are
  `procyon.semantic.zvec-runtime.windows-x86_64.0.7.0.3745106b3beee6be` and
  `procyon.semantic.zvec-runtime.linux-aarch64.0.7.0.621af6ba8249ce44`; worker IDs are
  `procyon.semantic.worker.windows-x86_64.0.1.0.24.c4f491b1267419ac` and
  `procyon.semantic.worker.linux-aarch64.0.1.0.24.d6d01578de0aa7fd`. Windows is explicitly
  unsigned. macOS reached linking but the semantic job had not installed the lld path required by
  `.cargo/config.toml`; the follow-up mirrors the existing desktop release's `brew install lld`.
  Linux x86-64 verified Zvec, then remained blocked at worker link because the pinned ONNX Runtime
  rc.13 archive references glibc 2.38 `__isoc23_*` and newer libstdc++ symbols unavailable on the
  Ubuntu 22.04 desktop baseline. Moving that job to Ubuntu 24.04 would hide rather than qualify the
  compatibility gap, so it remains an explicit task-0198 blocker.
- 2026-09-10 Copilot: Run `34482628513` proved the macOS arm64 bundle through Developer ID signing,
  codesign verification, Apple notary acceptance, and submitted-ZIP/artifact digest binding.
  Signed runtime and worker IDs were
  `procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.69abaed8e9309eeb` and
  `procyon.semantic.worker.macos-aarch64.0.1.0.24.73e5d803bfd2fe32`. The job then failed because
  `spctl --type execute` correctly reports a standalone CLI is not an app, so smoke/upload did not
  run. The follow-up removes that inapplicable app-bundle assessment while retaining strict
  codesign and artifact-bound Apple acceptance.
- 2026-09-10 Copilot: Final private run `34483603909` retained fully smoked payloads for macOS
  arm64, Windows x86-64, and Linux arm64. The macOS runtime
  `procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.77431044a055f64c` and worker
  `procyon.semantic.worker.macos-aarch64.0.1.0.24.ae2092aae393df7f` passed Developer ID validation,
  Apple notary acceptance, artifact-bound receipt recording, production-trust verification,
  protocol handshake, offline model activation, lifecycle tests, and private upload. Linux x86-64
  alone reproduced the ONNX Runtime/Ubuntu 22.04 link blocker. Every public or installer job stayed
  skipped. Dispatch-only continuation did not enable partial catalog signing and is removed so the
  final workflow reports this supported-target blocker as a failure; successful target payloads
  remain retained. The signed runtime is 23,164,624 bytes with SHA-256
  `77431044a055f64c359d8c70614d86d15c16f636b94a23e7afb3618c06f2d36c`; Apple submission
  `2a374034-7207-4187-b3ff-ae4f720caa32` accepted the 20,883,412-byte ZIP with SHA-256
  `73b452811684e8df5b5add22a9267c24a77d30affe0ec0d4f3bb208165524c94`.
