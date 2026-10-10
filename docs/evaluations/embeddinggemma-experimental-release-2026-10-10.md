# Experimental opt-in EmbeddingGemma 2 component: release decision

**Decision: offer an explicitly opt-in EXPERIMENTAL package, not a
quality-qualified replacement for E5.** E5 remains the default. This is a
release-owner override of the signed candidate's intentional `noGo`
negative-control decision, **not** a reclassification of its E5 aggregate
measurements as Gemma quality evidence. No automatic publication, desktop
enablement, or release tag follows from this document. The application
integrator must first combine and validate the concurrent UI/consent and
Metal work, including CPU fallback. The workflow must not run its publication
mode until the release environment and exact experimental approval are
deliberately enabled.

The only approved candidate is [qualification run
37974984924](https://github.com/erikvullings/procyon/actions/runs/37974984924),
retained candidate artifact **11640886748**, source revision
`d3f921fe08e4994a905727a71af102979f4fbe23`, catalog release tag
`semantic-v0.4.2-gemma-qualification-r2`, and aggregate candidate fingerprint
`sha256:acf96cda7d0bb0c706e76a41ac14f27ffa484dd3fabdf935f47a3e134887fd77`.
The four target catalog/signature hashes, precise run/tag/source, original
Gemma model revision, negative-control report hash, and candidate-lock hash
are reviewed in
[`semantic-gemma-experimental-v1.json`](semantic-gemma-experimental-v1.json).
All four signed catalogs and their exact catalog-listed bytes were verified
offline using the release public verification key on 2026-10-10. Candidate
artifact retention was previously recorded as ending **2026-10-23**; if
expired, a new qualification and **new reviewed lock** are required, not a
substitution under this approval.

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

For publication, dispatch the manual semantic-component workflow against the
approved workflow revision with **that exact release tag and qualification
run ID**, `approve_experimental_gemma=true`, and both independent release
environment gates (`SEMANTIC_COMPONENTS_RELEASE_QUALIFIED` and
`SEMANTIC_GEMMA_EXPERIMENTAL_APPROVED`) explicitly set to `true`. The workflow
rejects changed source, report, catalog/signature, payload, model revision,
run, or tag, and cryptographically verifies all four signed catalogs before
publication. Ordinary qualification dispatches remain read-only. This
approval does not change the standard desktop's approved E5 component lock;
the integrator must decide when and how to make a genuinely opt-in Gemma
catalog available to application users.
