# 0198 Semantic release qualification

Status: in_progress
Priority: high
Subsystem: quality, release
Depends on: 0195, 0196

## Context

Qualify the first opt-in semantic distribution for an explicitly experimental Procyon OSS alpha.
Feature-level tests and the macOS arm64 developer corpus are not substitutes for installed
artifact, cross-platform automation, retrieval-quality, privacy, and failure-mode evidence.
Production/stable qualification continues in task 0225.

## Acceptance Criteria

- Run the task-0188 labelled evaluation suite against the exact production model, converter,
  chunker, retrieval policy, and index identity; record file/chunk recall, MRR, nDCG, and negative
  controls. Generated-answer grounding remains deferred to production/stable task 0225 for the
  first experimental alpha.
- Calibrate absolute and relative Ask similarity thresholds from the labelled report. Do not lower
  the current `0.84` absolute floor merely to increase result count. Record rebuild and storage
  impact for the case-fold migration; its reviewed before/after quality comparison remains deferred
  to production/stable task 0225 for the first experimental alpha.
- Complete automated installed/absent, first-run, corruption, offline, low-disk, cancellation,
  crash/restart, and deletion/retention tests on supported macOS, Windows, and Linux release builds.
- Complete an installed upgrade/rollback and manual keyboard, VoiceOver, consent, progress, error,
  citation-opening, and data-deletion pass on macOS arm64 using an isolated OS user account.
- Record Windows and Linux manual accessibility/UX as explicitly untested in the alpha report and
  release notes; these remain mandatory for production/stable qualification under task 0225.
- Confirm default logs and crash reports contain no query, excerpt, filename, prompt, response,
  credential, token, or model payload content.
- Publish an operator-readable qualification report identifying exact artifact/catalog versions,
  known limitations, unsupported targets, and rollback instructions.
- Encode the experimental-alpha evidence policy in the fail-closed validator so deferred rows
  cannot be represented as passes or accidentally satisfy a stable-release policy.
- Only after all alpha gates pass, enable the production catalog in the normal desktop release
  workflow and verify the produced installers against the published catalog.

## Implementation Notes

- This is the gate for a user-facing semantic release, not for merging the implementation branch.
- Deterministic Docling needs no separate package, but its exact converter identity belongs in the
  evaluation fingerprint.
- OCRmyPDF qualification is added when 0197 ships and does not block semantic search/Ask for
  searchable documents.

## Experimental OSS alpha qualification checklist

These are the blocking gates for the first public, opt-in semantic alpha. Keep both
release-qualified repository variables absent or `false` until the final approval group.

### A. Make private installer testing practical and honest

- [x] Produce a private macOS qualification kit that provisions the exact signed production
  payloads for the catalog-enabled release app without public publication, developer-bundle
  overrides, embedded secrets, or weakened release checks.
- [x] Make the kit operate only in a dedicated macOS test user's application-data directory and
  provide a targeted cleanup command for that profile.
- [x] Add an explicit `experimental-alpha` evidence policy to the report schema and fail-closed
  validator. It must require four-platform automated PASS, macOS arm64 manual PASS, release-owner
  approval, and documented Windows/Linux manual limitations; it must not interpret deferred rows
  as passed or permit the same report to qualify a stable release.
- [x] Add regression tests for kit integrity/target matching, alpha-policy validation, publication
  isolation, and cleanup containment.

### B. Prepare one immutable alpha candidate

- [x] Merge the qualification tooling and commit `fd775a2` into `main`, prepare the next numeric
  alpha version, refresh generated evaluation fingerprints, and require normal CI to pass on the
  clean candidate revision.
- [x] Use the public base-only v26 installer as the user-visible upgrade/rollback baseline. A prior
  semantic-to-semantic candidate is not required for the first public semantic alpha.
- [x] Confirm catalog signing, verifying-key, and Apple signing/notarization configuration remains
  available while both release-qualified variables remain absent or exactly `false`.

### C. Run the private four-platform matrix

- [x] Run `pnpm run semantic:qualification:check`.
- [x] Dispatch `gh workflow run release-desktop.yml --ref main` from the immutable candidate and
  record the run ID.
- [x] Require qualification safety, all four payloads, all four signed catalogs, all four automated
  installed lifecycle/privacy jobs, and private aggregation to pass. All public/package-manager
  jobs must remain skipped.
- [x] Download every private artifact before its seven-day expiry and independently verify source
  revision, target, signature, checksum, package, catalog, model, runtime, converter, chunker, and
  retrieval-policy identities.
- [x] Review the exact four-target retrieval metrics and negative controls. Keep the `0.84`
  absolute floor and `0.02` relative window unless measured evidence justifies a change.

### D. Perform the macOS installer pass without a VM

- [x] Use either a separate standard macOS account or the current account with the helper-managed
  isolated profile and every ordinary Procyon instance closed. Never launch the candidate against
  the operator's normal `~/Library/Application Support/fm` profile.
- [x] Install/launch public base-only v26 once, then install the private alpha DMG over it. Back up
  the existing application first when using the current account. Verify DMG checksum, signature,
  notarization, stapling, app launch, and preservation of ordinary settings/workspaces.
- [ ] Provision components with the private qualification kit and exercise consent, estimates,
  activation, folder enrolment, indexing progress, cancellation/resume, restart, Semantic Search,
  Ask Your Files, citations, negative controls, and offline/error behavior.
- [x] Verify worker restart, corrupt-component rejection/recovery, retained-data uninstall,
  explicit-delete uninstall, reinstall, and rollback to the base-only installer.
- [ ] Complete keyboard-only and VoiceOver checks for control names/roles/states, status/error
  announcements, citation opening, focus restoration, and deletion completion. Check critical
  consent/error screens at 200% zoom.
- [ ] Inspect default app/worker/installer logs, Diagnostics output, and available crash artifacts
  for query, excerpt, filename/path, prompt, response, credential, token, authorization-header, or
  model-payload leakage.
- [ ] Record operator, date, hardware, macOS build, VoiceOver version, exact digests, results,
  defects, and private evidence location.

### E. Approve and publish the experimental alpha

- [x] Fix every alpha-blocking defect and rerun affected automated/manual rows on one immutable
  candidate.
- [x] Update the qualification report and checked-in opaque evaluation evidence with four exact
  production measurements, macOS manual evidence, the `experimental-alpha` tier, honest deferred
  Windows/Linux rows, known limitations, and rollback instructions.
- [x] Run the alpha-aware semantic precondition validator and obtain explicit dated release-owner
  approval for the named revision and artifacts.
- [ ] Qualify one independent `semantic-v*` component candidate, commit its generated fingerprint
  lock, then publish the exact retained run with `SEMANTIC_COMPONENTS_RELEASE_QUALIFIED=true`.
- [ ] Set `SEMANTIC_RELEASE_QUALIFIED=true`, tag the matching numeric alpha, and require desktop
  installers to fetch the already-published locked catalogs without rebuilding components.
- [ ] Verify the public macOS installer from the test account downloads only the published signed
  payloads and reproduces the qualified Search/Ask smoke. Verify the Windows/Linux installer
  digests and automated smoke reports.
- [ ] State prominently in release notes that semantic functionality is experimental and opt-in,
  Windows/Linux manual accessibility testing is pending, Windows artifacts are unsigned, and
  macOS x86-64 semantic runtime is unsupported.
- [ ] On a component failure, do not publish its component tag. On desktop integration failure,
  reset `SEMANTIC_RELEASE_QUALIFIED=false` and issue a base-only corrective alpha without changing
  the immutable component release.

## Deferred production/stable checklist

The production-grade checklist below is retained as the source for task 0225. Its unchecked rows do
not block the experimental OSS alpha defined above.

Keep both release-qualified repository variables absent or `false` while executing private
qualification; a private qualification run must never publish assets.

### 1. Freeze the next candidate and retain the rollback baseline

- [x] Merge commit `fd775a2` (settings preservation across upgrades) into `main` and require all
  normal CI jobs to pass.
- [x] Prepare the next numeric alpha candidate (expected `0.1.0-27`), refresh both semantic
  evaluation fingerprints changed by the version bump, and land it on a clean immutable `main`
  revision.
- [ ] Download the private run `34711114776` payloads, signed catalogs, packages, and reports
  before their seven-day retention expires. Store them in an access-controlled location outside
  the repository and record their artifact IDs, byte lengths, and SHA-256 digests.
- [ ] Designate those retained `0.1.0-25` signed artifacts as the preceding semantic candidate.
  If any required byte is unavailable or fails its recorded digest, generate and retain a new
  baseline candidate before testing the next candidate; do not substitute developer-bundle bytes.
- [x] Confirm the `desktop-release` environment still has the matching catalog signing secret,
  catalog verifying-key variable, and Apple signing/notarization credentials.
- [x] Confirm `SEMANTIC_RELEASE_QUALIFIED` and `KNOWLEDGE_SEARCH_RELEASE_QUALIFIED` are absent or
  exactly `false`.

### 2. Close the remaining qualification-tooling gaps

- [ ] Extend the private installed-qualification workflow to accept the exact retained preceding
  package/catalog/payload set and automate: preceding install, current upgrade with retained
  enrolment/index, rollback, and current reinstall. Run this on macOS arm64, Windows x86-64,
  Linux x86-64, and Linux arm64.
- [ ] Add regression tests proving the upgrade input is immutable, signed, target-matched, and
  rejected on missing bytes or digest/catalog drift.
- [ ] Produce a private manual-test kit for each platform that provisions the exact signed
  production components for the catalog-enabled release application without publishing them.
  It must not use `PROCYON_SEMANTIC_DEVELOPER_BUNDLE`, weaken release-build checks, expose signing
  material, or silently replace the production download lifecycle.
- [ ] Document a one-command cleanup for the manual-test kit that removes only its isolated test
  profile and leaves the operator's ordinary Procyon settings and semantic library untouched.
- [ ] Add exact-production setup and observations for the generated-summary, unavailable-source,
  and concept-label corpus cases currently blocked by missing specialized setup.
- [ ] Run grounded answer generation through the production Ask packing path and record
  deterministic citation correctness/recall without retaining prompts, responses, queries, or
  source content.
- [ ] Produce reviewed before/after `preserve-case/1` versus
  `unicode-default-case-fold/1` evidence on the same production corpus and artifacts. Record
  quality deltas, mandatory index rebuild behavior, and storage impact.

### 3. Generate and retain the private current-candidate installers

- [x] Run `pnpm run semantic:qualification:check` locally and confirm the workflow-dispatch graph
  has no GitHub Release, Homebrew, Chocolatey, or repository-variable mutation path.
- [x] Dispatch from the immutable candidate revision:
  `gh workflow run release-desktop.yml --ref main`.
- [x] Record the run ID and follow it with `gh run watch <run-id> --exit-status`.
- [ ] Require the safety job, all four payload jobs, all four signed-catalog jobs, all four
  installed-qualification jobs, and private aggregate collection to pass. Publication and ordinary
  release/package-manager jobs must remain skipped.
- [ ] Download all `semantic-payloads-*`, `semantic-catalog-*`,
  `semantic-installed-qualification-*`, and aggregate artifacts before their seven-day expiry.
- [x] Independently verify every signature, artifact/package checksum, target, source revision,
  model/runtime/converter/chunker identity, and catalog revision against the retained reports.
- [x] Require identical supported-target quality metrics or investigate and rerun from a new
  immutable revision. No failed or partially rerun target may be combined with another revision.
- [ ] Reduce the private evidence to the opaque checked-in aggregate/per-case format; never commit
  raw queries, excerpts, prompts, responses, filenames, or source documents.

### 4. Perform the macOS arm64 installer and manual UX pass

- [ ] Use a disposable macOS arm64 VM or separate test user. Do not test against the operator's
  existing `~/Library/Application Support/fm` data.
- [ ] Verify the DMG digest, mounted app signature, notarization, stapling, and catalog signature
  before copying the app into `/Applications`.
- [ ] Launch the installed app with the isolated private manual-test profile and provision semantic
  components through the qualification kit. Confirm the base app remains usable before components
  are installed.
- [ ] Exercise first consent, storage/download estimates, model activation, folder enrolment,
  initial indexing, progress, cancellation, resume, and restart.
- [ ] Verify Semantic Search and Ask Your Files against the qualification corpus, including
  multilingual queries, negative controls, unavailable sources, generated summaries, concept
  labels, citation opening, and focus restoration.
- [ ] Verify offline activation/error behavior, low-disk rejection, corrupt-payload rejection,
  worker crash/restart recovery, and a clean rebuild after derived-index corruption.
- [x] Verify upgrade from the retained preceding candidate, rollback, and re-upgrade without
  losing consent, enrolment policy, or source data and without reusing an incompatible index.
- [x] Verify uninstall with retained semantic data, reinstall, explicit semantic-data deletion,
  and uninstall with deletion as distinct and understandable flows.
- [ ] Complete keyboard-only navigation and VoiceOver checks for control name/role/state, dialog
  titles, status announcements, errors, citations, focus restoration, and deletion completion.
- [ ] Repeat consent and critical flows at 200% zoom, light/dark/high-contrast appearance, and
  reduced motion.
- [ ] Inspect default app/worker/installer logs, Diagnostics output, and any crash artifacts for
  query, excerpt, filename/path, prompt, response, credential, token, authorization header, or
  model-payload leakage.
- [ ] Record operator, date, hardware, macOS build, VoiceOver version, package/catalog/payload
  digests, each result, defect links, and private evidence location.

### 5. Complete the Windows and Linux native manual matrix

- [ ] Repeat the installer, semantic UX, keyboard, privacy, failure, retention/deletion, and
  upgrade/rollback checklist on Windows x86-64 with Narrator. Record that the current Windows
  installer/payload policy is unsigned as an explicit known limitation.
- [ ] Repeat it on Linux x86-64 at the Ubuntu 22.04 ABI baseline with Orca.
- [ ] Repeat it on Linux arm64 with Orca.
- [ ] Record macOS x86-64 as unsupported because Zvec 0.7.0 has no matching runtime, and verify
  the universal macOS app reports semantic functionality as unavailable on that architecture.
- [ ] Fix every release-blocking defect and rerun the affected automated and manual rows against
  one new immutable candidate. Do not waive a failure by editing the report.

### 6. Approve and publish only after every gate passes

- [ ] Update `docs/semantic-release-qualification.md` with the exact reviewed revision, artifact
  and installer identities, four-platform results, known limitations, and tested rollback steps.
- [ ] Update `docs/evaluations/semantic-production-v1.json` to contain four exact production
  measurements, completed manual criteria, case-fold comparison evidence, no blocking reasons,
  `productionMeasurement: true`, and `decision: "go"`.
- [ ] Run `pnpm run semantic:evaluation:check`; it must pass without bypasses or report edits made
  solely to satisfy the validator.
- [ ] Obtain an explicit dated release-owner approval referencing the immutable report and
  candidate revision.
- [ ] Set only `SEMANTIC_RELEASE_QUALIFIED=true`. Keep
  `KNOWLEDGE_SEARCH_RELEASE_QUALIFIED=false` unless its separate release evidence also supports GO.
- [ ] Create and push the matching numeric alpha tag. Require semantic payload/catalog publication
  and all desktop installer jobs to pass.
- [ ] On clean supported hosts, download the public release assets and verify that each installer
  embeds the matching signed catalog and public key, downloads only the published immutable
  payloads, activates Semantic Search/Ask, and reproduces the qualified smoke behavior.
- [ ] Verify release notes identify macOS x86-64 as unsupported, Windows signing limitations,
  component sizes, privacy/local-processing behavior, and rollback instructions.
- [ ] If any public verification fails, immediately set `SEMANTIC_RELEASE_QUALIFIED=false`, stop
  further package-manager rollout, retain evidence, and issue a base-only corrective release.

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
- 2026-09-10 Copilot: Private run `34509441435` passed payload construction and isolated packaged
  smoke on macOS arm64, Windows x86-64, Linux x86-64 Ubuntu 22.04, and Linux arm64. This closes the
  native Linux x86-64 construction blocker only. No signed aggregate catalog, public semantic
  asset, or catalog-embedded installer was produced, and all installed-app, task-0188 quality,
  accessibility, privacy, and failure-mode rows remain outstanding.
- 2026-09-11 Copilot: Task 0219 added a dispatch-only, read-only installed qualification
  continuation for the exact private four-target payload/catalog matrix. It builds catalog-enabled
  packages without publication, crosses DMG/MSI/DEB/AppImage boundaries, exercises the signed
  component lifecycle and packaged worker crash/restart, and scans retained evidence with unique
  sensitive canaries. Reports distinguish pass, manual-required, unsupported, blocked, and fail.
  This automation does not unblock 0198: exact production retrieval/Ask evaluation, an exact
  preceding-candidate upgrade/rollback, native VoiceOver/Narrator/Orca and keyboard UX passes, a
  completed private matrix run, and release-owner approval are still required.
- 2026-09-11 Copilot: Private run `34624618891` passed the exact packaged worker/model/runtime
  lifecycle and fail-closed privacy scan for all supported targets on commit
  `1fb6f144eb977468ea0335de8e3f0ab4421a5ae3`. During qualification, Windows exposed and fixed a
  named-pipe verification defect: Windows may normalize owner `GENERIC_ALL` to
  `FILE_ALL_ACCESS`; the verifier now accepts either full-control representation while still
  rejecting empty ACLs, deny/foreign ACEs, and foreign owners. Signed catalog and installed
  package qualification then failed closed because the protected signing secret and verifying-key
  variable are not configured. No release, public asset, base installer, Homebrew artifact, or
  Chocolatey artifact was produced. Task 0198 remains **NO-GO**.
- 2026-09-11 Copilot: Task 0220 adds the exact-production task-0188 runner and fail-closed report
  validator. The private payload matrix now ingests the repository-owned generated corpus through
  the packaged worker/runtime/model/converter/chunker/Zvec path and retains opaque per-case and
  aggregate evidence. The checked-in report remains an explicit NO-GO template, both release
  variables remain unchanged, and this task stays blocked on reviewed four-target metrics,
  generated-answer grounding, installed lifecycle, accessibility, privacy, failure-mode, and
  release-owner evidence.
- 2026-09-11 Copilot: The production candidate now applies versioned Unicode default case folding
  symmetrically to passage and query embeddings. Existing derived indexes are reset and rebuilt;
  original display/full-text content is preserved and the `0.84`/`0.02` Ask thresholds are
  unchanged. Release remains NO-GO until task 0220 records reviewed before/after production
  evidence for this embedding-space migration.
- 2026-09-11 Copilot: Rebased private run `34636435645` passed exact packaged evaluation and the
  task-0219 worker lifecycle/privacy path on macOS arm64,
  Windows x86-64, Linux x86-64, and Linux arm64 at clean revision
  `1680eebc0864cde494816285a24d6e0378678de1`. All targets measured file/chunk recall@10
  `0.958333`, MRR `0.916667`, nDCG@10 `0.967762`, zero negative-control false positives, offline
  citation correctness `1.0`, and offline citation recall `0.923077`. The private aggregate remains
  NO-GO because generated-answer grounding, three specialized corpus setups, before/after
  case-fold evidence, installed-package signing, accessibility, privacy/failure/manual gates are
  incomplete. Catalog signing and installed continuation failed closed on the known missing
  protected keys; no public or installer job ran.
- 2026-09-12 Copilot: Private run
  [`34711114776`](https://github.com/erikvullings/procyon/actions/runs/34711114776) passed signed
  catalog construction and automated installed qualification on macOS arm64, Windows x86-64,
  Linux x86-64, and Linux arm64 at
  `758672988429928b2227a7e8f1d8c8fa3adb06d7`. Every target passed exact lifecycle, diagnostic
  capture, cancellation/restart, native package launch, inherited worker privacy, and installed
  evidence privacy. All publication and base-installer jobs stayed skipped and both release
  variables remained absent. This closes task 0219's automated installed matrix, but task 0198
  remains blocked and **NO-GO**: no exact preceding production candidate was available for
  upgrade/rollback; generated-answer grounding, specialized-corpus and reviewed case-fold
  before/after evidence remain incomplete; native VoiceOver/Narrator/Orca, keyboard, consent,
  progress/error, citation, retention/deletion passes remain manual-required; and release-owner
  approval has not been recorded.
- 2026-09-14 Copilot: Added the ordered operator checklist above after the macOS developer bundle
  passed. The existing private four-target automated matrix remains valid evidence, but a
  catalog-enabled qualification installer cannot currently activate components on its own because
  dispatch catalogs intentionally use `qualification.invalid` URLs. Group 2 therefore requires a
  private exact-production provisioning kit before asking the operator to perform manual semantic
  UX checks. The task remains blocked and NO-GO.
- 2026-09-14 Copilot: The release owner selected an experimental OSS alpha bar instead of the
  production-grade checklist. Added the blocking A-E checklist: a separate macOS user replaces the
  VM requirement, four-platform automation remains mandatory, and Windows/Linux native manual
  accessibility becomes an explicit task-0225 stable-release follow-up. The validator must encode
  this tier honestly rather than treating deferred evidence as passed.
- 2026-09-14 Copilot: Prepared candidate `0.1.0-27` after landing the alpha policy and private
  macOS provisioning kit. Public prerelease `v0.1.0-26` remains the base-only rollback baseline.
  The `desktop-release` environment still exposes the semantic verifying key and catalog-signing
  secret plus all Apple signing/notarization secrets; both release-qualified variables remain
  absent. Final candidate immutability and normal CI remain pending until the candidate PR is
  merged to `main`.
- 2026-09-14 Copilot: Relaxed the macOS alpha pass to allow the current OS account when every
  ordinary Procyon process is closed and the helper-managed isolated profile is used. A separate
  account remains safer but is not mandatory; the normal application-data profile must never be
  used for qualification.
- 2026-09-14 Copilot: Completed Part B. PR #46 merged candidate `0.1.0-27` into `main` at immutable
  revision `9ed1dff617487980f4997a39e20e3fb6cd417942`; clean post-merge CI run `34836497504`
  passed dependency audit, frontend, Rust on macOS/Linux/Windows, and macOS/Windows installer
  builds. Public prerelease `v0.1.0-26` is retained as the base-only rollback baseline. Protected
  semantic catalog and Apple signing/notarization configuration is present, while both
  release-qualified variables remain absent.
- 2026-09-14 Copilot: Audited the deferred production/stable checklist against current evidence.
  Marked only the merged CI-green candidate, refreshed fingerprints, protected signing
  configuration, disabled release gates, private-dispatch safety proof, and dispatch of run
  `34853108775` from `main` revision `5ed349bba6d6a395cd50c96cfd4f132502d09880` complete.
  Predecessor artifact retention, cross-platform manual kits, upgrade/rollback automation,
  specialized/generated-answer evidence, native accessibility, and publication remain unchecked.
- 2026-09-14 Copilot: Completed Part C for private run `34853108775`, attempt 2, from immutable
  revision `5ed349bba6d6a395cd50c96cfd4f132502d09880`. Safety, all four production payloads, all four
  signed catalogs, and all four installed lifecycle/privacy jobs passed; release, semantic
  publication, Homebrew, and Chocolatey jobs remained skipped. The first Linux x86-64 attempt
  reached a signed catalog and built release binary/DEB before external `linuxdeploy` failed; the
  same-revision retry passed without mixing evidence from another candidate.
- 2026-09-14 Copilot: Retained all 12 private artifacts (4 payload, 4 catalog, 4 installed
  qualification; 4.3 GiB extracted) outside the repository at
  `qualification-run-34853108775`. Recorded byte lengths and SHA-256 values for 1,487 files in
  private manifests (`RETAINED-SHA256SUMS`
  `0507776569703044922cbacb7d22ca5abb2c479b68d9635c27f9f00683a8def2`;
  `RETAINED-FILE-SIZES`
  `553967746c90381c2ff4ad71a3bb905ca3fd6be81b95d05be269214997204fd2`).
  Independently verified every detached catalog signature and exact payload set with the protected
  public key, reconciled installed reports to catalog/signature digests, and confirmed all
  lifecycle and privacy stages passed. The retained macOS operator kit and DMG checksum, image
  integrity, Developer ID signature, notarization, and stapling also passed.
- 2026-09-14 Copilot: Local four-target aggregation succeeded with production-package identity.
  Every target recorded file/chunk recall@10 `0.958333`, MRR `0.916667`, nDCG@10 `0.967762`,
  negative-control false-positive rate `0`, offline citation correctness `1.0`, and offline
  citation recall `0.923077`; pipeline/model/runtime/converter/chunker and retrieval policy
  identities matched, so the `0.84` absolute floor and `0.02` relative window remain unchanged.
  The aggregate remains intentionally NO-GO pending Part D, release-owner approval, and the
  production/stable-only specialized-corpus, generated-answer, case-fold, and native
  Windows/Linux manual evidence tracked by task 0225.
- 2026-09-14 Copilot: Part D rejected private candidate `0.1.0-27` during real installed-app
  folder enrolment. The signed hardened macOS worker aborted before `main` because its
  `@rpath/libzvec_c_api.dylib` dependency had no `LC_RPATH`; macOS does not honor the launcher's
  `DYLD_LIBRARY_PATH` for this process. Consent persisted, but indexing failed closed as
  `semantic capability is unavailable`. The fix makes every semantic-runtime worker carry
  `LC_RPATH @loader_path`, stages the verified worker beside the verified native libraries, and
  makes production bundle construction reject a macOS worker without that load command. A local
  exact-shape launch created the authenticated IPC socket without any loader environment
  override. Candidate `0.1.0-27` remains NO-GO; Parts C and D must be rerun against a new immutable
  candidate before any semantic publication.
- 2026-09-14 Copilot: PR #49 merged the signed-worker loader fix to `main` at
  `36c4111d0513f395e3bbbfa0435c14801621c534`; all required CI jobs passed, including both desktop
  installer builds. Prepared replacement candidate `0.1.0-28` and refreshed both release-candidate
  fingerprints. It remains private and NO-GO until clean candidate CI and affected Parts C-D
  reruns pass.
- 2026-09-14 Copilot: Replacement candidate `0.1.0-28` passed normal CI and private four-platform
  workflow `34899128175` at revision `24029a2a5bc07e12ab76b20c476c11f65b586a70`. The signed,
  notarized macOS installer and exact catalog payloads passed independent integrity checks.
  Installed-app enrolment persisted consent and created the production Zvec index, but the worker
  still failed before accepting IPC: the isolated profile produced a 142-byte Unix-domain socket
  path, exceeding macOS `SUN_LEN`. Running the exact signed worker with the installed launch
  arguments reproduced `path must be shorter than SUN_LEN`; a rebuilt worker using the shortened,
  stable per-user endpoint accepted authenticated connections and exited cleanly against the same
  profile. Candidate `0.1.0-28` is therefore NO-GO. The regression suite now covers both endpoint
  selection and a real server/client round trip from a qualification-profile-shaped runtime path.
- 2026-09-15 Copilot: PR #51 merged the long Unix-endpoint fix to `main` at
  `d7065696951cdaa91abdce47429fa4623f0e81c9`; all required CI jobs passed, including both desktop
  installer builds. Prepared private replacement candidate `0.1.0-29` and refreshed both
  release-candidate fingerprints. It remains NO-GO until clean candidate CI and the affected
  private matrix and Part D rows pass.
- 2026-09-15 Copilot: Candidate `0.1.0-29` passed normal CI and private four-platform workflow
  `34954466515` at revision `1c4855daf8542add6e0aedff09b9d3a7dd092b56`. The real installed
  macOS app completed generation `1` for all three qualification documents, persisted indexed
  generation `1`, paused/resumed ingestion, and restarted with one root, three occurrences, and
  zero failures. Corrupting the installed worker was rejected by launch verification, and restoring
  the exact signed catalog bytes recovered reconciliation. However, the Semantic settings UI still
  reported `Installed and enabled` with no error or announcement while the worker was invalid.
  Candidate `0.1.0-29` is NO-GO pending a status-integrity fix and rerun of the affected Part D rows.
- 2026-09-15 Copilot: Fixed the status-integrity defect in `972ef18`. Managed component status now
  re-verifies every active installed payload against the trusted catalog and returns a typed
  integrity failure for changed bytes. Semantic settings renders that failure as an announced
  recovery alert and no longer claims the components are installed and enabled. Preparing private
  replacement candidate `0.1.0-30`; it remains NO-GO until candidate CI, the private matrix, and
  the affected installed-app qualification rows pass.
- 2026-09-15 Copilot: PR #53 merged replacement candidate `0.1.0-30` to `main` at
  `d8aefafbae25e1132b81cca8f4ae3a4a7379e8d2`. Private workflow `34964980701` passed all four
  payload, signed-catalog, and installed-qualification rows. Public prerelease, semantic asset
  collection/publication, desktop installer publication, Homebrew, and Chocolatey jobs remained
  skipped; both release variables remained absent. The retained macOS arm64 operator kit passed
  every recorded SHA-256 check. Its catalog revision is
  `procyon-macos-aarch64-0.1.0-30-d9f7ba0344e0dd22aa81adc06c916240`, catalog SHA-256
  `bc7523ce2a04b8d91827e169bc390ba3e0e9432c59e22569042ebbf79d5e3855`, worker SHA-256
  `3bb4328c8ee89121710557d57a109b299d712b33a2310c5974941575ab7754d5`, and DMG SHA-256
  `8fd8f403233c494ff309f0b294a7d32471bd357e13f38b733558a4ffbf7834cc`. Candidate
  `0.1.0-30` remains NO-GO pending the affected Part D installed-app rerun and remaining manual
  qualification rows.
- 2026-09-15 Copilot: Installed the retained signed/notarized `0.1.0-30` DMG into `/Applications`
  while preserving the helper-owned isolated profile and retaining the installed v29 bundle as
  evidence. The exact v30 worker/runtime generations reconciled the existing root with three
  occurrences and zero failures without losing consent or the Zvec index. Appending one null byte
  changed the worker SHA-256 to
  `531e4f3a00a6657870edbe2059bb953b9d83a56dd966e1ffa73efa75ec08e0c1`.
  On restart, Settings -> Semantic no longer reported `Installed and enabled`: its live alert named
  artifact `procyon.semantic.worker.macos-aarch64.0.1.0.30.3bb4328c8ee89121` as failing integrity
  validation, offered Retry, and instructed the user to restore/reinstall while explicitly stating
  that indexed files and folder consent remain unchanged. Restoring the exact signed worker
  (`3bb4328c8ee89121710557d57a109b299d712b33a2310c5974941575ab7754d5`) recovered startup
  reconciliation to one root, three occurrences, and zero failures. The corrupt-component
  rejection, user-visible error, accessibility role, and recovery rows now pass for v30.
- 2026-09-15 Copilot: Candidate `0.1.0-30` passed retained-data uninstall and reinstall against the
  helper-owned profile. `Retain index` removed the managed worker/runtime/model while preserving
  library policy, folder consent, root identity, and logical index data; reinstalling the exact
  signed components resumed the same enrolled root without renewed consent and advanced it to
  indexed generation `8`. Whole-tree Zvec byte equality is not a valid retention invariant because
  normal worker shutdown compacts or rewrites storage files.
- 2026-09-15 Copilot: Candidate `0.1.0-30` remains NO-GO after `Delete index` deterministically
  failed without mutation. The desktop quiescer rejected the live `worker.pid` and instructed the
  operator to restart, but startup reconciliation immediately relaunched the worker and recreated
  that marker. The application facade now performs the existing bounded semantic-worker restart
  operation before component-manager quiescence and deletion, with a regression proving this
  ordering; Semantic settings also gives each uninstall radio title and description an independent,
  zoom-safe layout row. These changes require a replacement signed candidate and affected Part D
  rerun before the explicit-delete row can pass.
- 2026-09-15 Copilot: Prepared private replacement candidate `0.1.0-31` with refreshed semantic
  production and knowledge-retrieval candidate fingerprints. It remains NO-GO pending normal CI,
  the private four-platform payload/catalog/installed matrix, and affected Part D installed-app
  reruns. Both public release gates remain disabled and no assets are approved for publication.
- 2026-09-15 Copilot: PR #54 merged candidate `0.1.0-31` to `main` at
  `7b2bef1241958b03c2d3d1015ee3264aa855a3e2`. Private workflow `34990760800` passed all four
  payload, all four signed-catalog, and all four installed-qualification rows. Public prerelease,
  semantic asset collection/publication, Windows/macOS/Linux installer publication, Homebrew, and
  Chocolatey jobs remained skipped; both semantic release variables remained absent. Downloaded
  only the macOS arm64 installed-qualification artifact. Every `SHA256SUMS` entry passed, the DMG
  checksum and image verified, stapling validation succeeded, Gatekeeper accepted both the DMG and
  mounted app as notarized Developer ID software, the app signature passed deep strict validation,
  and its bundle version is `0.1.0-31`. The catalog revision is
  `procyon-macos-aarch64-0.1.0-31-abd82adc1f1c027d9a465d71953daa86`, catalog SHA-256
  `c2340716ca7df018d5e0e0b75ebd21c6b3486bbdefdf414ebe8e619c7c5acda7`, signature SHA-256
  `8c12cfdee5f90112c1af7b3996a72d7b320e871cc0d4ca7c1dc5f06ab734c2d1`, worker SHA-256
  `bcacbac3fc3beb43308ae142f0c23c38180dc48153b402d12ac024170ef62e95`, Zvec runtime SHA-256
  `44ae3339bfc9bd1d09efa75c5adadb30ff23adddfbb4251bd2adc0747e33372a`, and DMG SHA-256
  `d8649d93e228150243eedeef9b2cd0ea4696f06c7309ea9d3589a0927549746a`. Candidate v31 remains
  NO-GO until the affected explicit-delete and remaining Part D manual rows pass; the report's
  separate stable-only production-evaluation and release-owner blockers remain expected.
- 2026-09-15 Copilot: Installed the verified signed/notarized v31 bundle into `/Applications` and
  upgraded only the helper-owned isolated profile's component-manager state and signed
  worker/runtime generations. Startup reconciliation preserved library
  `4dc1924a-9902-5ec4-adaf-971236d8fdc0`, root
  `be47a576-922e-41b3-8047-7ef3947e3b4a`, folder consent, and three occurrences with zero failures.
  The repaired `Delete index` flow then passed in the real app: its accessible status announced
  `Components and index deleted`, the worker exited, `worker.pid` disappeared, installed-component
  state became empty, active model/index schema were cleared, and the Zvec active-index marker was
  removed. Restarting v31 did not relaunch the worker or recreate component/index state.
  Reinstalling the exact signed v31 components from a clean helper-owned staging profile reused the
  existing enrolled root without renewed consent, rebuilt the deliberately deleted derived index,
  and advanced indexed generation from `8` before deletion to `11` with one root, three occurrences,
  and zero failures. The staging profile was then removed by the qualification helper.
- 2026-09-15 Copilot: The uninstall options expose complete accessible names for both title and
  description. At native installed-app scale, each option measured 37 px high with 6 px between
  rows and 5 px before the confirmation action, so the reported overlap is fixed. The installed
  Tauri WebView did not respond to the documented `Cmd++` page-zoom shortcut and its accessibility
  geometry remained unchanged; do not mark the separate 200% zoom row passed from this evidence.
- 2026-09-15 Copilot: Rolled the installed app back from signed v31 to the retained signed/notarized
  public base-only v26 bundle and launched it against the same isolated profile. No semantic worker
  started, and hashes for ordinary top-level profile files plus component-manager, library policy,
  consent/catalog, and indexed-generation state remained unchanged. Restored the signed v31 app,
  which reconciled the same root with three occurrences and zero failures and restarted the exact
  verified v31 worker/runtime. `/Applications/Procyon.app` is left at `0.1.0-31`; the normal profile
  was never launched by v27-v31.
- 2026-09-16 release owner: Confirmed the first experimental-alpha policy defers generated-answer
  grounding and reviewed preserve-case versus Unicode case-fold comparison to production/stable
  task 0225. The alpha report must mark generated-answer grounding explicitly deferred, document
  both limitations, and cannot qualify under the production-stable validator.
- 2026-09-16 release owner: Requested native Tauri `Ctrl/Cmd +/-` page-zoom support but deferred
  the 200% installed-app layout pass. The accessibility row remains pending and release-blocking;
  enabling the host capability is not qualification evidence.
- 2026-09-16 release owner: Approved preparing `0.1.0-32` as the first public experimental semantic
  prerelease for cross-platform testing. The 200% and Windows/Linux manual checks remain explicit
  production/stable follow-ups; publication still requires the exact v32 four-platform automated
  qualification and fail-closed experimental-alpha report.
- 2026-09-17 Copilot: Candidate `0.1.0-32` merged to `main` at
  `46eb7fce34cd68dbb03617275a664b0f96edef8d`. Exact private workflow
  [`35193552706`](https://github.com/erikvullings/procyon/actions/runs/35193552706) passed all four
  payload, signed-catalog, and installed-qualification rows; every public/package publication job
  remained skipped and both release gates remained absent. Retained all 12 artifacts outside the
  repository with `RETAINED-SHA256SUMS`
  `029a1ee90593fd9463583ad81fc9346ef66efc85ab3773876dd3190a0b25e159` and
  `RETAINED-FILE-SIZES`
  `01e2b32483f4b5879cdcb60e2091dfc2d86480e9da731d3887cae360bf893e87`.
  All 13 payload checksums and four detached catalog signatures verified independently. The
  signed/notarized v32 macOS app reconciled a safe clone of the populated isolated library with one
  root, three occurrences, zero failures, and indexed generation `13`; the original profile and
  normal profile were untouched.
- 2026-09-17 release owner: Approved the reviewed v32 experimental-alpha evidence and selected an
  alpha citation policy that requires deterministic citation correctness `1.0` and citation recall
  at least the existing chunk-recall floor `0.80`; production/stable remains `1.0`. Exact v32
  measurements are identical on all four targets: file/chunk recall@10 `0.958333`, MRR `0.916667`,
  nDCG@10 `0.967762`, negative-control false-positive rate `0`, offline citation correctness `1.0`,
  and offline citation recall `0.923077`. The missing multi-facet Beta citation scored `0.856455`
  and was honestly excluded by the unchanged `0.84` absolute floor plus `0.02` strongest-candidate
  window (`0.872621` effective cutoff). Changing the validator invalidates the candidate
  fingerprint, so run `35193552706` cannot authorize publication of the follow-up revision; a new
  immutable candidate CI and private qualification are required. No release gate or tag was set.
- 2026-09-17 release owner: Confirmed that the generated-summary, unavailable-source, and
  concept-label production scenarios belong to stable follow-up task 0225 and must not block the
  first experimental alpha. The alpha validator now defers only those typed specialized scenarios;
  ordinary and incremental-edit scenarios remain mandatory, and production/stable remains strict.
  This scoring change refreshes the release-candidate fingerprint, so private run `35213304875`
  remains retained reviewed evidence but cannot authorize the corrected revision. A new immutable
  candidate CI and private qualification are required before generating the GO report.
- 2026-09-17 Copilot: PR #61 merged the corrected experimental-alpha policy at immutable revision
  `fe77786592636d5540d59db458e5d17f829b500a`. Exact private workflow
  [`35226068036`](https://github.com/erikvullings/procyon/actions/runs/35226068036) then passed
  private safety, payloads 4/4, signed catalogs 4/4, and installed qualifications 4/4; every
  release/package publication job remained skipped and both release gates remained absent.
  Retained all 12 artifacts outside the repository (1,486 files, 4,529,778,107 bytes) with
  `RETAINED-SHA256SUMS`
  `a9480194dd68b38b92f3b52dcd7c97fb608bf3945e44c5eefe454e9dee8047b6` and
  `RETAINED-FILE-SIZES`
  `0f325ffbb8c2a26bec00e6d1791aef5490e4973ed164549d12856352969c2d72`.
  All retained digests, four detached signatures, exact catalog payload sets, installed
  lifecycle/privacy summaries, macOS operator-kit checksums, DMG integrity, notarization,
  stapling, Gatekeeper assessment, and mounted app signature were independently verified.
- 2026-09-17 Copilot: Generated the checked-in four-target report from the exact run
  `35226068036` inputs and the recorded release-owner approval. The report binds to fingerprint
  `sha256:97899d4adc899e181e3feed875c477594c37150d103d73b95d8281508b8a06e9`,
  records identical target metrics, explicitly defers generated-answer, reviewed case-fold,
  specialized task-0225 scenarios, 200% layout, and Windows/Linux native manual evidence, and
  validates as an `experimental-alpha` GO with zero blockers. The precondition wrapper's success
  message used an undefined variable after typed validation; the focused report change corrects it
  and adds success-path coverage. Publication remains blocked until this report PR is reviewed and
  merged; neither gate, tag, nor public asset has been changed.
- 2026-09-17 Copilot: Report-PR validation exposed a pre-existing circular gate in the
  fingerprinted `semantic_production_evaluation.rs`: its repository-report test requires the
  canonical report to remain `NoGo`, while the release workflow requires that same canonical file
  to be the reviewed `experimental-alpha` `Go`. Updating the assertion changes a source included
  in `release_candidate_fingerprint()`, so it would invalidate exact run `35226068036`; leaving it
  unchanged makes normal Rust CI fail on the GO report. No report PR was opened, and no gate, tag,
  publication, rerun, or duplicate dispatch occurred. The retained evidence and generated report
  remain available outside git. Resolving the circular test requires a focused source change and,
  because the source is fingerprinted, fresh exact private evidence unless the release owner
  explicitly changes that requirement.
- 2026-09-17 release owner: Authorized a focused prerequisite fix for the circular
  repository-report test followed by exactly one fresh private qualification run. The prerequisite
  keeps the canonical report fail-closed at NO-GO, makes the test accept either a fully validated
  NO-GO template or measured GO report, and fixes the precondition wrapper's success message. Do
  not enable either gate, tag, publish, rerun `35226068036`, or dispatch the replacement private
  workflow more than once.
- 2026-09-17 Copilot: PR #62 merged the circular-test fix at immutable revision
  `10613c716023f86074c67a4bb32780382f1b3623`. The one authorized replacement private workflow
  [`35247197346`](https://github.com/erikvullings/procyon/actions/runs/35247197346) passed safety,
  payloads 4/4, signed catalogs 4/4, and installed qualifications 4/4 while every public/package
  publication job stayed skipped and both gates remained absent. Retained and reverified all 1,486
  files; the SHA and size manifest digests are
  `cf1c8eb295270dbfbb787430973917ed6a1670cfcb72db4de8635d9461e644f4` and
  `08d1ccdfa94d2e76dea5fef54c816a9f9e7a4d6d500a836128d3971c2930a625`.
  Independently verified all four detached signatures and payload sets, all installed
  lifecycle/privacy summaries, and the macOS DMG checksum, image, notarization, stapling,
  Gatekeeper result, and mounted app signature.
- 2026-09-17 Copilot: Generated the canonical report from exact run `35247197346` and the recorded
  owner approval. It binds to fingerprint
  `sha256:5959df73fbd01471e637df2a566d7b8f2e3ee6e4f43a9e7d26a379d2c1a2789c`,
  contains four production measurements, records every approved task-0225 deferral, has zero
  blockers, and passes the fail-closed `experimental-alpha` validator as GO. Publication remains
  blocked until this focused report PR is reviewed and merged; neither gate, tag, nor public asset
  has been changed.
- 2026-09-17 Copilot: PR #63 merged the reviewed GO report and `v0.1.0-32` triggered exact public
  run `35261725956` at `9c0f9e7684529f5b4f12b41ae927b27125a0f88e`. Payloads and signed
  catalogs passed 4/4, but GitHub skipped semantic collection and publication: the collection job
  omitted `always()`, so the intentionally skipped private-safety ancestor propagated through the
  successful public payload matrix. The job also downloaded catalogs without declaring them as a
  dependency. The semantic gate was reset to `false`, monitoring was cleared, the run was
  cancelled before package publication, and the empty prerelease was withdrawn to draft. No
  rerun, redispatch, retag, or force-update occurred. The corrective workflow now explicitly needs
  both payloads and catalogs and evaluates their successful results under `always()`. Public runs
  intentionally reuse the exact private installed-qualification evidence rather than rerunning
  those four private-only jobs.
- 2026-09-17 release owner: Approved `0.1.0-33` as the clean recovery candidate after PR #64
  fixes the public semantic-collection graph. Keep `v0.1.0-32` immutable and withdrawn, run one
  fresh private qualification for the new version/fingerprint, and do not publish until its exact
  four-target evidence produces a reviewed GO report.
- 2026-09-17 Copilot: PR #65 merged `0.1.0-33` at immutable revision
  `020ac3bf021bf129934f88020fc8dcd4e786e4a2`. The single approved private workflow
  [`35271430396`](https://github.com/erikvullings/procyon/actions/runs/35271430396) passed private
  safety, payloads 4/4, signed catalogs 4/4, installed qualifications 4/4, and private aggregation;
  every public/package publication job remained skipped and both release paths stayed disabled.
  Retained all 13 artifacts outside the repository (1,514 files, 5,289,796,435 bytes) with SHA and
  size manifest digests `c695d4be67b6f7b3246911d976acbd7aabd470766d53daaee3939eb57603227a`
  and `3e751dccc25862ad388ad814c3b02e1e9b44404176aa909b6a95f86ba635b23f`.
  All detached signatures, exact payload sets, installed lifecycle/privacy summaries, operator-kit
  checksums, DMG integrity, notarization, stapling, Gatekeeper assessment, and mounted app signature
  verified independently. Exact evidence generates a four-target `experimental-alpha` GO report
  for fingerprint `sha256:1d963240ffe79d46983f6b2332e11fa82d7587b28639a38dbd97388a04837f5a`
  with zero blockers. Publication remains blocked until the focused report PR is reviewed and
  merged.
- 2026-09-18 Copilot: Public run `35285177542` proved the combined release flow unsound. Payloads
  and signed catalogs passed 4/4, but the public build used report-merge revision
  `6178befeb771dbd6babfc9aa4a3ab2c3d49654ac` and final GitHub release URLs while the reviewed
  private evidence bound revision `020ac3bf021bf129934f88020fc8dcd4e786e4a2` and qualification
  URLs. macOS and Windows rebuilds also produced different catalog revisions. The exact comparison
  correctly failed, semantic publication was skipped, and the workflow was cancelled before
  desktop assets were published. The prerelease retains zero assets.
- 2026-09-18 Copilot: Recovery adopts the independent semantic component release design. Semantic
  components now qualify and publish through
  a separate manual workflow. Qualification uses the final `semantic-v*` URL and emits a lock over
  the exact run, source revision, evaluation fingerprint, catalogs, and signatures. Publication
  reuses those exact retained bytes and verifies every payload against the reviewed lock. Desktop
  releases only fetch a previously published locked catalog, allowing component and Procyon
  releases to advance independently.
