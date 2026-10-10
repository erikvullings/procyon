# Human-judged EmbeddingGemma 2 / E5 retrieval slice (2026-10-10)

**Decision: keep Gemma disabled and the release guard intact.** This is a
larger, bounded, independently annotated **model-level** CPU retrieval
comparison on one macOS ARM host, not installed-worker quality on all targets
or evidence that Gemma should ship. It follows the
[earlier small probe](embeddinggemma-labelled-2026-10-09.md), whose Mr. TyDi
distractors were unjudged. Unlike that probe, **every candidate in this
multilingual split has an explicit human relevance judgement**.

## Source provenance and bounded selection

| Split | Pinned external sources and rights | Selection fixed before model inference |
| --- | --- | --- |
| Swahili and Bengali | [MIRACL dev topics/qrels](https://huggingface.co/datasets/miracl/miracl/tree/5be20db9509754dadad47689368639fcec739c00) commit `5be20db9509754dadad47689368639fcec739c00`; [MIRACL corpus](https://huggingface.co/datasets/miracl/miracl-corpus/tree/d921ec7e349ce0d28daf30b2da9da5ee698bef0d) commit `d921ec7e349ce0d28daf30b2da9da5ee698bef0d`. Both cards declare Apache-2.0. The underlying passages originate in Wikipedia, whose original attribution/share-alike rights must not be silently replaced by that dataset-card label; no passages are redistributed here. | The first **50 dev topics per language** with >=1 judged positive and >=3 judged negatives. Choose the lexicographically first judged positive and the three **judged grade-0** passages with greatest lexical token overlap (doc-ID tie-break). These negatives were pooled from lexical/dense retrieval and adjudicated by native speakers, *not* inferred from missing qrels. Each query ranks exactly four passages; this is not full-corpus recall or an unbiased random sample. |
| Python code | [CodeSearchNet human annotation CSV](https://github.com/github/CodeSearchNet/blob/bb121a53a559e99a6849409355ee5c83803f2e87/resources/annotationStore.csv), MIT, SHA-256 `0340af32b551ceadb74fec147f97642b7fedf3ff039e38fb86baff49ee899846`. Each selected code file is fetched at the annotation's immutable Git commit. The [per-revision SPDX manifest](../../scripts/fixtures/embedding-code-licenses-v2.json) records 47 repository revisions checked against their GitHub license endpoint (MIT/Apache-2.0/BSD); [source-file hashes](embeddinggemma-judged-2026-10-10/code-source-digests.json) cover the 48 fetched files. No code snippets are redistributed. | Fourteen fixed realistic queries: the previous four retained, and ten added by annotation coverage and verifiable rights **before running this split**. Up to two snippets with all available grades >=2 and two with all grades 0, sorted by source URL, for each query. Grade-1, conflicting ratings, unverified or copyleft-licensed sources, and unjudged cross-query snippets are excluded; some retained items have only **one** human rating. |

The MIRACL inputs total approximately **70 MiB compressed**, not a giant
corpus download. The source SHA-256 checks in
[`prepare-miracl-quality-fixtures.py`](../../scripts/prepare-miracl-quality-fixtures.py)
pin Swahili/Bengali corpus archives respectively to
`1a604a6571cd74061ba2778082bfff7eddc734c9076e11c5bad04125a39fd246` /
`7399b221f6dabab5e9ab0eed6107a27091df3807f4684d588357e4c8abbdaef2`.
Its qrels and topics checksums are pinned separately, and it refuses missing
or contradictory judgements. It emits 50 queries, 194 unique eligible
passages, 50 positive/150 negative *pairs* for Swahili (fixture SHA-256
`b3ee4b90c7988b38bdee5c10c1bc76f13f4fb46d1d43ee30d0f776dbe848dcd1`);
Bengali has 50 queries, 197 unique passages, 50 positive/150 negative pairs
(`be5e544d9864ae60152184dc9c8bed4c00f8f811eb04896fc0cb2ba74b012f41`).
The larger code fixture has 14 queries, 48 unique snippets, 20 positive/28
negative pairs (SHA-256
`4845d62f091b10e8eb74c605336b7a171c90b8512b568e6dd4e31fa3b7c455e7`).

## Matched candidate and results

The exact retained **signed candidate** is [run
37974984924](https://github.com/erikvullings/procyon/actions/runs/37974984924),
artifact **11640886748**. Its macOS ARM detached catalog signature and all
eight selected payload digests were checked against the release environment's
**public** verification key before staging the five Gemma originals and E5
model pack. Each runner additionally checks original member digests.
Gemma runs native Rust FP32 CPU using its `Document` and model-owned
`SearchQuery`/`CodeRetrieval` prompts, with identical truncation and L2
normalization on both sides at 128/256/512/768 dimensions. E5 runs the
candidate's 384d ONNX graph/tokenizer with masked mean pooling, L2
normalization and `passage: ` / `query: ` prefixes. Both use the **same exact
fixture bytes, eligible document IDs and query strings**, exact cosine
ranking and a stable tie-break, not Zvec/hybrid/installed search. Different
model-owned prompts, tokenization and runtimes remain confounders. The host
was an Apple M4 Max with 128 GiB RAM and macOS 27.0.1; do not extrapolate
latency or quality to other supported targets.

| Judged pool (hit@1 / MRR) | E5 384d ONNX | Gemma 128d | 256d | 512d | 768d |
| --- | ---: | ---: | ---: | ---: | ---: |
| Swahili, 50 queries | .72 / .835 | .82 / .902 | .86 / .922 | .88 / .930 | .86 / .920 |
| Bengali, 50 queries | .66 / .803 | .76 / .865 | .78 / .868 | .74 / .847 | .80 / .877 |
| Human-graded code, 14 queries | .857 / .917 | .714 / .845 | .857 / .917 | .857 / .929 | .857 / .929 |

The [machine reports and paired analysis](embeddinggemma-judged-2026-10-10/)
retain **every per-query top ID and first-positive rank**, not only averages.
At 768d Gemma wins/ties/loses versus E5 on judged hit@1 for **8/41/1**
Swahili and **11/35/4** Bengali queries. Swahili E5-only misses are
`sw-35, sw-49, sw-56, sw-141, sw-146, sw-178, sw-213, sw-238`;
the Gemma-only miss is `sw-172`. Bengali E5-only misses are
`bn-42, bn-63, bn-67, bn-103, bn-161, bn-168, bn-181, bn-182,
bn-220, bn-226, bn-241`; Gemma-only misses are `bn-90, bn-115,
bn-213, bn-248`. Both models miss six other queries in each language,
listed with ranks and retrieved IDs in `judged-summary.json`.

The 14 code queries have **five non-perfect cases**, with per-model ranks:

| Query | E5 | Gemma 128d | 256d | 512d/768d |
| --- | ---: | ---: | ---: | ---: |
| binomial distribution | 2 | 1 | 1 | 1 |
| buffered file reader read text | 1 | 2 | 3 | 2 |
| convert a date string into yyyymmdd | 3 | 2 | 2 | 2 |
| deducting the median from each column | 1 | 2 | 1 | 1 |
| find int in string | 1 | 3 | 1 | 1 |

**Buffered-reader diagnosis, without tuning:** the error persists in both
the original four-query and this enlarged code split. The evaluator uses the
same `CodeRetrieval` query and untitled `Document` prompts as the optional
worker; no unexpected prompt mapping was found. The decisive ranked
`channels`/`aredis` negative snippets are short (74/51 Gemma tokens;
76/49 E5 tokens) and the `fswrap` positive is 92 tokens for either model,
so E5's 512-token cutoff cannot explain their ordering. On this enlarged
split E5 ranks the `smart_open` positive first, while Gemma ranks the
human-grade-0 `channels` snippet first at every dimension. **Both
top-ranked negative snippets have only one annotator's grade 0**; that
limitation and the absence of real file titles/context prevent attributing
the miss definitively to model quality versus ambiguous labels or document
presentation. No prompt, label, candidate pool or production setting was
changed in response to evaluation outcomes.

## Uncertainty and remaining gates

The summary script reports a **seeded 10,000-resample paired-query bootstrap
95% percentile interval** for Gemma-minus-E5 hit@1, *conditional on these
deterministically selected tiny pools*. At 768d Swahili the observed
difference is +.14 (conditional interval **[+.04,+.26]**), Bengali +.14
(**[.00,+.28]**); code +.00 (**[-.214,+.214]**). At 128d the multilingual
intervals cross zero and code's observed difference is -.143
([-.429,+.143]). The queries are not a random sample of future files;
only four candidates/query were ranked, dimensions are multiple comparisons,
and the bootstrap is **not** a population-wide confidence statement or
qualification of superiority. MIRACL's original Wikipedia text rights
need legal review before redistributing any corpus-derived artifact; this
evaluation distributes no passages. Some code labels are singly assessed.

Larger representative pools and more judged code queries, independently
rights-verified real-photo/image-to-image hard negatives, lawful audio/video
labels if those modalities are offered, and matched **installed-worker**
retrieval on all supported targets remain open. Existing photo caption
discrimination was only 4/5 at each Gemma dimension; this slice did not
repeat or silently upgrade that evidence. The Gemma release guard and
default-hidden state remain unchanged.

## Reproduce

Download the **two** pinned MIRACL `docs-0.jsonl.gz` files under
`miracl-corpus-v1.0-{sw,bn}/` at the corpus commit above and save them as
`target/gemma-quality-data/miracl/{sw,bn}-corpus.jsonl.gz`. At the qrels
commit, download each language's
`miracl-v1.0-$lang/topics/topics.miracl-v1.0-$lang-dev.tsv` and matching
`qrels/qrels.miracl-v1.0-$lang-dev.tsv`, saving as
`{sw,bn}-{topics,qrels}.dev.tsv`. The fixture preparer checks their exact
SHA-256 values and never downloads a corpus itself. Use the already pinned
CodeSearchNet CSV and candidate model extraction steps in the
[previous report](embeddinggemma-labelled-2026-10-09.md); the code
preparer fetches only the 48 commit-pinned, rights-verified source files
when asked for `--expanded-code`.

```sh
python3 -B scripts/prepare-miracl-quality-fixtures.py target/gemma-quality-data/miracl
python3 -B scripts/prepare-gemma-quality-fixtures.py \
  target/gemma-quality-data --expanded-code
target/gemma-quality-venv/bin/python scripts/evaluate_e5_quality.py \
  target/gemma-quality-data/e5-model target/gemma-quality-data/miracl/sw-judged-fixture.json
DYLD_LIBRARY_PATH="$PWD/target/gemma-quality-data/runtime" \
  cargo run --release -p fm-semantic-worker --features gemma-native \
  --example evaluate_gemma_native -- target/gemma-quality-data/gemma-model \
  --labelled-corpus target/gemma-quality-data/miracl/sw-judged-fixture.json search
```

Repeat for Bengali with `bn-judged-fixture.json`, and for the code fixture
with `code-expanded-fixture.json` and the native `code` intent. Capture all
six JSON reports under the names in the machine-report directory, then run
`python3 -B scripts/summarize-gemma-judged-quality.py
target/gemma-quality-data/miracl target/gemma-quality-data`. The summary
refuses wrong model revisions, dimensions, corpus hashes, case order or
missing candidate labels. No model data, snippet bodies or Wikipedia text
are checked in.
