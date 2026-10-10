# Experimental opt-in EmbeddingGemma 2 component: release decision

**Decision: an explicitly opt-in EXPERIMENTAL package is desired, but no
candidate is approved for publication yet.** E5 remains the default. The
reviewed candidate predates both the automatic Metal worker and mixed-media
UI work. It would ship a CPU-only worker even if a newer desktop binary
enabled Metal. Its checked-in experimental lock is deliberately `noGo` and
`blocked-stale-pre-integration-candidate`: the manual publication workflow
rejects it even if its release environment gates are set to `true`; the
experimental verifier independently rejects that historical source revision
even if someone changes the lock decision to `go`. Do not
dispatch publication, publish, tag, or enable it. The future release-owner
override must be an explicit new decision, **not** a reclassification of
E5 aggregate measurements as Gemma quality evidence.

The historical, **not publishable** candidate is [qualification run
37974984924](https://github.com/erikvullings/procyon/actions/runs/37974984924),
retained candidate artifact **11640886748**, source revision
`d3f921fe08e4994a905727a71af102979f4fbe23`, catalog release tag
`semantic-v0.4.2-gemma-qualification-r2`, and aggregate candidate fingerprint
`sha256:acf96cda7d0bb0c706e76a41ac14f27ffa484dd3fabdf935f47a3e134887fd77`.
Its four target catalog/signature hashes, precise run/tag/source, original
Gemma model revision, negative-control report hash, and candidate-lock hash
remain recorded in
[`semantic-gemma-experimental-v1.json`](semantic-gemma-experimental-v1.json).
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

**Before any publication:** integrate the Metal-enabled worker, mixed-media
UI, and application-level consented media enrollment and authorized search
through host/VFS/worker; verify automatic Metal selection and CPU fallback on
supported hardware. Build and qualify a **new four-target signed candidate**
from the integrated source. Its catalog URLs are bound to its release tag,
and its source revision, candidate run ID, artifact fingerprints and
signatures must receive a new reviewed experimental lock and disclosure.
For the next installed lifecycle check, provide the new candidate's exact
`installed_candidate_run_id` and `installed_candidate_source_revision`:
the read-only workflow downloads the artifact for that pair and rejects the
historical source. Candidate artifacts expire after 14 days, so complete
installed checks and retain the reviewed manifests before they expire;
if expired, qualify a fresh candidate rather than substituting the old run.
Only after these checks and a fresh explicit
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
reviewed manifest remain outstanding; do not enable the switch or tag 0.5.0
until the integrated worker and app path have been exercised.
