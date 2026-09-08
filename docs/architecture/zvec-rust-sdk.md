# Zvec Rust SDK qualification

Task 0181 qualifies the official Rust SDK as an optional, worker-local derived-index backend. The
qualified versions are:

- `zvec-rust = 0.7.0`
- `zvec-rust-sys = 0.7.0`
- Zvec native C API `0.7.0`
- Rust SDK tag commit `733e0bc82e02a0c63202bff594a7f4530520dfd0`
- Native Zvec tag commit `8321c1314a559fd5f909e92498f43e5194bf9b99`

Both Rust crates and native Zvec are Apache-2.0. A binary distribution must include the Apache
license and the native Zvec `NOTICE`, including its Unicode Character Database and `pyglass` MIT
attributions. Locally modified upstream files would also need to be identified.

## Linkage and default-build policy

The published Rust SDK always dynamically links `zvec_c_api`; its default `bundled` feature means
"download a native release archive during the Cargo build", not static linkage. The sys crate emits
an absolute build-directory rpath on macOS/Linux. Procyon therefore keeps `zvec-rust` behind the
`fm-semantic-worker/zvec` feature. Normal workspace builds do not download or link Zvec.

Reproducible release builds must:

1. Obtain the exact target artifact through the signed semantic-component catalog.
2. Verify its pinned checksum before invoking Cargo.
3. Set `ZVEC_AUTO_BUILD=0` and `ZVEC_LIB_DIR` to the verified artifact.
4. Copy the dylib/SO/DLL beside the packaged worker and set a relocatable runtime search path.
5. Include and, where applicable, sign the native library and required notices.

Do not rely on the SDK's source-build fallback for cross-compilation. It invokes host CMake without
a target toolchain file, target compiler, or target architecture settings.

## Qualified targets and deployment cost

| Target | v0.7.0 artifact | Compressed | Dynamic library | Result |
| --- | ---: | ---: | ---: | --- |
| macOS arm64 | yes | 8,135,328 B | 23,146,352 B | qualified |
| macOS x64 | **no** | - | - | packaging blocker |
| Windows x64 MSVC | yes | 8,839,865 B | 26,570,240 B | qualified |
| Linux x64 glibc | yes | 13,331,422 B | 36,854,864 B | qualified |
| Linux arm64 glibc | yes | 11,784,274 B | 32,470,624 B | qualified |

The Rust README claims macOS x64 support, but its v0.7.0 release has no x64 macOS artifact and the
target is commented out of the SDK's release and CI matrices. Procyon must not advertise the
optional Zvec component for that target until it has a pinned, tested artifact.

## API and recovery evidence

The feature-gated `zvec_storage` tests exercise the actual SDK and native library:

- collection create/open and schema inspection;
- FP32 cosine vectors with FLAT and HNSW;
- inverted scalar fields and structured filter compilation;
- insert, update, upsert, delete, bounded top-k query, and snapshot iteration;
- optimize (Zvec's segment merge/index rebuild operation), flush, and reopen;
- concurrent read-only handles and Procyon's explicit single-writer lock;
- forced worker termination followed by WAL recovery, optimize, and another flush.

The adapter rejects zero or excessive top-k values before entering the SDK. Zvec exposes no offset
or pagination API. SQLite remains authoritative: Zvec candidates are always checked against one
SQLite publication snapshot before occurrence evidence can leave worker storage.

## Native FTS and rank fusion audit

Task 0201 audited the exact pinned `zvec-rust` 0.7.0 source rather than APIs from another binding or
release. The safe Rust API provides every primitive needed for Structured Knowledge Search:

- `IndexParams::fts(tokenizer_name, filters, extra_params)` creates an FTS index on a
  `DataType::String` field;
- `SearchQuery::fts(field_name, &fts, top_k)` executes bounded FTS without a vector;
- `Fts::set_match_string` accepts ordinary terms, while `Fts::set_query_string` accepts Zvec's
  explicit query syntax;
- `SubQuery` represents either a dense-vector or FTS candidate route;
- `MultiQuery::set_rerank_rrf(rank_constant)` and `Collection::multi_query` fuse independent ranks
  without combining BM25 and cosine scores;
- `Collection::add_column`, `create_index`, `drop_index`, and `optimize` support runtime schema
  changes;
- `CollectionSchema::has_field`, `has_index`, and `FieldSchema::index_type` expose FTS index
  presence; `Collection::stats` reports document count and vector-index completeness but does not
  list the FTS index in the pinned SDK's observed output.

Prebuilt packages include the cppjieba dictionary, which `initialize` discovers for the documented
`jieba` tokenizer. The default tokenizer is proven by the pinned crate's FTS tests and is the safe
initial choice for space-delimited English, Dutch, German, and French content. The Rust API does
not enumerate other tokenizer/filter names or expose the configured tokenizer through schema
inspection, so Procyon must not guess filter names. The selected FTS tokenizer and configuration
must be recorded in the worker-owned manifest and changed only through a schema migration.

The safe 0.7.0 schema API does not expose a field lookup that would let Procyon read the persisted
vector metric. Startup therefore verifies every required field/index, executes dimension and FTS
schema probes, and relies on the authoritative manifest's cosine metric. Collections are created
only by this adapter; an externally replaced collection is unsupported and query failures remain
typed rather than silently changing scoring.

These APIs use the same native library on every qualified target in the table above. Native FTS
does not remove the existing macOS x64 packaging blocker.

## Existing schema and migration decision

Schema version 1 of `procyon-semantic-records` is vector-only. It contains inverted scalar indexes
for tenant, library, root, optional workspace, media type, modification time, optional concept, and
generation, plus one FP32 cosine `embedding` field using FLAT or HNSW. Zvec documents do not contain
chunk text, title, or section metadata. Inserts, updates, and upserts replace the scalar/vector
document under the stable record ID; deletes remove that primary key. Dense queries use
`SearchQuery::new`, apply a mandatory tenant filter plus optional scope filters, and return candidate
IDs for SQLite reauthorization.

The authoritative SQLite `records` and `vectors` tables retain every chunk's complete embedding
input, display excerpt, provenance, structural metadata, and vector. Zvec remains disposable
derived state. Consequently, the version-1-to-version-2 migration will use a staged rebuild rather
than mutate the live collection:

1. Detect the vector-only schema and manifest version.
2. Create a new version-2 collection at a staging path with content/metadata FTS fields and the
   existing vector/filter fields.
3. Stream authorized records and vectors from SQLite in bounded batches; no source conversion or
   embedding call is required.
4. Flush, optimize, verify the rebuilt document count, and inspect the expected FTS and vector
   schema fields. Zvec 0.7.0 does not expose FTS build completeness through `Collection::stats`.
5. Atomically publish the new collection and manifest only after validation.
6. Retain the old version-1 directory until publication succeeds, then reclaim it through the
   existing deferred-cleanup path.

Although `add_column` plus `create_index` is available, every version-1 row would still need a text
backfill. A staged rebuild gives an unambiguous rollback boundary and avoids exposing a partially
built FTS index. Interrupted staging is restartable; failure leaves the old dense index untouched.
Mixed versions remain readable through the existing dense route until their rebuild completes.

FTS is independently queryable. If an embedding runtime is unavailable, incompatible, or fails
before execution, the application may request FTS-only retrieval and must report the fallback.
Semantic-only requests still return a typed unavailable result; hybrid requests degrade explicitly
to FTS rather than becoming an empty search.
