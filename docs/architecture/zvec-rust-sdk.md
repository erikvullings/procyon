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

