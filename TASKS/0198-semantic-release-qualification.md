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
  chunker, retrieval policy, and index identity; record file/chunk recall, MRR, nDCG, negative
  controls, and grounded-answer citation correctness.
- Calibrate absolute and relative Ask similarity thresholds from the labelled report. Do not lower
  the current `0.84` absolute floor merely to increase result count; document before/after quality
  and storage/migration impact for any threshold or pipeline change.
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

- [ ] Use either a separate standard macOS account or the current account with the helper-managed
  isolated profile and every ordinary Procyon instance closed. Never launch the candidate against
  the operator's normal `~/Library/Application Support/fm` profile.
- [ ] Install/launch public base-only v26 once, then install the private alpha DMG over it. Back up
  the existing application first when using the current account. Verify DMG checksum, signature,
  notarization, stapling, app launch, and preservation of ordinary settings/workspaces.
- [ ] Provision components with the private qualification kit and exercise consent, estimates,
  activation, folder enrolment, indexing progress, cancellation/resume, restart, Semantic Search,
  Ask Your Files, citations, negative controls, and offline/error behavior.
- [ ] Verify worker restart, corrupt-component rejection/recovery, retained-data uninstall,
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

- [ ] Fix every alpha-blocking defect and rerun affected automated/manual rows on one immutable
  candidate.
- [ ] Update the qualification report and checked-in opaque evaluation evidence with four exact
  production measurements, macOS manual evidence, the `experimental-alpha` tier, honest deferred
  Windows/Linux rows, known limitations, and rollback instructions.
- [ ] Run the alpha-aware semantic precondition validator and obtain explicit dated release-owner
  approval for the named revision and artifacts.
- [ ] Set only `SEMANTIC_RELEASE_QUALIFIED=true`, tag the matching numeric alpha, and require
  semantic publication plus all desktop installer jobs to pass.
- [ ] Verify the public macOS installer from the test account downloads only the published signed
  payloads and reproduces the qualified Search/Ask smoke. Verify the Windows/Linux installer
  digests and automated smoke reports.
- [ ] State prominently in release notes that semantic functionality is experimental and opt-in,
  Windows/Linux manual accessibility testing is pending, Windows artifacts are unsigned, and
  macOS x86-64 semantic runtime is unsupported.
- [ ] On any public verification failure, reset `SEMANTIC_RELEASE_QUALIFIED=false` and issue a
  base-only corrective alpha.

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
- [ ] Verify upgrade from the retained preceding candidate, rollback, and re-upgrade without
  losing consent, enrolment policy, or source data and without reusing an incompatible index.
- [ ] Verify uninstall with retained semantic data, reinstall, explicit semantic-data deletion,
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
