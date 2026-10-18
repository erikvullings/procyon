# Bounded real-photo hard-negative follow-up

**Status: not release-qualified.** This is a ten-pair, one-host, model-level
image-to-caption discrimination probe, not a representative photo retrieval
benchmark. It adds **five distinct** photos to the previous five-photo
[baseline](embeddinggemma-labelled-2026-10-09.md), not eight: SugarCrepe
`swap_obj` pairs 37, 42, 43 and 44 in the follow-up rights search were already
in that baseline. The photos are not redistributed. The exact signed Gemma
candidate is from [run 37974984924](https://github.com/erikvullings/procyon/actions/runs/37974984924),
artifact **11640886748**; the native Rust FP32 CPU evaluator verifies all five
original checkpoint sizes and SHA-256 digests before loading them.

## Rights and judgements

[SugarCrepe at revision `0047054b243992f0fad63d6f64f7544862daf846`](https://github.com/RAIVNLab/sugar-crepe/tree/0047054b243992f0fad63d6f64f7544862daf846)
provides human-validated positive/adversarial-caption pairs under its MIT
annotation license. Its `swap_obj.json`, `swap_att.json`, and `replace_obj.json`
SHA-256 digests are respectively
`073cdb8e253d053614e80710834d9773b09dbc1dd0a412f6f9492262caa1dcad`,
`7a7ce04e5c4c80412b48c6f0c1347fb1a2f5d54c0be6aa429b64d390e442f8d0`,
and `9d299600639947e9d15281a9ded1382b376cf4ea088a8d08a1314bfe76e81898`.
Unlike generic unmatched COCO images, each negative here is an explicitly
validated hard caption for its own photo. This tests caption composition, **not**
which of multiple photos is relevant to a query. The selections are
opportunistic, not a held-out or statistically sampled split.

The five earlier photographs and their individual Flickr attribution and
licenses are recorded in the baseline report. The **additional** photos below
were retrieved through the TLS-valid COCO val2017 endpoint; each Flickr photo
page's *photoModel* license was rechecked on 2026-10-10, together with all five
earlier pages. Code 4 is CC BY 2.0 and code 5 is CC BY-SA 2.0. COCO's historical
license metadata is not a substitute for this live check.

| Category / pair | COCO ID | Live Flickr photo and credit | Current license | Downloaded JPEG SHA-256 |
| --- | --- | --- | --- | --- |
| `swap_obj/45` | 102805 | [danhurt](https://www.flickr.com/photos/danhurt/6101493857/) | CC BY-SA 2.0 | `66e6c4530c4a84e97b77bf32227f5f7dfebb1c22accb0e7fa5dbd009308a8c0a` |
| `swap_obj/92` | 11511 | [William Murphy (infomatique)](https://www.flickr.com/photos/infomatique/7556029168/) | CC BY-SA 2.0 | `4e24ed1528891621658ff01d23e8a74735cd18720d41f7fd2264044497ad2a0e` |
| `swap_obj/231` | 563349 | [Konstantin Zamkov](https://www.flickr.com/photos/zamkov/5588983985/) | CC BY 2.0 | `c0cae527104995431e0e90c67a0805089e2ccfeabcc968ac67f0c5290b783792` |
| `swap_att/192` | 175364 | [Eric Audige-Soutter (YHA Rowen)](https://www.flickr.com/photos/yhaconwy-yharowen/6267025876/) | CC BY 2.0 | `badb2570222f6900a65bfc9d39615216c683ce3c5d03afb85c692a3cf08f323c` |
| `replace_obj/93` | 447314 | [Dan Hughes (dghughes)](https://www.flickr.com/photos/dghughes/263967623/) | CC BY-SA 2.0 | `3fb6b82c5c04e6fd280a51bc5a17331c3f1c799531eb294873802f34e85c7c97` |

All ten image/caption pairs are checked by
`scripts/prepare-gemma-quality-fixtures.py DATA_DIR --media-photos` against
annotation and JPEG hashes; its separate `media-photos-fixture.json` includes
the pair IDs, pages and credits. The existing five-photo fixture and previous
result are unchanged. Live Flickr rights can change again: verify before
downloading or reusing any image.

## Observed results and open gates

The [machine report](embeddinggemma-media-2026-10-10/gemma-photos.json)
retains each individual correct-minus-swapped-caption cosine margin and
boolean outcome at 128/256/512/768 dimensions (report SHA-256
`e4245217777a11dc3e8e0fabe0a5ff228d13af1bbcbdf1265c9ec04e25cbc555`;
fixture SHA-256
`4243048f67caf46d3fb1cd4005e1aa6041ddb5a6ee42bdd2ff1bd70a5e92332c`).
On this same set, **8/10** positive captions outranked the validated hard
negative at **every** dimension. The original five remain 4/5, and the five
new photos are also 4/5. These are not an independent random sample and
cannot establish a general accuracy rate or uncertainty bound.

| Category / pair (photo ID) | 128d margin | 256d | 512d | 768d |
| --- | ---: | ---: | ---: | ---: |
| `swap_obj/4` (579635) | +.02114 | +.03397 | +.03021 | +.03039 |
| `swap_obj/37` (232348) | +.01716 | +.02278 | +.01999 | +.02233 |
| `swap_obj/42` (355610) | **-.00117** | **-.00997** | **-.00515** | **-.00619** |
| `swap_obj/43` (179174) | +.02967 | +.06038 | +.06538 | +.05921 |
| `swap_obj/44` (7511) | +.00468 | +.00963 | +.01164 | +.01087 |
| `swap_obj/45` (102805) | +.02962 | +.02995 | +.03139 | +.03141 |
| `swap_obj/92` (11511) | +.00982 | +.00808 | +.01586 | +.01333 |
| `swap_obj/231` (563349) | +.01203 | +.01377 | +.02345 | +.02016 |
| `swap_att/192` (175364) | **-.01626** | **-.03112** | **-.02949** | **-.02777** |
| `replace_obj/93` (447314) | +.02362 | +.03777 | +.04847 | +.04431 |

Photo 355610 repeats the original teddy-bear/sheep swap failure, and the
new kitchen attribute swap (175364) also ranks the hard negative higher
at all four dimensions. Margins are rounded here; the machine report
retains their full precision. No prompts or labels were tuned on these
failures. E5 has no signed image
encoder, so **no** E5 image ranking is implied. Embeddings are generated
from raw JPEGs with Gemma's image path, and captions with its `Search` task;
both vectors are truncated and normalized per dimension. This does not test
installed-worker/vector-index ranking, near duplicates, or photo-to-photo
relevance. The local host is an Apple M4 Max (macOS ARM); the other signed
targets were not run on these photos.

No audio/video quality result is claimed. [Clotho 2.1](https://zenodo.org/records/4783391)
has human positive audio captions and per-clip Freesound licenses, but the
examined release does not provide independently judged **hard negative**
relevance labels; treating every other clip as irrelevant would fabricate
negatives. Its caption license is also primarily non-commercial/attribution
and each source clip's rights require separate checking.
[Charades-STA](https://github.com/jiyanggao/TALL#charades-sta-anno-download)
provides positive query/temporal intervals, not independently judged
semantically hard **negative** intervals in the examined annotations;
underlying clip rights and bounded decodable originals have not been
verified. Neither lead met this evaluation's label-and-rights bar, so
no clips were downloaded. Likewise the expert-curated two-image/two-caption
[Winoground](https://huggingface.co/datasets/facebook/winoground) would
offer judged image/caption contrasts, but unauthenticated raw, resolve,
parquet, and preview endpoints returned HTTP **401** for its gated Getty
images; no terms were accepted and no token was used. No separately judged,
rights-verified **photo-to-photo** positives and hard negatives have been
obtained. A larger lawful benchmark and a separately judged, temporally
grounded audio/video set remain concrete release blockers. Do not enable,
publish, or alter the optional-package guard based on this slice.

## Reproduction

Obtain artifact 11640886748 while retained (previously scheduled expiry
2026-10-23), verify its signed macOS catalog and every selected payload with
the existing `semantic_production_catalog verify` command and its public
key, then stage the five verified original checkpoint files as described in
the [baseline](embeddinggemma-labelled-2026-10-09.md#reproduction).
Fetch only the three JSON files at the exact revision above as
`DATA_DIR/{swap_obj,swap_att,replace_obj}-media.json`. Recheck each linked
photo's current Flickr license before downloading the ten val2017 JPEGs from
`https://s3.amazonaws.com/images.cocodataset.org/val2017/{ID zero-padded to 12 digits}.jpg`;
store them as `DATA_DIR/photos/{unpadded-ID}.jpg`. The fixture builder
rejects any changed labels, image IDs, or JPEG bytes. Then run:

```sh
python3 scripts/prepare-gemma-quality-fixtures.py DATA_DIR --media-photos
cargo run --release -p fm-semantic-worker --features gemma-native \
  --example evaluate_gemma_native -- GEMMA_MODEL_DIR \
  --labelled-photos DATA_DIR/media-photos-fixture.json
```

On macOS, point `DYLD_LIBRARY_PATH` to the verified candidate's Zvec
runtime if needed. Do not substitute an unsigned runtime or use E5 text
aggregate metrics as an image quality result.
