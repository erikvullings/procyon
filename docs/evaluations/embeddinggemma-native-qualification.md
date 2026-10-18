# Native EmbeddingGemma 2 CPU retrieval probe (2026-10-09)

**Decision: do not enable Gemma for release.** The [machine-readable per-case
report](embeddinggemma-native-cpu-2026-10-09.json) is one local, model-level
measurement, not a four-target quality or installed-component qualification.
The [four-target candidate run](https://github.com/erikvullings/procyon/actions/runs/37974984924)
verified packaged smoke and signed catalogs, but its aggregate retrieval
evaluation measures E5. This probe measures Gemma itself.

`evaluate_gemma_native` hashes the five required original files against the
pinned revision `914f7f89142e33e77833254d9c9b90c3cef7303b`, then runs the
Rust native FP32 CPU encoder offline. It uses the same 30 eligible chunks,
10 positive and 3 unanswerable multilingual queries as the [exploratory
E5/Gemma comparison](https://github.com/erikvullings/procyon/blob/embedding-model-comparison/docs/evaluations/embedding-model-comparison.md),
from `knowledge-retrieval-corpus-v1.json` (SHA-256
`c4591389db16b6ba91dba1b4795f9a903ae038b27dccc7aeba852c497f502abb`).
The 12 generated code snippets and six hand-labelled queries with behaviorally
similar wrong answers are restored unchanged from that comparison (SHA-256
`c2420e55dabdbecacba39542b33ce483e988e74c33495c5e44121a190c558bac`).
Knowledge queries use `SearchQuery`, code queries use `CodeRetrieval`, and
untitled documents use `Document`; the titles are deliberately omitted to
match the earlier baseline. Both sides use the same 768d inference vectors,
truncated and L2-renormalized for 128/256/512/768d. Rankings are exact cosine
similarity with source-ID collapse for the knowledge corpus, not Zvec/hybrid.

| Labelled task | E5 384d, earlier Python CPU | Native Gemma 128d | 256d | 512d | 768d |
| --- | ---: | ---: | ---: | ---: | ---: |
| Knowledge first relevant file at rank 1 / MRR (10 queries) | 1 / 1 | .9 / .95 | .9 / .95 | .9 / .95 | .9 / .95 |
| Code first relevant snippet at rank 1 / MRR (6 queries) | 1 / 1 | 1 / 1 | 1 / 1 | 1 / 1 | 1 / 1 |
| Generated color/shape image at rank 1 / MRR (4 text-to-image queries) | n/a | 1 / 1 | 1 / 1 | 1 / 1 | 1 / 1 |

All positive knowledge and code cases have a relevant result within the first
three ranks. On this native run `application-context-distraction` ranked the
file-manager pane-sorting distractor above the relevant checklist at 128d;
the same aggregate knowledge MRR held at every dimension. Three unanswerable
knowledge queries are reported with top IDs but **excluded** from ranking
metrics: no calibrated threshold was tested, so no rejection claim follows.
The image fixture generates four labelled colored shapes and four same-color
or same-shape wrong answers as 128-pixel PNGs; all four text queries ranked
their positive first. These easy generated negatives do not establish
photographic relevance, hard near-duplicate detection, or image-to-image
quality. E5 has no image encoder.

On an Apple M4 Max with 128 GiB RAM (macOS arm64), the optimized native
text-and-image probe used **2,583,347,200 bytes peak process RSS** and 209.07 s
total wall time (`/usr/bin/time -l`, including image inference). Warm-cache
checkpoint construction took 0.69 s, 30 knowledge documents ran at 11.64/s
and 12 code snippets at 14.35/s; single-query median/p95 were 30.35/45.96 ms
(knowledge) and 23.95/27.53 ms (code). Eight generated images ran at
**0.039 images/s**, including image preprocessing; four text-to-image query
median was 22.30 ms. This is one mixed workload, not a per-modality peak,
per-dimension CPU speed, or supported-target resource ceiling. The earlier
E5 numbers use a different Python runtime: compare **ranking on shared
labels**, not throughput or memory across runtimes. All timing above uses
768d inference with projection-width truncation for ranking only.

To reproduce, obtain verified pinned originals with
`node scripts/fetch-embeddinggemma-probe.mjs`, then run:

```sh
cargo test -p fm-semantic-worker --features gemma-native --example evaluate_gemma_native
cargo run --release -p fm-semantic-worker --features gemma-native \
  --example evaluate_gemma_native -- <verified-model-directory> > native-report.json
```

The example refuses missing or altered original files and emits per-case top
IDs, all four dimension metrics, corpus hashes, and text/image timings. On
macOS, the optional Zvec native dependency may require the bundled runtime
loader path (`DYLD_LIBRARY_PATH`) before running the binary; for process peak
RSS, run `/usr/bin/time -l` over a shell that sets that path. The reported run
used originals extracted from candidate artifact 11640886748 and retained
its full machine-readable report in this directory.

Before any release decision: independently label more realistic multilingual
and code workloads; evaluate photographic images, image-to-image and media
near-duplicate negatives, audio and video if offered; compare against the
packaged E5 backend under matched conditions; measure query/index latency,
throughput, peak RAM, disk and installation across each supported CPU target
and output dimension. Qualify signed installed/offline Gemma lifecycle and
review the release gate separately. Do not interpret these small saturated
fixtures or E5 aggregate evaluation as Gemma approval.
