# Semantic release qualification

## Decision

**NO-GO as of 2026-09-10.** Do not set the protected repository variable
`SEMANTIC_RELEASE_QUALIFIED` to `true`. Tagged desktop releases remain available, but skip semantic
payload construction, catalog signing and publication, and catalog embedding. Managed semantic
installation therefore remains unavailable in those installers. A manual workflow dispatch still
builds non-published semantic payloads and signed catalogs so operators can retain native artifact
evidence without creating an installer that points at private or nonexistent release assets.
Desktop installer jobs are push-only. Installed semantic qualification remains a separate
task-0198 gap.

This report is the operator record for task 0198. A code-complete subsystem and developer-bundle
results are not substitutes for measurements from the exact signed production artifacts.

## Candidate identity

| Property | Candidate |
| --- | --- |
| Procyon revision reviewed | Bound by each private report's `procyonRevision`; no reviewed four-target aggregate is checked in |
| Model | `intfloat/multilingual-e5-small` |
| Model revision | `614241f622f53c4eeff9890bdc4f31cfecc418b3` |
| Tokenizer | `xlm-roberta-sentencepiece.614241f6` |
| Dimensions / normalization | 384 / L2 |
| Converter | `docling-pdf/1036000+baseline/2` |
| Chunker | `structural/3` |
| Embedding preprocessing | Unicode default case folding `unicode-default-case-fold/1` for both passages and queries |
| Retrieval policy | single query, absolute floor `0.84`, strongest-candidate window `0.02`, maximum 8 documents, 2 chunks per document, 8,192 context tokens |
| Worker protocol / index schema | 1 / 2 |
| Zvec runtime | `zvec-rust` `v0.7.0` |
| Linux x86-64 inference runtime | Microsoft ONNX Runtime `v1.28.0` CPU shared loader |
| Optional OCR | user-installed OCRmyPDF stable `>=16.0.0,<18.0.0`, disabled by default |

The final artifact IDs, byte lengths, SHA-256 digests, catalog revision, signature, and installer
digests are intentionally blank until a qualification run produces and retains them. Do not
substitute developer catalog identities.

## Zvec native-runtime qualification inputs

Task 0218 independently audited the Zvec Rust v0.7.0 release and made its runtime inputs immutable.
The Rust tag resolves to commit `733e0bc82e02a0c63202bff594a7f4530520dfd0`; its native Zvec
submodule and the native v0.7.0 tag resolve to
`8321c1314a559fd5f909e92498f43e5194bf9b99`. The crates.io checksums are
`09da6c0c29360b764d54f8d4107174f1fb60921d25387fd433f263ec4bf19e5a` for `zvec-rust` and
`e1ea26d758a283798af569947fe9e3fb29e10cbffc5b09f50ef626cedfc4e013` for
`zvec-rust-sys`.

| Target | Asset ID | Archive bytes / SHA-256 | Loader | Loader bytes / SHA-256 | Allowed dynamic dependencies |
| --- | ---: | --- | --- | --- | --- |
| macOS arm64 | `530247359` | 8,135,328 / `59c41dcbaab69b9fbcf3ca0f1997f58f189a025657fd09a464dca199107cdeb2` | `libzvec_c_api.dylib` | 23,146,352 / `c9e4bf9387ef7261a284de407ec7e48ac9a48309d8daaa4c5ed85a8fa5bb4763` | CoreFoundation, libc++, libSystem |
| Windows x86-64 | `530247358` | 8,839,865 / `d8fe5585ad83066038f6e60990fe6e69528637a58fffc5024ca211c187a9d49a` | `zvec_c_api.dll` | 26,570,240 / `3745106b3beee6be2d50ca678b46d3f0289afb5136ff51e1d1ec037a27b29e4a` | dbghelp, KERNEL32, ole32, RPCRT4, SHELL32, SHLWAPI |
| Linux x86-64 | `530247354` | 13,331,422 / `7e9adbeadc42c772665efed45112220aa895d3f7963fa03c016102f2f414c37f` | `libzvec_c_api.so` | 36,854,864 / `89eac719eb426a2066d2104e5b1199aa83ec18eaa4c31c7797b9bf469904cfd5` | x86-64 loader, glibc, libdl, libm, libpthread, librt |
| Linux arm64 | `530247355` | 11,784,274 / `0195a85f07370d7bcbf26f990bf794e31430e00224c8b9303d43ea677db6f77d` | `libzvec_c_api.so` | 32,470,624 / `621af6ba8249ce44dc17fb05da6c51c723cc466843e7f46ee44a40bd7eee1169` | AArch64 loader, glibc, libm |

The archive digests are published by the GitHub release API; loader digests and dependency lists
were calculated from those verified archives and pinned in the release tooling. The macOS arm64
archive was downloaded again on 2026-09-10 and matched both values; `file` and `otool -L` confirmed
the expected arm64 Mach-O and only the listed system dependencies. The remaining target bytes have
not yet been rerun on their native qualification hosts.

Each non-published bundle now retains `zvec-runtime-qualification.json`, linked to the catalog
artifact ID. It records the final post-signing artifact length/SHA-256, target, loader filename,
both source revisions, release asset, crate checksums, Procyon build revision, dynamic-dependency
inspection, immutable Apache-2.0 license/NOTICE sources, and signing/notarization status. macOS
qualification requires `developer-id-verified` and `apple-notary-service-accepted`; Windows is
accurately recorded as unsigned; Linux signing and notarization are not applicable.
Accepted macOS evidence includes the Apple submission ID, SHA-256 and length of the submitted ZIP,
and the exact worker/runtime artifact IDs, lengths, and SHA-256 digests. The recorder extracts the
submitted ZIP and rejects it unless those bytes match the catalog before changing the status.

The release build sets `ZVEC_AUTO_BUILD=0` and links only from the verified cache. Its offline smoke
temporarily removes that cache, copies the content-addressed runtime under the platform loader
name, and proves that the content-addressed worker reaches its argument parser. Later bundle smoke
repeats the protocol handshake from an isolated loader directory.

## Linux x86-64 ONNX Runtime input

The Linux x86-64 worker uses a separately packaged Microsoft ONNX Runtime CPU shared loader.
macOS arm64, Windows x86-64, and Linux arm64 retain their previously exercised static linkage.
The dynamic input is target-specific and optional; it is not installed with the base desktop
application and is not resolved from a host library.

| Property | Pinned value |
| --- | --- |
| Release / source revision | `v1.28.0` / `da9b5e364c465de65c49d91e696cd6485270757f` |
| Release asset | ID `489174677`, `onnxruntime-linux-x64-1.28.0.tgz` |
| Archive bytes / SHA-256 | 9,125,960 / `a3e1b79d7bb1bf09696ce675f49e4064e6c81f6202b8225624fff0e93f8d6407` |
| Loader source file | `libonnxruntime.so.1.28.0` |
| Loader bytes / SHA-256 | 24,268,848 / `1461ef7cc3d9e49982591721683cc3e3a55580aeca9a5254e7aac47b75ee4bab` |
| Runtime SONAME | `libonnxruntime.so.1` |
| Required ABI maxima | GLIBC 2.27, GLIBCXX 3.4.21, CXXABI 1.3.11 |
| Ubuntu 22.04 rejection ceilings | GLIBC 2.35, GLIBCXX 3.4.30, CXXABI 1.3.13 |
| License / notices | MIT `LICENSE`, 1,073 bytes / `2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c`; `ThirdPartyNotices.txt`, 325,054 bytes / `0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2` |

The exact pyke static archive remains useful negative evidence: its 10,060,013 bytes hash to
`e454f710f8a49f53aa5b4ff51e3454ae1835777e431c6c35c5255ce6f205fd68`, and its extracted
105,481,448-byte `libonnxruntime.a` hashes to
`0bb8a9982b44df690195c2c34b75ca791c3b9f20070b8cecbd8f50c6264dd2e2`.
The archive's objects directly reference `__isoc23_strtol`, `__isoc23_strtoll`,
`__isoc23_strtoull`, and libstdc++ `_M_replace_cold`; the packaging regression check rejects
those symbols and any ABI version above the Ubuntu 22.04 ceilings before catalog construction.

The official archive also contains `libonnxruntime_providers_shared.so`, but the CPU loader has no
`DT_NEEDED` dependency on it and Procyon enables no plugin execution provider. It is therefore not
shipped. The isolated production smoke must load the exact packaged worker with only the cataloged
ONNX and Zvec runtime bytes, proving that no omitted provider or build-cache library is required.

Rejected alternatives were: moving x86-64 to Ubuntu 24.04, which hides the supported baseline;
symbol shims; an environment-only host-library override; ONNX Runtime 1.27, an unnecessary native
downgrade; a broad Cargo upgrade without evidence that it changes the incompatible native input;
and a costly source build when Microsoft publishes a matching, checksum-addressable,
redistributable CPU loader.

`pnpm run semantic:qualification:check` statically verifies that manual dispatch cannot reach a
GitHub Release upload, Homebrew push, or Chocolatey publication. The audit found and closed
unguarded Linux and Windows release-action steps plus the Chocolatey reusable job. On tag builds,
semantic payload construction remains gated by
`SEMANTIC_RELEASE_QUALIFIED == 'true'`; neither semantic release variable is changed here.
Dispatch catalogs use the reserved `qualification.invalid` host and desktop installer jobs are
disabled for dispatch. This makes the non-publication boundary explicit rather than producing
installers whose catalog URLs cannot resolve.

### First private dispatch evidence

Workflow run
[`34475811458`](https://github.com/erikvullings/procyon/actions/runs/34475811458) exercised commit
`5eb184809796439fcdfd88231ad201fbc0850e0a` on 2026-09-10. The prerelease, semantic publication,
Homebrew, and Chocolatey jobs were all skipped; no public release or semantic asset was created.
The existing macOS, Windows, and Linux base-installer jobs completed successfully without embedding
a semantic catalog.

The semantic payload result was not artifact evidence:

- Linux x86-64, Linux arm64, and Windows x86-64 reached `Build verified semantic release payloads`
  and failed immediately because pnpm forwarded its conventional argument separator as a literal
  `--`. The bundle parser rejected it before any runtime/model download or build.
- macOS arm64 acquired no runner and executed no steps because `macos-14-xlarge` is unavailable to
  this repository.

The follow-up accepts one leading package-manager separator, covers it with a deterministic test,
and uses the standard `macos-15` arm64 runner label also used by upstream Zvec. Manual dispatch now
runs only the private semantic payload, signing, catalog, and Actions-artifact assembly path.

### Second private dispatch evidence

Workflow run
[`34480698440`](https://github.com/erikvullings/procyon/actions/runs/34480698440) exercised clean
commit `1f2024c81d32193fc03c65c71680fbe387d9e030`. Every prerelease, desktop-installer, semantic
publication, Homebrew, and Chocolatey job was skipped.

Windows x86-64 and Linux arm64 completed archive/checksum/member/architecture/dependency
verification, built with `ZVEC_AUTO_BUILD=0`, passed the cache-hidden packaged argument-parser
check, passed the isolated packaged-worker protocol handshake and offline model activation, passed
component lifecycle tests, and retained private payload artifacts:

| Target | Runtime artifact | Worker artifact | Trust status |
| --- | --- | --- | --- |
| Windows x86-64 | `procyon.semantic.zvec-runtime.windows-x86_64.0.7.0.3745106b3beee6be` | `procyon.semantic.worker.windows-x86_64.0.1.0.24.c4f491b1267419ac` | Unsigned, notarization not applicable |
| Linux arm64 | `procyon.semantic.zvec-runtime.linux-aarch64.0.7.0.621af6ba8249ce44` | `procyon.semantic.worker.linux-aarch64.0.1.0.24.d6d01578de0aa7fd` | Platform signing/notarization not applicable |

Both use model artifact
`procyon.semantic.model.multilingual-e5-small.1.0.0.c6a9b539cad7f507`. The retained attestations
record a clean source tree and the expected `dumpbin`/`readelf` dependency sets.

macOS arm64 reached the final worker link but failed because the semantic matrix omitted the
`brew install lld` step required by the repository's `.cargo/config.toml`; the configured
`/opt/homebrew/opt/lld/bin/ld64.lld` therefore did not exist. The follow-up installs that exact
linker before building.

Linux x86-64 also reached the final worker link. Zvec itself had already passed pinned archive,
loader, x86-64 architecture, and `readelf` dependency verification. The pinned ONNX Runtime rc.13
static archive then failed to link on the required Ubuntu 22.04 baseline because it references
glibc 2.38 `__isoc23_strtol`, `__isoc23_strtoll`, and `__isoc23_strtoull` plus newer libstdc++
`basic_string::_M_replace_cold` symbols. Linux arm64 succeeds on Ubuntu 24.04. Building x86-64 on
Ubuntu 24.04 would conceal the desktop's Ubuntu 22.04 compatibility gap, so this remains an exact
production blocker rather than an unsupported compatibility claim.

### Third private dispatch evidence

Workflow run
[`34482628513`](https://github.com/erikvullings/procyon/actions/runs/34482628513) exercised clean
commit `a5cadd9a3848137d4cab614a5c0d0362a3c229fc`. Windows x86-64 and Linux arm64 again completed
and retained their payloads; Linux x86-64 reproduced the same ONNX Runtime/Ubuntu 22.04 link
blocker. Every public release, installer, Homebrew, and Chocolatey job remained skipped.

macOS arm64 passed the pinned runtime verification, worker build, Developer ID signing, codesign
verification, Apple notarization submission, accepted notary result, submitted-ZIP byte binding,
and qualification-record update. Its signed content-addressed IDs were
`procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.69abaed8e9309eeb` and
`procyon.semantic.worker.macos-aarch64.0.1.0.24.73e5d803bfd2fe32`. The job then failed before smoke
and artifact retention because `spctl --assess --type execute` reports a standalone CLI as “code
is valid but does not seem to be an app.” Gatekeeper app-bundle assessment is not applicable to
these raw optional payloads; the follow-up removes that invalid check while retaining strict
codesign verification and the artifact-bound accepted Apple receipt.

The attempted dispatch-only continuation did not make the catalog matrix eligible because the
matrix dependency still had a failed result. It is removed in the final workflow so a supported
target failure remains visibly red. Successful per-target payload artifacts are retained before
the aggregate failure; signed catalogs still require the complete supported matrix.

### Final private dispatch evidence

Workflow run
[`34483603909`](https://github.com/erikvullings/procyon/actions/runs/34483603909) exercised clean
commit `fa70ff3faa3974c64e2d12e45a205856c27303de`. macOS arm64, Windows x86-64, and Linux arm64
all completed payload build, offline/package smoke, lifecycle tests, and private artifact upload.
Linux x86-64 reproduced the documented ONNX Runtime/Ubuntu 22.04 link blocker. Prerelease, desktop
installer, catalog, semantic publication, Homebrew, and Chocolatey jobs were all skipped; no public
asset or release was created.

The final macOS content-addressed IDs are
`procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.77431044a055f64c` and
`procyon.semantic.worker.macos-aarch64.0.1.0.24.ae2092aae393df7f`. Both passed strict Developer ID
verification. Apple accepted the ZIP containing those exact bytes, the recorder bound its digest
and submission ID to both artifact hashes, and the production-trust smoke revalidated that record
before the packaged protocol/model/component tests.

| Retained macOS evidence | Value |
| --- | --- |
| Signed runtime bytes / SHA-256 | 23,164,624 / `77431044a055f64c359d8c70614d86d15c16f636b94a23e7afb3618c06f2d36c` |
| Signed worker bytes / SHA-256 | 38,411,504 / `ae2092aae393df7f05e8013cee470e8ca4addd68615a74e580d263b8ccfda754` |
| Apple notary submission | `2a374034-7207-4187-b3ff-ae4f720caa32` / `Accepted` |
| Submitted ZIP bytes / SHA-256 | 20,883,412 / `73b452811684e8df5b5add22a9267c24a77d30affe0ec0d4f3bb208165524c94` |
| Source revision / tree | `fa70ff3faa3974c64e2d12e45a205856c27303de` / clean |

### Linux ABI remediation dispatch evidence

Private workflow run
[`34509441435`](https://github.com/erikvullings/procyon/actions/runs/34509441435) exercised clean
commit `39d9d1077f8ed4374f991bb85259fcdd79410fa8` on 2026-09-10. All four supported payload jobs
passed. Prerelease creation, desktop installers, aggregate catalog signing, semantic publication,
Homebrew, and Chocolatey were skipped; no public release or asset was created. Both
`SEMANTIC_RELEASE_QUALIFIED` and `KNOWLEDGE_SEARCH_RELEASE_QUALIFIED` remained absent.

Linux x86-64 ran on Ubuntu 22.04 and passed the pinned archive, source revision, license, notice,
ELF architecture, SONAME, exact dependency, and ABI-ceiling checks. Cargo linked the worker
dynamically to the verified Microsoft loader without enabling `ort`'s download or copy-dylib
features. The build then hid both native caches, launched the exact packaged worker with only the
content-addressed ONNX and Zvec bytes restored under their loader names, reached its parser,
completed the protocol handshake, activated the production model offline, and passed the complete
`fm-semantic-components` lifecycle suite. The downloaded private payload was independently
reverified against both qualification records.

| Linux x86-64 retained evidence | Value |
| --- | --- |
| Private Actions artifact | ID `10165415845`, 303,780,936 bytes, SHA-256 `c799240a76d83862767850e6c0ba5a05aaf3fa74ddbddceea7abc4be3ad6466c` |
| Catalog revision | `procyon-linux-x86_64-0.1.0-24-5e51c2e39dbb4f3c8d8df785be841dd3` |
| Worker | `procyon.semantic.worker.linux-x86_64.0.1.0.24.baf8a9db9cab9c16`, 15,144,128 bytes, SHA-256 `baf8a9db9cab9c167aa13112088aaf6d627c67ce12e4957f6266078c40352a7b` |
| ONNX Runtime | `procyon.semantic.onnx-runtime.linux-x86_64.1.28.0.1461ef7cc3d9e499`, 24,268,848 bytes, SHA-256 `1461ef7cc3d9e49982591721683cc3e3a55580aeca9a5254e7aac47b75ee4bab` |
| Zvec runtime | `procyon.semantic.zvec-runtime.linux-x86_64.0.7.0.89eac719eb426a20`, 36,854,864 bytes, SHA-256 `89eac719eb426a2066d2104e5b1199aa83ec18eaa4c31c7797b9bf469904cfd5` |
| Model pack | `procyon.semantic.model.multilingual-e5-small.1.0.0.c6a9b539cad7f507`, 487,353,895 bytes, SHA-256 `c6a9b539cad7f507e4f09b172a93f47f51c598f7377581792a23bf74fbdd80b3` |

The other target paths also remained healthy. macOS arm64 retained worker
`procyon.semantic.worker.macos-aarch64.0.1.0.24.d8780bacc61653f9` (38,410,336 bytes, SHA-256
`d8780bacc61653f9cb3850aeae99891754fdf53b175cc110f6b6e9813e009aad`) and signed Zvec runtime
`procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.a4d3635cbe5dbc68` (23,164,624 bytes,
SHA-256 `a4d3635cbe5dbc6818af08feaa51c64a92bbe550bc469f887eb3f0cdabdf3ab2`). Apple accepted
submission `9fba1cf3-e93e-46a3-9d23-1c7b68e928f0`, bound to a 20,882,722-byte ZIP with SHA-256
`329e869f1f59383316ad5736ffea4657eca338e047e6dc12ebf42aff68f9ca36`. Windows x86-64
retained unsigned worker `procyon.semantic.worker.windows-x86_64.0.1.0.24.30975f6ad8f49d1c`
and Zvec runtime `procyon.semantic.zvec-runtime.windows-x86_64.0.7.0.3745106b3beee6be`.
Linux arm64 retained worker `procyon.semantic.worker.linux-aarch64.0.1.0.24.4bf8a63c58d300f8`
and Zvec runtime `procyon.semantic.zvec-runtime.linux-aarch64.0.7.0.621af6ba8249ce44`.

### Exact-production evaluation dispatch evidence

Private workflow run
[`34636435645`](https://github.com/erikvullings/procyon/actions/runs/34636435645) exercised
clean revision `1680eebc0864cde494816285a24d6e0378678de1`. All four supported payload jobs
passed package construction, platform trust checks, cache-hidden protocol/model smoke, component
lifecycle and privacy tests, the generated task-0188 corpus, case-variant equality checks, and
private artifact upload. Catalog signing and installed-package continuation failed closed because
the protected signing secret and verifying-key variable remain unavailable. Every prerelease,
installer, semantic-publication, Homebrew, and Chocolatey job was skipped. Both release-qualified
repository variables were absent before dispatch.

| Target | Private Actions artifact | Catalog revision | Worker | Native runtime |
| --- | --- | --- | --- | --- |
| macOS arm64 | `10278167897`, SHA-256 `fc4225aae56bb89f305f76a30cdd654789ef5c5c86a3f7b92e94aeeff3be49ef` | `procyon-macos-aarch64-0.1.0-25-24f657849ad0b0d7b4809dab9ceac269` | `procyon.semantic.worker.macos-aarch64.0.1.0.25.7562bb74a14aac02` | `procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.6c11e76150de84a5` |
| Windows x86-64 | `10279205804`, SHA-256 `fbc59476f165c9df458d3fc582ac5a5966db03740165bf55502d4380238601bc` | `procyon-windows-x86_64-0.1.0-25-c95c566e92f274441279085150e42115` | `procyon.semantic.worker.windows-x86_64.0.1.0.25.966f3ea63d4cba4b` | `procyon.semantic.zvec-runtime.windows-x86_64.0.7.0.3745106b3beee6be` |
| Linux x86-64 | `10278119285`, SHA-256 `d37959037d20ebed913fc1aafd93b21631b975049080bfefb16416b1de4dd6ef` | `procyon-linux-x86_64-0.1.0-25-ece6702f31d6dc036cc6dcf591d9e956` | `procyon.semantic.worker.linux-x86_64.0.1.0.25.f4e3bda772147ce3` | Zvec `procyon.semantic.zvec-runtime.linux-x86_64.0.7.0.89eac719eb426a20`; ONNX `procyon.semantic.onnx-runtime.linux-x86_64.1.28.0.1461ef7cc3d9e499` |
| Linux arm64 | `10278233631`, SHA-256 `500fec169bf4b67f6102956fd16a51762031ef0f8dc96160907d0e2fdb12fd3d` | `procyon-linux-aarch64-0.1.0-25-228b265a24572a2b07db8a5e707090ce` | `procyon.semantic.worker.linux-aarch64.0.1.0.25.cb6d1bb16aa83f95` | `procyon.semantic.zvec-runtime.linux-aarch64.0.7.0.621af6ba8249ce44` |

Every target used model artifact
`procyon.semantic.model.multilingual-e5-small.1.0.0.c6a9b539cad7f507` (SHA-256
`c6a9b539cad7f507e4f09b172a93f47f51c598f7377581792a23bf74fbdd80b3`), corpus
fingerprint `sha256:6e0c201a6393bdfad56999ecfa86d6b66387c8aa41c8809055bf14429755f3ca`,
and release-candidate fingerprint
`sha256:1a39813e8f05450174a592d38c9331b4bbd478932baf1019d87c1779a7801e86`.
The four reports were downloaded and revalidated locally into a private aggregate with SHA-256
`e5da4ac20368da8376775a8b9b4f6b6411dc338c788efa5f487b66a9f4fbf0d8`.
No query, excerpt, or source text is present in those reports.

## Evidence status

| Gate | macOS arm64 | Windows x86-64 | Linux x86-64 | Linux arm64 |
| --- | --- | --- | --- | --- |
| Signed production payload/catalog retained | Developer ID signed and Apple-notarized private payload retained; signed catalog missing | Unsigned private payload retained; signed catalog missing | Private payload retained; signing not applicable; signed catalog missing | Private payload retained; signed catalog missing |
| Packaged worker handshake, offline activation, and crash/restart | Pass in combined run `34636435645` | Pass in combined run `34636435645` | Pass on Ubuntu 22.04 in combined run `34636435645` | Pass in combined run `34636435645` |
| Exact task-0188 retrieval evaluation | Packaged retrieval pass in run `34636435645`; specialized/manual gaps remain | Packaged retrieval pass in run `34636435645`; specialized/manual gaps remain | Packaged retrieval pass in run `34636435645`; specialized/manual gaps remain | Packaged retrieval pass in run `34636435645`; specialized/manual gaps remain |
| Installed/absent and first-run | Not run | Not run | Not run | Not run |
| Upgrade and rollback | Not run | Not run | Not run | Not run |
| Corruption, offline, and low-disk | Not run | Not run | Not run | Not run |
| Cancellation and crash/restart | Not run | Not run | Not run | Not run |
| Uninstall retention and deletion | Not run | Not run | Not run | Not run |
| Keyboard and screen reader | Not run | Not run | Not run | Not run |
| Consent, progress, error, citation opening, deletion | Not run | Not run | Not run | Not run |
| Packaged worker/default evidence privacy scan | Pass in combined run `34636435645`; installed app not run | Pass in combined run `34636435645`; installed app not run | Pass in combined run `34636435645`; installed app not run | Pass in combined run `34636435645`; installed app not run |

macOS x86-64 is unsupported because Zvec 0.7.0 has no matching runtime. The universal macOS desktop
application must continue to report semantic functionality as unavailable on that architecture.
Windows installers and payloads remain unsigned under the existing desktop release policy; that
limitation must be presented in release notes and must not be mistaken for a qualified signed
artifact.

## Automated installed lifecycle and privacy matrix

Task 0219 adds a private `workflow_dispatch` continuation to the existing release workflow. The
continuation cannot start until `scripts/check-semantic-qualification-workflow.mjs` proves that no
dispatch-reachable step can publish and that both `SEMANTIC_RELEASE_QUALIFIED` and
`KNOWLEDGE_SEARCH_RELEASE_QUALIFIED` are absent or exactly `false`. Qualification jobs have
read-only repository permission. They upload seven-day Actions artifacts only; they do not create
or modify a GitHub Release, Homebrew tap, Chocolatey package, repository variable, or public asset.

The matrix preserves the production target contract:

| Qualification target | Runner and package boundary | Trust statement |
| --- | --- | --- |
| macOS arm64 | `macos-15`; signed app inside the generated DMG, copied from a mounted image and launched from an isolated user-data root | Developer ID signing and Apple notarization remain mandatory |
| Windows x86-64 | `windows-latest`; generated MSI installed silently, launched, and uninstalled with retained MSI logs | Intentionally unsigned under the current release policy |
| Linux x86-64 | `ubuntu-22.04`; generated DEB extracted and AppImage expanded, each launched under Xvfb | Ubuntu 22.04 remains the ABI baseline |
| Linux arm64 | `ubuntu-24.04-arm`; generated DEB and AppImage boundaries launched under Xvfb | Signing is not applicable |

Each target consumes the exact private payload and signed catalog produced earlier in the same
workflow. `qualify_semantic_lifecycle` verifies the detached signature and complete payload set,
then exercises absent state, low disk before any artifact read, an interrupted local artifact read
and byte-range resume, first install without network access, application-manager restart, installed
payload tampering and recovery, serialized concurrent lifecycle access, uninstall with retention,
explicit derived-data deletion, and clean reinstall. Catalog tampering and streamed payload
tampering fail before activation. The packaged worker test ingests and queries through the exact
worker/runtime/model, forcibly terminates that worker, reconnects to a new process, and verifies
the durable index after restart.

Every retained report uses schema version 1 and records target, source revision, runner/OS, exact
catalog and package digests, stage, sanitized command, result, redacted evidence path, and rollback
instruction. Result values are restricted to:

| Result | Meaning |
| --- | --- |
| `pass` | Automated evidence completed against the named bytes and host |
| `manual-required` | Native human interaction or assistive technology is required |
| `unsupported` | The platform or product intentionally has no such boundary |
| `blocked` | Required exact input or approval was unavailable |
| `fail` | An automated stage or privacy check failed |

The privacy runner creates unique values for query, excerpt, filename/path, prompt, response,
credential, token, authorization header, and model-payload categories. Values pass through the
packaged worker's normal ingest/query path and representative failure/restart path. Collected
application, worker, installer, diagnostic, and command evidence is scanned as bytes for UTF-8,
UTF-16LE/BE, slash and backslash paths, percent encoding, JSON escaping, base64/base64url, and
hex, including matches split across read boundaries. The report contains only category names and
category-bound SHA-256 fingerprints. Symlinks, unreadable inputs, changed/missing/added evidence,
and unscoped or expired capture fail closed. Evidence files containing a canary are removed before
failure reports can be uploaded. An authorized diagnostic capture must be previewed,
category-scoped, no longer than 15 minutes, valid at scan time, and is deleted before evidence is
retained.

Forced termination does not guarantee an operating-system crash dump, and Procyon has no automatic
semantic updater. Those rows are `unsupported`, not fabricated passes. If a platform produces a
crash artifact or installer log, place it under the collected evidence root before scanning.
Upgrade and rollback remain `blocked` until an exact signed preceding production candidate is
provided; a fixture or developer bundle is not acceptable production evidence.

### Automated payload and privacy evidence

Private workflow run
[`34624618891`](https://github.com/erikvullings/procyon/actions/runs/34624618891) exercised clean
commit `1fb6f144eb977468ea0335de8e3f0ab4421a5ae3`. Its safety job proved both release variables
absent and no dispatch-reachable publication path. All four payload jobs passed exact packaged
worker startup, offline model activation, ingestion/query, forced crash/restart with durable
reopen, corruption/rebuild tests, component lifecycle tests, and the nine-category privacy scan.
Every prerelease, base-installer, public semantic publication, Homebrew, and Chocolatey job stayed
skipped.

| Target | Actions artifact | Compressed bytes / SHA-256 | Catalog revision | Worker |
| --- | --- | --- | --- | --- |
| macOS arm64 | `10273976490` | 297,030,751 / `cedcd7ac903673b90ce2521596c6911cc9a2e38f9976347433380da946838a20` | `procyon-macos-aarch64-0.1.0-25-efeffd56ee755a343b51687d513518bd` | `procyon.semantic.worker.macos-aarch64.0.1.0.25.c7d4446365a10c7a` / `c7d4446365a10c7a8bc6b982747572597a650c9221c2d3f7365d515027086c2a` |
| Windows x86-64 | `10273787545` | 296,523,134 / `0a96382b070c8d3fe917b4bfa8dd5c8a45b74225a1cfd73866bf8429e6e8c912` | `procyon-windows-x86_64-0.1.0-25-c1e6d375f4703fde634e00612e0a44bd` | `procyon.semantic.worker.windows-x86_64.0.1.0.25.64d114762cbbc184` / `64d114762cbbc18420b5311680a3b37985770cb6f200a0a551d48167ed0bc5a3` |
| Linux x86-64 | `10274656240` | 303,795,376 / `cc1119b9aab6da139c119e497bee50c33be2cb73ba1ef15f75fbb9b7584d76b3` | `procyon-linux-x86_64-0.1.0-25-11ce94ec411b60021c63ddd0cc13a56b` | `procyon.semantic.worker.linux-x86_64.0.1.0.25.1d06fc6e7055de49` / `1d06fc6e7055de49d2084963450dc070cdf7ad51f7e5020d1198273dca7578d5` |
| Linux arm64 | `10273661828` | 303,506,504 / `472ed9a0126b4286eba7aa698847aac041dfedd4d890d470913dd9f353bd6793` | `procyon-linux-aarch64-0.1.0-25-1b2601b2f7c8a71439372024db1bbcf5` | `procyon.semantic.worker.linux-aarch64.0.1.0.25.e68f99ed5b2344a6` / `e68f99ed5b2344a6f2bc059c11b40da046b0eef22328df32b2fe54208c358b74` |

Every target uses model artifact
`procyon.semantic.model.multilingual-e5-small.1.0.0.c6a9b539cad7f507`
(`c6a9b539cad7f507e4f09b172a93f47f51c598f7377581792a23bf74fbdd80b3`).
Zvec runtime digests are `2df52a19cc43a012592e39e6d34f71f6f15b0804e8cade0b2d960bb382e040ff`
(macOS), `3745106b3beee6be2d50ca678b46d3f0289afb5136ff51e1d1ec037a27b29e4a`
(Windows), `89eac719eb426a2066d2104e5b1199aa83ec18eaa4c31c7797b9bf469904cfd5`
(Linux x86-64), and
`621af6ba8249ce44dc17fb05da6c51c723cc466843e7f46ee44a40bd7eee1169`
(Linux arm64). Linux x86-64 additionally used ONNX Runtime
`1461ef7cc3d9e49982591721683cc3e3a55580aeca9a5254e7aac47b75ee4bab`.

The privacy result is **PASS** for every retained payload. Reports contain only the nine category
names and category-bound hashes; no canary values are retained. The artifacts expire after seven
days on 2026-09-18.

Catalog signing then failed closed on every target because protected
`SEMANTIC_CATALOG_SIGNING_KEY_BASE64` and `SEMANTIC_CATALOG_VERIFYING_KEY_BASE64` are not
configured. Consequently no signed catalog or private catalog-enabled installer exists for this
candidate, and installed package rows remain `blocked`. This is not a waiver: configure both
matching protected values and rerun the same commit before performing installed and manual passes.

### Manual operator checklist

Automation does not establish native accessibility or comprehension. For each retained package,
record operator, date, hardware, OS build, package/catalog digests, assistive-technology version,
result, defect link, and evidence location for:

1. VoiceOver on macOS, Narrator on Windows, and Orca on Linux: application structure, dialog
   title, control name/role/state, status updates, errors, result evidence, and citations.
2. Keyboard-only install consent, settings entry, enrolment-tree navigation, model choice,
   progress cancellation, error recovery, citation opening, focus restoration, retention choice,
   explicit deletion, and uninstall.
3. Consent and privacy wording at default zoom and 200%, light/dark/high-contrast settings, and
   reduced motion.
4. Screen-reader announcement of download/storage estimates, offline or low-disk failure,
   cancellation, worker restart, unavailable source, and deletion completion.
5. Confirmation that retained-index uninstall and explicit-delete uninstall communicate different
   outcomes and that a clean reinstall/rebuild remains understandable.

Use `docs/architecture/accessibility.md` for the general interaction procedures. A checklist row
remains `manual-required` until a human records actual results from the native packaged build.

The upstream slim runtime archives do not carry native Zvec's `LICENSE` or `NOTICE` files. The
catalog and qualification record therefore preserve immutable URLs, byte lengths, and SHA-256
digests for both Apache licenses and the native NOTICE, including its Unicode Character Database
and pyglass attributions. Publication remains NO-GO until the retained production outputs are
reviewed as part of the complete task-0198 evidence set.

## Quality and threshold decision

The checked-in task-0188 fixture defines 12 cases and repository-owned generated source documents
spanning multilingual retrieval, duplicates, boilerplate diversity, structural citations, edits,
summaries, scope isolation, unavailable-source discovery, concepts, multi-facet retrieval, a
negative control, and prompt-injection-shaped input. The release payload matrix now runs that
corpus through the exact packaged worker/runtime/model/converter/chunker/Zvec path and retains
opaque per-case evidence plus file recall@10, chunk recall@10, MRR, nDCG@10,
negative-control false-positive rate, and citation metrics. Run `34636435645` measured the same
results on all four targets: file recall@10 `0.958333`, chunk recall@10 `0.958333`, MRR `0.916667`,
nDCG@10 `0.967762`, negative-control false-positive rate `0`, offline citation correctness `1.0`,
and offline citation recall `0.923077`. Generated-answer citation correctness remains unmeasured.
The private aggregate is **NO-GO**, and the checked-in report remains a fail-closed template.

The developer TRIZ observations (`0.905` for `Su-fields`, `0.881` for the cup/hot-liquid question,
and `0.848`-`0.850` for hard unrelated controls) do not justify lowering the current `0.84`
absolute floor. The evaluator therefore retains `0.84` with a `0.02` relative window and records
zero storage or migration impact attributable to the threshold decision. Any future threshold
change requires comparable before/after production observations and an explicit storage/migration
statement.

The candidate now case-folds both passage and query text before adding the model's role prefix.
Original text remains unchanged for display, citation, and full-text search. This intentionally
changes the embedding space from `preserve-case/1` to `unicode-default-case-fold/1`: the worker's
active-index marker and library manifest force a clean derived-index rebuild, while steady-state
storage policy is unchanged. Reviewed exact-production before/after evidence is not yet attached,
so this migration is an additional explicit NO-GO reason rather than an inferred improvement.

The bounded multi-query candidate remains developer-only and defaults to single-query because its
checked-in report uses normalized fixture timings rather than production measurements.

### Local exact-package dry run

An unsigned, dirty-tree macOS arm64 dry run exercised the exact content-addressed release worker,
model pack, Zvec loader, converter, chunker, native index, and application Ask packing path. It is
retained privately with SHA-256
`65c7d7a06d034460fb0c359937b92ed930d9817721d98840baadc82898d8ec70` and is **not**
production evidence. It measured file recall@10 `0.958333`, chunk recall@10 `0.958333`, MRR
`0.916667`, nDCG@10 `0.967762`, negative-control false-positive rate `0`, offline citation
correctness `1.0`, and offline citation recall `0.923077`. Generated-answer citation metrics remain
unmeasured. The report also correctly blocks the generated-summary, unavailable-source, and
concept-label cases because their specialized production setup was not exercised.

The dry-run catalog revision was
`procyon-macos-aarch64-0.1.0-25-30196fb3aaca0290032cfd360b386bbf`; artifact IDs were
`procyon.semantic.worker.macos-aarch64.0.1.0.25.7840c5049f2f08f9`,
`procyon.semantic.zvec-runtime.macos-aarch64.0.7.0.c9e4bf9387ef7261`, and
`procyon.semantic.model.multilingual-e5-small.1.0.0.c6a9b539cad7f507`. The run retained the
`0.84`/`0.02` thresholds and declared the case-folded embedding index rebuild separately.

## Existing automated evidence

Repository tests already cover deterministic metric calculation, scope isolation, bounded
retrieval, catalog and payload integrity, interrupted download/resume, component lifecycle,
low-disk admission, cancellation/recovery, deletion/retention, OCR process containment, and
default-safe semantic diagnostics. The task-0188 report validator additionally rejects corpus or
implementation drift, missing/duplicate/unknown cases, malformed ranks and citations, scope
leakage, package identity drift, threshold changes, stale metrics, and forged decisions. Default
diagnostic events have no free-text field and reject path-like values; sensitive capture requires
a previewed, scoped, expiring grant.

These tests reduce qualification risk but do not satisfy installed release testing, native
assistive-technology review, packaged crash-report inspection, or exact production quality
measurement.

## Required qualification run

1. From one immutable commit, dispatch the semantic payload matrix for all supported targets and
   retain each unsigned input manifest, signed catalog, signature, payload, and installer digest.
2. Verify every catalog signature and artifact checksum, then run the packaged worker handshake,
   offline model activation, and component lifecycle smoke tests against those retained bytes.
3. Build a clean evaluation library from the task-0188 corpus with the candidate identity above.
   Capture ranked file and chunk observations at cutoff 10 and grounded Ask citation outcomes.
   Publish the aggregate metrics and per-case failures without publishing query or source text.
4. Run the lifecycle and failure matrix in this report on clean supported hosts. Repeat after an
   upgrade from the preceding production candidate and after rollback.
5. Perform keyboard and native screen-reader passes (VoiceOver, Narrator, and Orca), including
   consent, progress, cancellation, error recovery, citation opening, and deletion.
6. Inspect default application logs and crash artifacts from each failure scenario for queries,
   excerpts, filenames, prompts, responses, credentials, tokens, and model payload content.
7. Record operator, date, hardware, OS, installer/catalog/artifact digests, results, and defects in
   this report. All rows must be Pass; waivers require an explicit release-owner decision.
8. Set `SEMANTIC_RELEASE_QUALIFIED` to `true` only after approval, dispatch the release workflow,
   and verify each produced installer embeds the matching published catalog and public key.

## Rollback

If any post-qualification regression occurs, set `SEMANTIC_RELEASE_QUALIFIED` to `false` before the
next release. This stops semantic payload/catalog publication and produces ordinary desktop
installers without managed semantic activation. Existing installations may use the component
manager's signed rollback action; retain enrolment policy unless the user explicitly requests
index deletion. Never repoint a signed catalog artifact URL or replace immutable payload bytes.
