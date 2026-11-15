# Zvec Rust SDK qualification

Task 0181 qualifies the official Rust SDK as an optional, worker-local derived-index backend. The
pinned release, and the exact identity the production bundle records for it, is:

- `zvec-rust = 0.8.1`
- `zvec-rust-sys = 0.8.1`
- Zvec native C API `0.8.1`
- Rust SDK tag commit `668f8270d3023bda0f3ed2ba650a508dca78d648` (`zvec-ai/zvec-rust` `v0.8.1`)
- Native Zvec revision `30d662db4d2467937d843443074b34743bcc9baa` (`alibaba/zvec` `v0.8.1`), the
  commit the SDK's release pins as its `vendor/zvec` submodule

Both Rust crates and native Zvec are Apache-2.0. A binary distribution must include the Apache
license and the native Zvec `NOTICE`, including its Unicode Character Database and `pyglass` MIT
attributions. Locally modified upstream files would also need to be identified.

The release evidence below was re-derived from the `v0.8.1` GitHub release for this pin. Task
0181's functional qualification and the API/FTS audit in this document were performed against
0.7.0; task 0334 tracks re-running them against 0.8.1.

## Linkage and default-build policy

The published Rust SDK always dynamically links `zvec_c_api`; its default `bundled` feature means
"download a native release archive during the Cargo build", not static linkage. The sys crate emits
an absolute build-directory rpath on macOS/Linux. Procyon therefore keeps `zvec-rust` behind the
`fm-semantic-worker/zvec` feature. Normal workspace builds do not download or link Zvec.

Reproducible release builds must:

1. Obtain the exact target archive from the Zvec Rust v0.8.1 GitHub release during the release
   build, never from the worker at runtime.
2. Verify the pinned release asset ID, byte length, SHA-256, exact archive member set, `TARGET`
   marker, loader byte length/SHA-256, binary format, architecture, and dynamic dependencies.
3. Set `ZVEC_AUTO_BUILD=0` and `ZVEC_LIB_DIR` to that verified build cache before invoking Cargo.
4. Copy the dylib/SO/DLL beside the packaged worker and set a relocatable runtime search path.
5. Record the immutable Apache-2.0 license and native NOTICE provenance and, where applicable, sign
   and notarize the native library.

`scripts/zvec-runtime-qualification.mjs` is the executable source of those pins. Cargo's own
`bundled` downloader is intentionally disabled for release builds because it does not verify a
published archive checksum before linking.

Do not rely on the SDK's source-build fallback for cross-compilation. It invokes host CMake without
a target toolchain file, target compiler, or target architecture settings.

## Qualified targets and deployment cost

| Target | v0.8.1 artifact | Compressed | Dynamic library | Result |
| --- | ---: | ---: | ---: | --- |
| macOS arm64 | yes | 11,013,728 B | 27,207,856 B | qualified |
| macOS x64 | yes | 12,340,298 B | 30,439,416 B | artifact published; not qualified by Procyon yet |
| Windows x64 MSVC | yes | 11,527,420 B | 29,406,720 B | qualified |
| Linux x64 glibc | yes | 16,156,796 B | 40,560,496 B | qualified |
| Linux arm64 glibc | yes | 14,519,213 B | 35,409,600 B | qualified |

The exact release inputs are:

| Target | GitHub asset ID | Archive SHA-256 | Loader SHA-256 |
| --- | ---: | --- | --- |
| macOS arm64 | `742882592` | `d2a1e5da3e45404947a4b1810466724c125f3be3e2161406e4c03ac43f421447` | `6a9c8c58fcddd4e82dc58f1a3206e5e5787277c1f5e4b3408aa3016544bcd322` |
| Windows x86-64 | `742882584` | `d375c6d96803d6925375e6698715db0c8c59da6969504c7b4382025380f6d857` | `25887240dbce5a5e1719f306dd881223fe27ee25c06c02048729e7b157198788` |
| Linux x86-64 | `742882589` | `c678f45db0de17650730b835390219f267b508f28e77291b381138bf74e455ad` | `f9d83ee18a04ef926a7a5177967c3cb79686eef9051187605d3f42e4b23814a0` |
| Linux arm64 | `742882586` | `71b03d3e088e701c7926330b4006987d2de02c83a180f1bda45030e1e63d7ca1` | `3b02683773d69d002ff61cfb861d24dac75ca8215490563c880f4db03d705ed9` |

The upstream GitHub release API publishes the archive digests. Loader digests were independently
calculated from those checksum-verified archives and are pinned by Procyon. The release archives
contain `TARGET` and the loader; Windows additionally contains the required
`zvec_c_api.lib` import library (110,282 B,
`0638d2a286595b313b21a1cd865a0010b872b3588191b640a330996467a07ee3`).

The allowed unbundled dependencies are system-owned:

- macOS arm64: CoreFoundation, `/usr/lib/libc++.1.dylib`, `/usr/lib/libSystem.B.dylib`.
- Windows x86-64: `dbghelp.dll`, `KERNEL32.dll`, `ole32.dll`, `RPCRT4.dll`, `SHELL32.dll`,
  `SHLWAPI.dll`.
- Linux x86-64: the x86-64 loader plus glibc, libdl, libm, libpthread, and librt.
- Linux arm64: the AArch64 loader plus glibc, libdl, libm, libpthread, and librt (0.8.1 links
  libdl, libpthread, and librt where 0.7.0 did not).

Release packaging fails if `otool -L`, `dumpbin /dependents`, or `readelf -d` reports a different
set. Native Zvec's immutable Apache NOTICE is
`332b1a498b446fab1232b671c2ba74102fc563c198dc6f53980d1282075958ad` (5,020 B) and carries the
Unicode Character Database and pyglass attributions. Both upstream Apache license files hash to
`43070e2d4e532684de521b885f385d0841030efa2b1a20bafb76133a5e1379c1` (11,356 B).

The `v0.8.1` release does publish an `x86_64-apple-darwin` prebuilt (asset `742882583`,
12,340,298 B), which the `v0.7.0` release lacked. That removes the packaging blocker but not the
qualification gap: Procyon builds no macOS x64 worker, so the optional Zvec component stays
unadvertised for Intel macOS until that target is pinned, built, and tested (task 0334).

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
release; task 0334 re-runs that audit against 0.8.1, whose release commit adapts the bindings to
grouped search and IVF product quantization. The safe Rust API provides every primitive needed for
Structured Knowledge Search:

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

The safe schema API audited at 0.7.0 does not expose a field lookup that would let Procyon read the
persisted vector metric. Startup therefore verifies every required field/index, executes dimension
and FTS schema probes, and relies on the authoritative manifest's cosine metric. Collections are
created only by this adapter; an externally replaced collection is unsupported and query failures
remain typed rather than silently changing scoring.

These APIs use the same native library on every qualified target in the table above. Native FTS
does not qualify macOS x64, whose prebuilt 0.8.1 now publishes but Procyon has not tested.

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
   schema fields. The pinned Zvec release does not expose FTS build completeness through
   `Collection::stats`.
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
