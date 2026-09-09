# Semantic release qualification

## Decision

**NO-GO as of 2026-09-08.** Do not set the protected repository variable
`SEMANTIC_RELEASE_QUALIFIED` to `true`. Tagged desktop releases remain available, but skip semantic
payload construction, catalog signing and publication, and catalog embedding. Managed semantic
installation therefore remains unavailable in those installers. A manual workflow dispatch still
builds non-published semantic payloads, signed catalogs, and catalog-enabled installers so operators
can collect the evidence required to change this decision.

This report is the operator record for task 0198. A code-complete subsystem and developer-bundle
results are not substitutes for measurements from the exact signed production artifacts.

## Candidate identity

| Property | Candidate |
| --- | --- |
| Procyon revision reviewed | `ae1f811635f11b726262910d25b3aaae65e6654b` |
| Model | `intfloat/multilingual-e5-small` |
| Model revision | `614241f622f53c4eeff9890bdc4f31cfecc418b3` |
| Tokenizer | `xlm-roberta-sentencepiece.614241f6` |
| Dimensions / normalization | 384 / L2 |
| Converter | `docling-pdf/1036000+baseline/2` |
| Chunker | `structural/3` |
| Retrieval policy | single query, absolute floor `0.84`, strongest-candidate window `0.02`, maximum 8 documents, 2 chunks per document, 8,192 context tokens |
| Worker protocol / index schema | 1 / 2 |
| Zvec runtime | `zvec-rust` `v0.7.0` |
| Optional OCR | user-installed OCRmyPDF stable `>=16.0.0,<18.0.0`, disabled by default |

The final artifact IDs, byte lengths, SHA-256 digests, catalog revision, signature, and installer
digests are intentionally blank until a qualification run produces and retains them. Do not
substitute developer catalog identities.

## Evidence status

| Gate | macOS arm64 | Windows x86-64 | Linux x86-64 | Linux arm64 |
| --- | --- | --- | --- | --- |
| Signed production payload/catalog retained | Missing | Missing | Missing | Missing |
| Packaged worker handshake and offline model activation | Not run on qualification artifact | Not run | Not run | Not run |
| Exact task-0188 retrieval evaluation | Not run | Not run | Not run | Not run |
| Installed/absent and first-run | Not run | Not run | Not run | Not run |
| Upgrade and rollback | Not run | Not run | Not run | Not run |
| Corruption, offline, and low-disk | Not run | Not run | Not run | Not run |
| Cancellation and crash/restart | Not run | Not run | Not run | Not run |
| Uninstall retention and deletion | Not run | Not run | Not run | Not run |
| Keyboard and screen reader | Not run | Not run | Not run | Not run |
| Consent, progress, error, citation opening, deletion | Not run | Not run | Not run | Not run |
| Default-log and crash-report privacy inspection | Not run on packaged app | Not run | Not run | Not run |

macOS x86-64 is unsupported because Zvec 0.7.0 has no matching runtime. The universal macOS desktop
application must continue to report semantic functionality as unavailable on that architecture.
Windows installers and payloads remain unsigned under the existing desktop release policy; that
limitation must be presented in release notes and must not be mistaken for a qualified signed
artifact.

## Quality and threshold decision

The checked-in task-0188 fixture defines 12 cases spanning multilingual retrieval, duplicates,
boilerplate diversity, structural citations, edits, summaries, scope isolation, unavailable
sources, concepts, multi-facet retrieval, a negative control, and prompt-injection-shaped input.
No observations from the exact production package have been recorded, so file recall@10, chunk
recall@10, MRR, nDCG@10, negative-control false-positive rate, and grounded-answer citation
correctness are **not measured** for this candidate.

The developer TRIZ observations (`0.905` for `Su-fields`, `0.881` for the cup/hot-liquid question,
and `0.848`-`0.850` for hard unrelated controls) do not justify lowering the current `0.84`
absolute floor. The candidate remains at `0.84` with a `0.02` relative window. This makes no
pipeline, embedding-space, retained-vector, storage, or migration change. Any future threshold
change requires comparable before/after production observations and an explicit storage/migration
statement.

The bounded multi-query candidate remains developer-only and defaults to single-query because its
checked-in report uses normalized fixture timings rather than production measurements.

## Existing automated evidence

Repository tests already cover deterministic metric calculation, scope isolation, bounded
retrieval, catalog and payload integrity, interrupted download/resume, component lifecycle,
low-disk admission, cancellation/recovery, deletion/retention, OCR process containment, and
default-safe semantic diagnostics. Default diagnostic events have no free-text field and reject
path-like values; sensitive capture requires a previewed, scoped, expiring grant.

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
