# Experimental opt-in EmbeddingGemma 2 component: release decision

**Decision: an explicitly opt-in EXPERIMENTAL package is desired, but no
candidate is approved for publication yet.** E5 remains the default. The
historical candidate predates both the automatic Metal worker and mixed-media
UI work. It would ship a CPU-only worker even if a newer desktop binary
enabled Metal. The checked-in lock now identifies the integrated candidate,
but remains deliberately `noGo` and `blocked-pending-release-owner-review`:
the manual publication workflow rejects it even if its release environment
gates are set to `true`; the experimental verifier independently rejects the
historical source revision even if someone changes the lock decision to `go`. Do not
dispatch publication, publish, tag, or enable it. The future release-owner
override must be an explicit new decision, **not** a reclassification of
E5 aggregate measurements as Gemma quality evidence.

The integrated, **still unapproved** four-target signed candidate was built
from source `c6c7a94058950bf2704aca484f9c4b77c6dd2dcf` in
[run 38077959991](https://github.com/erikvullings/procyon/actions/runs/38077959991)
for tag `semantic-v0.5.0-gemma-experimental-r1`. Its retained artifact is
`semantic-component-candidate-c6c7a94058950bf2704aca484f9c4b77c6dd2dcf`
(ID **11679899935**, expires **2026-10-24 19:54:23 UTC**) and its aggregate
fingerprint is
`sha256:a0c16d0fec4f1ecc266af4587c31440d5e2ac0dccf69f6505a94b8d28b51becf`.
All four signed catalogs and their catalog-listed payload bytes were verified
against the release public key. The candidate evaluation is still `noGo`,
`experimental-alpha`: its E5 measurements are negative controls, **not**
Gemma retrieval-quality evidence. The old candidate below remains revoked;
neither candidate has a reviewed `go` approval. The checked-in experimental
lock binds this new candidate's exact manifest, report, model revision, and
four catalog/signature hashes without authorizing publication.

The [exact installed qualification run
38082235196](https://github.com/erikvullings/procyon/actions/runs/38082235196)
downloaded that retained candidate using its run ID and source revision.
On Linux aarch64/x86-64, macOS aarch64, and Windows x86-64, each signed
offline lifecycle passed **13 automated checks**, with zero network artifact
reads. Image, audio, video, and text ingestion, query, and worker restart
passed at 128/256/512/768 dimensions. Each target's report still marks
preceding-candidate upgrade/rollback **blocked** without an exact preceding
signed candidate, and native screen-reader and packaged keyboard/consent/
progress/error/citation/deletion checks **manual-required**. The four
`installed-gemma-*` run artifacts retain the per-dimension measurements.
The bounded synthetic mixed-media runs used CPU fallback on macOS; image
ingestion took approximately 56-67 seconds on a virtual M1, 101-106 seconds
on Linux ARM, 75-77 seconds on Linux x86-64, and 87-93 seconds on Windows
x86-64. Observed worker peak RSS was about 2.7-3.6 GiB across targets,
not a safe minimum; retain the 8 GiB planning estimate. These single-item
measurements do not establish quality, sustained throughput, GPU performance,
or a representative user workload.

The new signed macOS worker also passed the opt-in application-level test:
consented PNG, MP3, and MP4 sources crossed host/VFS/IPC, returned with
available citations in the authorized tenant, and were absent from another
tenant. Its mixed-media library reported CPU fallback; a separate image-only
startup of the same signed worker automatically selected FP32 Metal images.
An additional run of that exact signed worker removed the enrolled PNG:
reconciliation revoked its citation, and the file-primary semantic search
excluded it even though the worker's derived index could still return a raw
stale vector. The application test now waits for a clean worker shutdown
before removing its temporary index. This is **automated host-path evidence**,
not a manual desktop check or a claim that raw worker vectors are immediately
erased.
The local image-only Metal comparison in
[`embeddinggemma-metal-image.md`](embeddinggemma-metal-image.md) is bounded
hardware evidence, not a GPU benchmark of the installed four-target run.

**UI preflight boundary:** automated Mithril tests cover explicit Gemma
fresh-index consent and immutable media/dimension choices, immediate install
progress and typed errors, distinct retain/delete uninstall choices, media
source-open actions in file-primary and Knowledge results, unavailable and
stale-result disabling, and labelled, focusable Gemma setup controls. The
focused settings/pane/Knowledge run passed 289 tests before the extra
accessible-name/focus regression was added; that new settings test also
passed. These are DOM and mock-client checks, not interaction with the
signed installed candidate.

On 2026-10-10 a **bounded native development UI preflight** used a macOS
ARM Tauri debug build (`fm-desktop` v0.5.0, `semantic-gemma` feature) and the
locally development-key-signed `darwin-arm64-gemma-metal` bundle, in an
isolated `dev.procyon.gemma-review` app/home. This is **not** the four-target
production-key-signed candidate or a packaged release binary. Through the
native window, macOS accessibility inspection found named Settings, Semantic,
Gemma profile, media-permission checkboxes, dimension selector, fresh-index
acknowledgment, and install button. E5 remained the recommended profile.
Selecting Gemma exposed the 1.5 GiB download/install, 8.1 GiB planning
RAM, licenses, local processing, and separate-library disclosure. The
install action was disabled until 768 dimensions and fresh-index
acknowledgment were selected. Explicitly opting into image/audio/video
and accepting displayed "Starting the signed model download and
installation"; the isolated app then displayed "Installed and enabled" and
"Exact active model: EmbeddingGemma 2 (optional)". Its development-bundle
warning explicitly says it cannot validate retrieval quality.

Keyboard Return opened an isolated fixture folder in a pane. The native
folder-consent view named **only** that folder, showed an unavailable size
estimate and local retention warning, and accepted "Include and index folder".
Settings subsequently showed the root enrolled and indexing generation 1.
However, the isolated catalog's `documents` and `occurrences` remained
empty; native semantic search showed "No indexed documents are available
in this scope." The fixtures were inside this repository's ignored
`target/` review root, so this does **not** establish a general indexing
failure. No native image/audio/video result appeared to open, navigate,
test stale/unavailable behavior, or delete. No install error was deliberately
induced, and no macOS VoiceOver session was run: inspecting accessible names
with System Events is **not** a screen-reader walkthrough. The review
window later disappeared while its process remained; the isolated Tauri/Vite
session was stopped. Windows/Linux native UI, production-signed packaged
keyboard/consent/progress/error/deletion, and full macOS VoiceOver checks
remain untested.

A normal desktop cannot install the exact candidate while its catalog is
unpublished and the approval lock is `noGo`; bypassing those gates to make
the UI appear releasable would invalidate this preflight. Release-owner
review must obtain or explicitly disposition the remaining manual checks
before approval.

The historical, **not publishable** candidate is [qualification run
37974984924](https://github.com/erikvullings/procyon/actions/runs/37974984924),
retained candidate artifact **11640886748**, source revision
`d3f921fe08e4994a905727a71af102979f4fbe23`, catalog release tag
`semantic-v0.4.2-gemma-qualification-r2`, and aggregate candidate fingerprint
`sha256:acf96cda7d0bb0c706e76a41ac14f27ffa484dd3fabdf935f47a3e134887fd77`.
Its four target catalog/signature hashes, precise run/tag/source, original
Gemma model revision, negative-control report hash, and candidate-lock hash
were retained during its evaluation; the current
[`semantic-gemma-experimental-v1.json`](semantic-gemma-experimental-v1.json)
instead binds the new integrated candidate without approving it.
All four signed catalogs and their exact catalog-listed bytes were verified
offline using the release public verification key on 2026-10-10. Signed bytes
alone cannot make this pre-integration worker suitable for automatic Metal.
Candidate artifact retention was previously recorded as ending
**2026-10-23**, but its availability does not make it publishable.

An opt-in application-level [integration probe](../../crates/fm-application/tests/semantic_indexing.rs)
on macOS ARM additionally confirmed the **old signed CPU worker** receives
three real files through an enrolled local folder, VFS listing/read, host
consent/eligibility decisions, and the IPC ingestion boundary. A PNG,
44 kHz MP3, and two-second MP4 were admitted only for a Gemma library with
all three media types selected; all three ingestions completed and a
tenant-scoped worker search returned each with its correct media type and a
host-resolved, available source citation. Another tenant saw no results.
This is a bounded synthetic *pipeline* check, not a retrieval-quality
judgement, a full desktop UI check, automatic Metal validation, a
four-target application-level test, or qualification of an integrated
worker. The existing default-library test checks that unselected media
are skipped. Re-run the same end-to-end check on the new integrated,
signed candidate before claiming mixed-media search in a release.

The installed/offline lifecycle (activation, restart, supported media, and
cleanup/recovery) passed on macOS aarch64, Linux aarch64/x86-64, and Windows
x86-64 in [run 37990723942](https://github.com/erikvullings/procyon/actions/runs/37990723942).
Four-target signed CPU resource probes at 128/256/512/768 dimensions ran in
[run 37992680322](https://github.com/erikvullings/procyon/actions/runs/37992680322).
They use one bounded synthetic item per modality and are **not** sustained
throughput or retrieval quality; peak worker RSS on Linux/Windows was about
3.42–3.47 GiB and sampled macOS maximum at least 3.33 GiB, not a universal
memory ceiling. The pinned original model alone is 1,488,915,288 bytes
(~1.39 GiB); tokenizer and configs add ~32.2 MB, alongside separate E5,
workers, runtimes, indexes, and potentially substantial temporary disk use.
The signed model metadata estimates **8 GiB RAM** for installation planning;
do not present measured peaks as a safe minimum. CPU inference may be slow,
particularly for media. Do not advertise GPU/Metal unless the merged target's
runtime and fallback have independently passed qualification.

Independently judged multilingual/code [results](embeddinggemma-judged-2026-10-10.md)
show conditional gains on bounded MIRACL candidate pools but no demonstrated
code advantage over E5, including a persistent buffered-reader failure.
The separate [real-photo caption probe](embeddinggemma-media-2026-10-10.md)
preferred the correct hard-negative caption in **8/10** photos at each
dimension; two photos failed at all dimensions. These ten opportunistically
rights-checked photos are not representative image retrieval, and E5 has no
image encoder for a matched media comparison. **No** independently judged
photo-to-photo, audio, or temporally grounded video hard-negative results
have been obtained. These are disclosure limits, not missing data to fill
with self-labelled or E5-only fixtures.

The optional library must require an informed, explicit selection and
consent before installation/index creation. Select one output dimension
(128/256/512/768) and separately opt in to image/audio/video coverage;
128-dimensional media can lose discrimination. These choices are immutable
for that library: changing them requires a new index/reindex with explicit
confirmation. Starting a Gemma library does **not** silently convert or
destroy an E5 library, and E5 stays available. Source exclusions and
unsupported media must remain clear, not silently treated as indexed.
Supported signed CPU targets are the four named above; packaging a component
alone does not establish quality or GPU behavior on them.

**Before any publication:** review the integrated candidate and installed
reports above, complete or explicitly disposition the blocked/manual checks,
and obtain a fresh release-owner decision on the fail-closed experimental
lock bound to the new run, source, release tag, evaluation report, and four
signed catalogs.
The candidate artifact expires after 14 days; if unavailable when that
review is complete, qualify a fresh candidate rather than substituting the
old run. Only after those checks and a fresh explicit
release-owner decision may an operator request publication with
`approve_experimental_gemma=true` and both independent release environment
gates (`SEMANTIC_COMPONENTS_RELEASE_QUALIFIED` and
`SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED`) enabled. The verifier rejects the
historical lock and changed source, report, catalog/signature, payload,
model revision, run or tag; it cryptographically verifies signed catalogs
before any release. Ordinary qualification dispatches remain read-only.
The standard desktop's approved E5 component lock is unchanged.

The 0.5.0 integration branch prepares a separate desktop opt-in switch,
`SEMANTIC_GEMMA_DESKTOP_EXPERIMENTAL=true`. It fails closed unless both
`SEMANTIC_RELEASE_QUALIFIED` and `SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED` are
`true` and the checked-in experimental approval records a fresh `go` decision,
exactly four target catalogs and an explicit opt-in. The desktop fetches only
the catalog and signature matching that approval, validates its Gemma model
revision, and compiles `semantic-gemma` only in that branch. Merely changing
the switch cannot enable the revoked candidate. A new qualification run and
reviewed manifest are separate: the former has passed, while the latter
remains outstanding. Do not enable the switch or tag 0.5.0 before the
remaining review and approval. Its Gemma
candidate recipe includes automatic FP32 Metal images on macOS ARM only,
with CPU fallback for unavailable Metal and mixed media; the other three
signed targets remain CPU-only. This new recipe does not retroactively
qualify the old CPU-only candidate or establish quality gains.
