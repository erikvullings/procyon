# Bounded independently sourced EmbeddingGemma 2 retrieval probe

**Decision: still not qualified for release.** This is a local macOS ARM
model-level CPU comparison, not a four-target or product-index evaluation.
The [signed candidate run](https://github.com/erikvullings/procyon/actions/runs/37974984924)
and retained artifact **11640886748** supplied the exact Gemma originals and
E5 model pack. The macOS catalog signature and all eight catalog payload
digests were verified with the existing `semantic_production_catalog verify`
command and the release environment's **public** verification key before
extraction. No component was published or enabled.

## External labels and rights

| Task | Immutable source and checked bytes | Judgements and limitation |
| --- | --- | --- |
| Multilingual | [Mr. TyDi v1.1](https://github.com/castorini/mr.tydi), Apache-2.0 benchmark annotations (underlying Wikipedia passages retain their own attribution/share-alike terms); [corpus revision `3a3aa21`](https://huggingface.co/datasets/castorini/mr-tydi-corpus/tree/3a3aa212bbe94a8cc0dc858710a3dad49d532054), [topics/qrels revision `1d43c80`](https://huggingface.co/datasets/castorini/mr-tydi/tree/1d43c80218d06d0ef80f5b172ccabd848b948bc1). Swahili corpus SHA-256 `be45a839aa4445fb77ae8e43974a5310abc5665e1750bcedf7e7d5134e292fe1`, Telugu `6142ecfdfba4c77e60296bf3f62a48661db237057aa7c7facb4f92c213065726`. | First eight **dev** questions per language, 19 human-judged positive passages in total. Three lexical-overlap distractors per question are **unjudged**, not human-verified negatives. They may themselves be relevant; the ranking metric is conditional on this tiny candidate pool, not full-corpus recall. The original Waterloo archive currently redirects to login; the pinned Hugging Face snapshots are publicly accessible. |
| Code | [CodeSearchNet human relevance annotations](https://github.com/github/CodeSearchNet/blob/bb121a53a559e99a6849409355ee5c83803f2e87/resources/annotationStore.csv), MIT, SHA-256 `0340af32b551ceadb74fec147f97642b7fedf3ff039e38fb86baff49ee899846`. | Four realistic Python search queries; seven positive snippets and ten *explicit* grade-0 irrelevant snippets. Only passages with all available human grades >=2 or all 0 are included; grade 1, disagreements, and unjudged cross-query snippets are excluded from that query's ranking. Some snippets have just one judgement. Each source is fetched at its annotated Git commit; the selected sources' MIT/BSD/Apache licenses were checked at those commits. Snippets are **not** checked into this repository. |
| Text/image | [SugarCrepe `swap_obj.json`](https://github.com/RAIVNLab/sugar-crepe/blob/546eb51ecd55859f1d1888636bb9399a8df4cd92/data/swap_obj.json), MIT, SHA-256 `073cdb8e253d053614e80710834d9773b09dbc1dd0a412f6f9492262caa1dcad`. | Five real COCO validation photos, five human-validated swapped-object hard-negative captions (pair IDs 4, 37, 42, 43, 44). **Each** photo's Flickr page still showed CC BY 2.0 or CC BY-SA 2.0 on 2026-10-09; COCO's historical license metadata alone was *not* trusted. Photo bytes and authors/pages are pinned in the fixture builder; photos are not checked in. Flickr licenses can drift, so re-check each live page before re-downloading or using the photos. E5 has no image encoder; these are Gemma-only image/caption discrimination cases, not matched E5 image retrieval or near-duplicate testing. |

The five photographic attributions are [danramarch](https://www.flickr.com/photos/danramarch/8933634910/)
(CC BY 2.0), [ex_magician](https://www.flickr.com/photos/ex_magician/5926562644/)
(CC BY 2.0), [jepoirrier](https://www.flickr.com/photos/jepoirrier/2090506037/)
(CC BY-SA 2.0), [33979492@N00](https://www.flickr.com/photos/33979492@N00/7611201536/)
(CC BY 2.0), and [pajp](https://www.flickr.com/photos/pajp/173980729/)
(CC BY-SA 2.0), in that order. Their evaluation copies are unmodified.
COCO's [terms](https://cocodataset.org/#termsofuse) explicitly do **not**
grant rights to all dataset images; one other tested photo had changed from
COCO's recorded CC BY to CC BY-NC and was excluded.

## Method

`scripts/prepare-gemma-quality-fixtures.py` checks every source annotation,
qrels and corpus digest, streams the two Wikipedia passage corpora, selects
bounded lexical candidate pools, fetches only the four code queries' eligible
snippets from commit-pinned upstream files, and verifies the five photographic
byte hashes. The generated fixture JSON and external source content stay under
ignored `target/gemma-quality-data/`. Corpus SHA-256 values and per-case ranks
are in the machine reports beside this document.

Both models rank exactly the same eligible text/code IDs and query strings in
each generated fixture. E5 uses the signed candidate's 384d ONNX graph and
tokenizer in Python ONNX Runtime, mean pools attended token states, L2
normalizes, and applies the production `passage: ` / `query: ` prefixes.
Gemma uses its pinned native Rust FP32 CPU encoder, `Document` plus
`SearchQuery` or `CodeRetrieval` prompts, and truncates then normalizes both
sides at 128/256/512/768d. Both use exact cosine ranking with a stable
tie-break, *not* SQLite/Zvec/hybrid retrieval. This is a model **and runtime**
comparison, not isolated architecture or latency attribution. Different
model-owned prompts and input truncation remain unavoidable.
The local host is an Apple M4 Max (16 logical CPUs, 128 GiB RAM, macOS 27.0.1).
Neither throughput nor peak memory is compared here; see the separate
[four-target signed CPU resource report](embeddinggemma-signed-cpu-2026-10-09.md).

## Observed results

The [seven machine reports](embeddinggemma-labelled-2026-10-09/) include
exact fixture hashes, model/runtime identity, per-case ranks and photo cosine
margins. Each E5/Gemma text or code pair reports the **same** fixture SHA-256:
Swahili `bd63545ded82c77669444f64df4b89c562ff6e49e9da625d89e166a74c71933c`,
Telugu `e638dbb84ac1300b0a8f8af5598de96f8aa40aef5dd362afeeb7a8201e1e1dc1`,
code `60d5ad35065df87d93fcb9f07b992e2384276dd55a752f5c0a7eac33254a4cee`.
The photo fixture SHA-256 is
`460702f7fea6f3c081b2cec94cb02d89c20cafdf92056d51e42aaf333a3cb190`.

| Candidate pool | E5 ONNX 384d hit@1 / MRR | Gemma native 128d | 256d | 512d | 768d |
| --- | ---: | ---: | ---: | ---: | ---: |
| Swahili, 8 questions / 35 unique passages | 0.875 / 0.938 | 1 / 1 | 1 / 1 | 1 / 1 | 1 / 1 |
| Telugu, 8 questions / 26 unique passages | 1 / 1 | 1 / 1 | 1 / 1 | 1 / 1 | 1 / 1 |
| Human-graded Python code, 4 queries / 17 unique snippets | 1 / 1 | 0.75 / 0.833 | 0.75 / 0.813 | 0.75 / 0.833 | 0.75 / 0.833 |
| Real photos, 5 positive versus swapped-caption pairs (accuracy only) | n/a | 4/5 | 4/5 | 4/5 | 4/5 |

E5's Swahili question `swahili-27` ranked an *unjudged* passage
`64652#5` above the judged positive `64652#0` from the same article. It
cannot be called a verified E5 error without judging that other passage.
Gemma's `buffered file reader read text` code query ranked two explicitly
grade-0 snippets above its first positive (rank 3 at 128/512/768d and rank
4 at 256d); E5 ranked a positive first. In the photo test, Gemma preferred
the swapped-caption negative for photo `355610` (teddy bear/sheep) at
**every** dimension; the correct-minus-negative cosine margin ranges from
-0.00117 to -0.00997. Those failures must not be smoothed away by aggregate
scores. Four code queries and five photos cannot estimate population-level
quality, and these results do not show Gemma is better than E5.

For code, only the query's explicitly judged code snippets enter its rankings;
scores over other queries' unjudged code would fabricate negative labels.
Mr. TyDi's lexical distractors have no explicit negative judgement, so even a
high multilingual score **does not close** the hard-negative multilingual
gate. The real-photo pairs test whether the true caption outscores an
adversarial caption against the same photo, not photo retrieval among a
representative image corpus. No audio/video evaluation is claimed: lawfully
usable independently labelled media hard negatives have not been pinned.

## Reproduction

Obtain retained artifact `11640886748` from run `37974984924` while it is
available (scheduled expiry 2026-10-23). Verify its macOS catalog and eight
exact selected assets with `cargo run -p fm-semantic-components --example
semantic_production_catalog -- verify` and the release environment's **public**
key, as in the read-only lifecycle workflow. Stage the five verified Gemma
originals with their usual filenames (`model.safetensors`, `tokenizer.json`,
`config.json`, `processor_config.json`, `preprocessor_config.json`). Read
`model.onnx` and `tokenizer.json` from the verified E5 model pack using
`fm_semantic_components::ModelPack::read` (which verifies member SHA-256), or
fetch those same pinned original members with `node
scripts/fetch-semantic-model.mjs`; the E5 evaluator independently requires
their two pinned member digests. The native Gemma example independently
requires all five pinned original sizes/digests.

Download only the Swahili and Telugu `corpus.jsonl.gz` files from the above
corpus revision and each language's `ir-format-data/{topics,qrels}.dev.txt`
from the queries revision; name them `{language}-corpus.jsonl.gz`,
`{language}-{topics,qrels}.dev.txt`. Download the pinned
`resources/annotationStore.csv` as `code-annotations.csv` and SugarCrepe's
`data/swap_obj.json` at the commits above. After checking **current** Flickr
licenses, download COCO validation images `000000579635.jpg`,
`000000232348.jpg`, `000000355610.jpg`, `000000179174.jpg`, and
`000000007511.jpg` via the TLS-valid
`https://s3.amazonaws.com/images.cocodataset.org/val2017/` endpoint into
`target/gemma-quality-data/photos/` named by their unpadded numeric ID.
The preparer refuses unexpected source/photo bytes.

```sh
python3 scripts/prepare-gemma-quality-fixtures.py target/gemma-quality-data
python3.12 -m venv target/gemma-quality-venv
target/gemma-quality-venv/bin/pip install -r scripts/requirements-gemma-quality.txt
target/gemma-quality-venv/bin/python scripts/evaluate_e5_quality.py \
  target/gemma-quality-data/e5-model target/gemma-quality-data/code-fixture.json
cargo run --release -p fm-semantic-worker --features gemma-native \
  --example evaluate_gemma_native -- target/gemma-quality-data/gemma-model \
  --labelled-corpus target/gemma-quality-data/code-fixture.json code
```

Repeat the last two commands with `swahili-fixture.json` and
`telugu-fixture.json` using the `search` intent; for the photos use the
native example's `--labelled-photos photos-fixture.json`. On macOS the native
example may need `DYLD_LIBRARY_PATH` pointing to the **signed** candidate's
`libzvec_c_api.dylib`; never substitute an unverified runtime. Generated
machine reports retain the hashes, dimensions and per-case outcomes.

**Remaining gates:** an explicitly judged multilingual hard-negative pool
with a larger representative split; more code queries and licence-verified
source repositories; a larger, individually rights-verified photographic
hard-negative split including image-to-image and near-duplicate negatives;
lawful audio/video relevance labels if those modalities are to be offered;
and matched installed-worker/index quality on supported targets. This small
probe must not change the optional-package release guard.
