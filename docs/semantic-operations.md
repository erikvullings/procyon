# Semantic subsystem operations

The semantic subsystem is optional. Removing it leaves ordinary file management, filename search,
content search, and baseline document viewing available.

## Installation and enrolment

- `pnpm dev:tauri` enables the deterministic semantic lifecycle simulator. It exercises consent,
  install, pause, migration, removal, and gating UI, but does not install a real worker, model, or
  index. The Settings screen labels this state as a development simulation.
- `pnpm semantic:bundle:dev` builds a host-platform developer bundle under
  `target/semantic-developer-bundle/<platform>-<architecture>`. The bundle contains the real worker,
  native Zvec runtime, two model packs, SHA-256 checksums, and a catalog signed by the repository's
  public development key. It is supported where Zvec 0.7 publishes a native runtime: Apple-silicon
  macOS, x86-64 Windows, and x86-64 or arm64 Linux. Intel macOS is unavailable.
- Developer artifact IDs and the catalog revision include a content-derived digest. Rebuilding
  changed worker, runtime, or model bytes therefore creates a distinct immutable artifact instead
  of colliding with an earlier bundle installed under the same workspace version.
- The semantic disk-use panel reports physical storage categories. The developer worker keeps
  normalized chunk text beside its authoritative records in the model-specific Zvec directory, so
  **Separate extracted-content cache** can remain at 0 B after successful extraction and indexing.
- `pnpm dev:tauri:semantic` rebuilds that bundle and starts the debug Tauri app with it. Open
  **Settings > Semantic**, review the development-only disclosure, and install the offered
  components. Enrolment and indexing still require explicit consent for each local root.
- Local OCR automation is disabled even when OCRmyPDF is installed. Start the developer host with
  `pnpm dev:tauri:semantic:ocr` to opt in for that process. Set
  `PROCYON_OCRMYPDF_EXECUTABLE` as well when `ocrmypdf` is outside `PATH`. On macOS install it with
  Homebrew (`brew install ocrmypdf`); on Linux use the distribution's OCRmyPDF package; on Windows
  install and run Procyon's local semantic developer environment through WSL. Procyon never
  downloads OCRmyPDF.
- **Settings > Semantic > Enrolled roots** lists the decoded paths of files that still require OCR
  after the latest completed reconciliation. The list clears when a later OCR-enabled
  reconciliation successfully indexes them.
- Once a model and an LLM profile are active, the command toolbar shows a chat-bubble action. It is
  also available as **Ask your files** in the command palette and through
  `Ctrl/Cmd+Shift+F`. It always opens Ask across the
  existing library. The question and answer workspace stays prominent; generation profile, evidence
  scope, privacy disclosure, retrieved evidence, and saved conversations are expandable. The final
  row keeps **Index current folder**, **Allow model knowledge**, and **Options and privacy** together.
  Each retrieved excerpt shows its cosine-similarity score; higher values are closer matches, but
  they are not calibrated probabilities. Indexing a folder shows the same recursive estimate, retention
  disclosure, budget warnings, and explicit consent used by Settings; cancelling returns to the
  question in progress. Press **Enter** to retrieve evidence and generate an answer, or use
  **Shift+Enter** for a multiline question. Questions, evidence excerpts, and answers remain
  selectable and have copy actions. Answers render sanitized Markdown while copying preserves the
  original Markdown text. Evidence sources and answer references open the containing folder,
  select the source file, and close Ask without discarding its state. **New question** explicitly
  clears that state. Detailed root administration remains under **Settings > Semantic**.
  Citation locations are rendered as page, line, slide, or block labels rather than serialized
  provenance. Ollama profiles use its native generation endpoint with hidden reasoning disabled so
  reasoning-capable models spend the configured answer allowance on visible output; any response
  that still reaches its length limit is rejected rather than presented as a complete answer.
- Local enrolments persist an opaque device/inode identity on Unix or volume/file identity on
  Windows. This lets Procyon distinguish a moved root from a different folder at the old path.
  Desktop startup safely fills this identity for legacy reachable local roots that lack it, without
  replacing existing identity evidence or changing consent, exclusions, or workspace references.

#### Developer-bundle models

The bundle offers a different model per profile, and the worker loads whichever model the host
installed and activated rather than a fixed one.

| Profile | Model | Download | Disk | Peak RAM |
| --- | --- | --- | --- | --- |
| Compact multilingual, Compact English | Deterministic token-hashing fixture | none | < 1 KiB | 8 MiB |
| Multilingual quality | `intfloat/multilingual-e5-small` | ~465 MiB | ~465 MiB installed (plus the same again in the build cache) | up to 1,600 MiB while the graph is loaded |

- The compact profiles keep a deterministic 384-dimensional token-hashing embedder. It exercises
  managed installation, authenticated IPC, conversion, ingestion, persistent vector storage,
  restart recovery, and retrieval plumbing without any download. It is not a trained model and says
  nothing about retrieval quality.
- **Multilingual quality** installs a real model: `intfloat/multilingual-e5-small`, MIT licensed,
  pinned to the immutable Hugging Face revision `614241f622f53c4eeff9890bdc4f31cfecc418b3`. It
  produces 384-dimensional unit-length vectors, accepts 512 tokens per input, and covers roughly a
  hundred languages, so a query in one language retrieves documents written in another.
- The figures above are the ones the signed catalog declares, so the installation preview, the
  free-space admission check, and this table always agree. The memory figure is the conservative
  rounded-up peak while the graph is resident, not a steady-state average.
- To switch an installed developer bundle, open **Settings > Semantic > Change embedding model**,
  select **Multilingual quality**, review the signed model identity, MIT license, and approximately
  465 MiB installation disclosure, then confirm the migration. Confirmation downloads and verifies
  the target pack without replacing the running model; activation happens only after the durable
  migration checkpoint, followed by an automatic reindex of enrolled roots. Ingestion and search
  then run real transformer inference locally on the CPU. Expect indexing to be markedly slower
  than the fixture, and the first query after a worker launch to pay a few seconds of model-load
  cost. Developer worker startup allows a bounded 30 seconds for that cold load before reporting
  failure.
- After a successful update, **Components in use** lists one active worker, runtime, and model.
  Procyon keeps the previous worker and model package locally as recovery copies; Settings places
  them in a collapsed **previous components retained** section. These copies are inactive but still
  count toward the reported semantic disk use.
- The model is applied the way E5 requires: indexed passages are embedded with the `passage: `
  prefix and search queries with the `query: ` prefix. Both prefixes are data in the installed
  model pack, so a model needing none is used unchanged. Inputs longer than the 512-token window
  are truncated rather than rejected.
- Each model owns its own catalog and vector index under the worker data root, so switching
  profiles never mixes incompatible embedding spaces. The device-local library identity, folder
  consent, exclusions, and workspace references remain stable across that switch; only the
  policy's exact model identity changes. Completing a model migration in the debug host updates
  that policy atomically under the library lock, stops the worker still holding the previous model,
  drops its connection, deletes any earlier index for the newly activated model, and then
  reconciles every available enrolled root into a clean index. Clearing an index when switching
  back prevents deleted documents or revoked roots from reappearing from stale vectors. Model
  activation is already durable at that point, so a failed restart or a failed root is logged with
  the same visibility as post-enrolment indexing rather than rolled back — check the development
  log and repeat **Include folder** for any root the log names. Unreachable roots are skipped and
  logged, and are picked up the next time they are reconciled.
- Indexes written by the earlier task-0190 layout lived directly beneath the worker data root
  (`catalog.sqlite` and `zvec/`) and were implicitly owned by the only model that existed then.
  On first launch after this change the worker moves them to `superseded-flat-index/` under the
  same data root and logs that it did. They belong to a superseded embedding space and are never
  queried again; delete that directory once you no longer want to inspect it.
- `pnpm semantic:model:fetch` downloads and verifies the pinned files on their own into
  `target/semantic-model-cache/intfloat--multilingual-e5-small/<revision>/`, reporting progress as
  it goes. `pnpm semantic:bundle:dev` calls it first and reuses that cache, so only the first build
  pays the download. Every file is checked against a pinned exact byte length and SHA-256 before it
  is accepted, and the completed pack is streamed and checked against those same pins immediately
  before its catalog checksum is signed. A mismatch fails the build rather than producing a bundle.
- Downloading happens only in that repository build script. The worker never contacts Hugging Face
  or any other network service: it reads the graph and tokenizer from the installed pack, and ONNX
  Runtime is linked into the worker executable rather than resolved from an ambient shared library.
- `pnpm semantic:model:verify` runs the real-model checks against the built bundle: offline load,
  384-dimensional unit-length output, cross-language ranking, truncation, cancellation, and an
  end-to-end ingest-and-query pass. Those tests are `#[ignore]` so an ordinary
  `cargo nextest run`/`cargo test` never depends on the download; the script sets
  `PROCYON_SEMANTIC_MODEL_PACK` from the built bundle and passes `--ignored`, and the tests fail
  loudly rather than pass silently if that variable is missing. Run `pnpm semantic:bundle:dev`
  first.
- The debug host performs one bounded indexing pass immediately after enrolment. If worker or
  provider indexing fails, consent remains enrolled and its reconciliation generation stays
  unchanged; inspect the development log, correct the reported problem, then repeat **Include
  folder** to retry without removing the root.
- Production desktop builds remain unavailable until the host injects a
  `ManagedSemanticComponentCapability`. This is an optional component pack, not a Lua plugin.
- Desktop-managed builds install only packages from the signed catalog. The preview reports exact
  package/model identity, download bytes, disk/RAM estimates, and the filesystem authority used.
- Before enrolment, estimate eligible files, extracted text, vectors, and any missing model bytes.
  `StorageAdmission` rejects the operation unless the configured free-space reserve remains.
- Server deployments use administrator-provisioned components, local-only enrolment policy by
  default, and hard tenant quotas. A tenant quota failure does not alter another tenant's catalog.
- Enrolment never occurs from a search. Roots require explicit recursive consent and remain
  independently excludable.

### Publishing a production component pack

The common production catalog contract is implemented; target payload production and desktop
activation remain tasks 0195 and 0196. A release input artifact has this fixed layout:

```text
catalog-input.json
artifacts/
  <content-addressed worker artifact ID>
  <content-addressed Zvec runtime artifact ID>
  <content-addressed model artifact ID>
```

`catalog-input.json` is a `ProductionCatalogManifest`. Its embedded managed-component catalog
records each payload's credential-free HTTPS distribution location, SPDX license and notice, exact
download/installed/RAM bytes, SHA-256, target, protocol, runtime requirements, and index schema.
The production wrapper adds an immutable public source URL and source revision for every artifact,
plus one exact worker-protocol/index-schema/converter/chunker/tokenizer/model identity. Production
IDs use the `procyon.semantic.*` component namespaces and include target, package version, and a
SHA-256 prefix; development IDs and the public developer key are never accepted as release trust.

1. Build worker/runtime artifacts for each supported target and publish immutable payloads through
   a catalog-ID-only artifact source. Do not accept user-supplied download URLs.
2. Evaluate the exact model revision and package set against the task-0188 baseline, then assemble
   `catalog-input.json` from the build outputs. The release tool enumerates the exact payload set
   and recomputes each file's length and SHA-256 before signing, so an unknown, missing, truncated,
   oversized, or changed file fails closed.
3. Invoke the reusable `.github/workflows/sign-semantic-catalog.yml` workflow from the release
   payload job. Store the base64-encoded raw 32-byte Ed25519 seed only as the protected
   `desktop-release` environment secret `SEMANTIC_CATALOG_SIGNING_KEY_BASE64`. Store the matching
   public key as `SEMANTIC_CATALOG_VERIFYING_KEY_BASE64`; release builds convert that public value
   to `PROCYON_SEMANTIC_CATALOG_VERIFYING_KEY_HEX`. The workflow writes both keys only below
   `RUNNER_TEMP`, restricts the private file to mode 0600, never places key material in command
   arguments or generated files, and removes the files in an `always()` step.
4. The signing job runs `pnpm semantic:catalog -- sign <input> <artifacts> <output>`, then performs
   an independent public-key-only
   `pnpm semantic:catalog -- verify <catalog> <signature> <artifacts> <public-key>` pass. Signatures
   cover RFC 8785 canonical JSON even though `catalog.json` is emitted as stable pretty JSON.
5. Construct `ComponentManager` with the platform app-data root and inject
   `ManagedSemanticComponentAdapters` for artifact reads, free-space checks, activation probing,
   indexing pause, worker quiescence, and authoritative index removal. Pass the resulting
   `ManagedSemanticComponentCapability` to `FileManagerService::with_semantic_component_capability`
   in the desktop host.
6. Publish the platform payloads alongside a direct-distribution release and run installed/absent,
   rollback, tamper, low-disk, and hardware smoke tests. Mac App Store builds must bundle executable
   capabilities or keep them unavailable; they may not download executable packs at runtime.

The production identity contract pins `intfloat/multilingual-e5-small` at revision
`614241f622f53c4eeff9890bdc4f31cfecc418b3`, tokenizer
`xlm-roberta-sentencepiece.614241f6`, converter
`docling-pdf/1036000+baseline/1`, chunker `structural/2`, worker protocol 1, and index schema 1.
No production payload set or release catalog is shipped yet; task 0195 produces the platform
artifacts, task 0196 embeds the matching public key and activates the desktop host, and task 0198
qualifies the installed result. The developer bundle
packs the same real multilingual model for local testing, but remains development-only. It
is platform-specific and may be copied as a complete directory to another developer using the same
OS and architecture. The recipient must use a debug build and point
`PROCYON_SEMANTIC_DEVELOPER_BUNDLE` at that directory. Its signing key is public, so the signature
only tests catalog verification; it establishes no publisher trust. Never redistribute it as a
production component pack. Remove it by uninstalling the semantic components in Settings and
deleting the copied bundle directory after the app exits.

#### Rotation, retention, rollback, and revocation

- **Routine rotation:** generate a new Ed25519 key offline, place only its base64 public key in the
  protected environment variable, and release an application that trusts it before using the new
  private seed. During a planned overlap, publish catalogs signed by both generations as separate
  immutable release assets; never overwrite a catalog or reuse a revision. Remove old trust only
  after every supported application line has an upgrade path.
- **Artifact retention:** retain every payload referenced by the current catalog and the immediately
  preceding catalog for at least the full supported rollback window. Content-addressed IDs make
  retention unambiguous. Garbage-collect only payloads absent from both retained catalogs and from
  supported installed states.
- **Rollback:** republish or select the previous immutable catalog and its unchanged payloads. The
  managed installer verifies the old signature and checksums and retains the last working worker
  during activation. Never edit a signed catalog in place; a metadata correction is a new revision.
- **Emergency revocation:** remove the compromised payload and catalog from distribution, disable
  semantic installation in the release channel, and publish a new application/catalog revision
  that omits the revoked artifact ID. If the signing seed may be compromised, rotate the embedded
  public key and ship that application update before resuming distribution. Existing signed
  catalogs are intentionally immutable and cannot be remotely rewritten, so the incident record
  must list affected catalog revisions and minimum safe application versions.
- **Reproducible local verification:** download `catalog.json`, `catalog.sig`, the `artifacts/`
  directory, and the 32-byte public key from independently authenticated release sources. Run
  `pnpm semantic:catalog -- verify catalog.json catalog.sig artifacts public-key.bin`. Success
  prints only the catalog revision; any signature, compatibility, file-set, length, or SHA-256
  mismatch exits non-zero. Re-running `sign` with the same manifest, payloads, and protected seed
  produces byte-identical `catalog.json` and `catalog.sig`.

## Diagnostics and privacy

Default logs include only opaque IDs or hashes, stage, timing, counts, component/model/profile
identities, and redacted error categories. They exclude queries, excerpts, filenames, prompts,
responses, credentials, headers, and HTTP bodies. A sensitive capture is local, previewed,
category-scoped, and expires after at most 15 minutes.

Remote LLM requests always show their profile, locality, scope, minimized metadata, and exact
content preview before transmission. The semantic worker itself has no network authority.

## Backup and recovery

Ordinary backup includes enrolment policy/exclusions, exact model manifest identity, profile
metadata without secrets, SKOS sources/accepted edits/review decisions/attachments, and saved
conversation/citation pins. Zvec indexes, extracted chunks, embeddings, generated summaries, and
concept annotations are rebuildable and may be omitted.

Whole-library exports use schema version 1, stable entry names, SHA-256 checksums, and an explicit
plaintext-content warning. Import rejects unknown schema versions, unsafe names, and any checksum
mismatch before applying data. Encrypt exported files with the platform's storage controls.

After a crash, reopen the authoritative catalogs first, recover or roll back incomplete journal
transactions, then resume deletion and ingestion jobs. A damaged or incompatible derived index is
quarantined and rebuilt from authoritative state; never reinterpret vectors under a different
model-space fingerprint.

## Evaluation and change control

The local evaluation suite stores query/relevance judgments on device and computes file recall@k,
chunk recall@k, mean reciprocal rank, and binary nDCG@k. It performs no telemetry. The repository
fixture covers multilingual recall, near duplicates, boilerplate diversity, structural citations,
incremental edits, summaries, scope isolation, unavailable sources, and concept labels.

Interactive Ask applies both an absolute cosine-similarity floor (`0.84`) and a relative floor
(`0.02` below the strongest in-scope extracted chunk). A 2026-09-06 local multilingual-E5
calibration against the enrolled TRIZ corpus observed a `0.872` peak for the SU-fields query and a
`0.826` peak for the unrelated negative-control query "How do I bake a sourdough croissant on
Mars?". The previous `0.25` floor retained 14 and 16 excerpts respectively; the calibrated policy
retains the close SU-fields evidence and rejects the negative control. This is a retrieval-policy
change only: it requires no reindexing and has no storage impact.

Chunking itself is structural, not embedding-driven semantic segmentation. Converters retain
paragraph, heading, page, slide, sheet, and section structure; the chunker packs compatible adjacent
units toward a 400-token target without crossing incompatible top-level boundaries. The full section
heading hierarchy is prepended to the chunk content used for embedding and is preserved for evidence
display.

Changes to model, chunker, converter, index, grouping, summary selection, or labelling threshold
require a before/after `EvaluationChangeReport` with distinct fingerprints, the same cases/cutoff,
an explicit migration description, and signed storage impact. Generated-answer fluency is not an
evaluation metric.

## Storage and unsupported formats

Storage diagnostics aggregate authoritative measurements by enrolled root and detected format,
separating active bytes from bytes pending cleanup. They do not trust caller-supplied totals.
Baseline conversion supports plain text, source code, Markdown, HTML, DOCX, PPTX, XLSX, and CSV.
PDFs with an extractable text layer use the deterministic, pure-Rust Docling Adapter by default,
with the original `lopdf` implementation retained for recoverable fallback. Image-only PDFs are
excluded from semantic indexing with actionable OCRmyPDF guidance; they are not counted as
retryable ingestion failures. When local OCR is explicitly enabled, only that `NoTextLayer`
outcome invokes OCRmyPDF. Procyon copies the bounded bytes into a private temporary directory,
passes neither the provider path nor credentials, gives the child only an allow-listed environment,
enforces the remaining conversion deadline, terminates the complete OCR process group on
cancellation, and removes input, output, and OCR scratch files on every exit path. Successful output
is converted again by deterministic Docling (with baseline extraction retained for OCR text-layer
encodings Docling cannot yet read) before publication. A later reconciliation retries previously
excluded PDFs automatically. Unsupported, encrypted, malformed, and over-budget documents remain
visible as typed skip or omission reasons.

Local embedding work is checkpointed in small durable batches. Interrupted ingestion reuses those
vectors without exposing them to search until the complete document generation is published.
Publication respects Zvec's 1,024-document write limit, and one document that exceeds the
five-minute ingestion fail-safe is recorded as failed without aborting reconciliation of the rest
of the enrolled root. Source conversion remains bounded at 64 MiB per document. Once a replacement
generation is published, superseded catalogue and Zvec records are reclaimed in bounded batches.

## Optional advanced packs

Advanced converters, acceleration backends, and rerankers are independent packs rather than
baseline dependencies. Each pack has its own signed manifest, artifact checksum, target triples,
worker-protocol and index-schema range, download/disk/RAM estimate, and baseline comparison. A
server accepts only capability families on its administrator allow-list. Installation verifies the
signature, compatibility, policy, and payload before activation; activation retains one rollback
version per capability family. Removing one family does not alter the others.

Advanced converters receive bounded bytes and trusted metadata, never paths, credentials, provider
handles, or network authority. Deterministic Docling is the built-in PDF-specific advanced-first
converter; other formats remain baseline-first. A promoted format-specific pack may explicitly
select advanced-first conversion; successful advanced output
then carries its own converter fingerprint, while pack absence, incompatibility, or recoverable
runtime failure falls back to the baseline. Cancellation, resource-limit, and encryption outcomes
are not hidden by fallback. Advanced output must preserve the task-0180 structure, report
provenance precision and omissions, and pass the same source, expansion, output, timeout, and
cancellation limits.

The Docling PDF integration is pinned and audited in
[`docling-pdf-evaluation.md`](docling-pdf-evaluation.md). Its deterministic text-layer Adapter is
pure Rust and is the default PDF path. The optional ML mode requires a Procyon-signed managed pack containing PDFium, ONNX Runtime,
layout/OCR/TableFormer models and tokenizers; release builds set `ORT_SKIP_DOWNLOAD=1` and point
Docling only at checksum-verified installed assets. Activating a converter pack returns its signed
affected formats and migration impact. The host must obtain consent for the disclosed
download/disk/RAM cost, rebuild affected PDFs into a candidate generation, and retain the previous
pack and active index until the candidate is complete. Removal or rollback restores baseline PDF
conversion without changing library enrolment.

Acceleration starts in CPU mode. Before sharing an existing index, the accelerated backend must
match CPU vectors within the configured tolerance. A different embedding space is exposed as an
explicit baseline/candidate fingerprint migration and requires building and activating a separate
index generation. Driver or runtime failure returns CPU vectors; it never publishes partial or
unlabelled accelerated vectors.

Reranking is local, quality-gated, bounded to the configured candidate count and text size, and
cancellable. Diagnostics report only model fingerprint, count, input characters, and elapsed time.
Without an active reranker, dense retrieval remains unchanged. `Semantic` remains dense-only;
`Hybrid` is a separately selected mode that combines independently ranked dense and lexical
results using deterministic weighted reciprocal-rank fusion. Fusion uses ranks rather than
incomparable raw scores, applies structured filters first, and rejects a request if any candidate
has a different tenant or library scope.

Every signed advanced-pack report contains comparable baseline and candidate measurements for
multilingual, OCR, exact-term, code, structured-document, duplicate, latency, memory, and storage
fixtures. Rerankers additionally require at least a 0.02 absolute nDCG improvement. Release
packaging tests verify all declared macOS, Windows, and Linux target triples and reject undeclared
targets; platform hardware smoke tests remain mandatory before publishing an accelerated pack.

## Troubleshooting and deletion

1. Check component/model compatibility and available disk reserve.
2. Inspect root coverage for pending, stale, excluded, skipped, failed, and unavailable counts.
3. Restart the worker; authenticated clients reconnect and incomplete staging generations remain
   invisible.
4. If the derived index is corrupt, preserve authoritative settings, remove only the identified
   derived data directory, and rebuild.
5. For exclusion, monitor every deletion category until complete. Shared vectors remain only while
   another authorized occurrence references them.
6. For complete removal, delete saved conversations/pins, vocabulary attachments and annotations,
   semantic catalog/extracted data, Zvec data, models/runtimes, then verify the deletion inventory.

Manual release verification covers keyboard and screen-reader operation for installation consent,
the enrolment tree, progress/errors, semantic results/evidence, summaries, Ask/citations, profile
setup, and SKOS review in both browser and desktop hosts. Platform packaging smoke tests run with
the optional subsystem installed and absent.
