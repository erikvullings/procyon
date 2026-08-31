# 0011 Archive library selection

Status: accepted

## Context

Task 0076 requires archives to behave as virtual folders, including safe mutation and encrypted
archives. Open-source licensing takes precedence over complete format coverage. The implementation
must work in packaged Axum and Tauri hosts without invoking an externally installed archiver.

## Decision

Use format-specific, permissively licensed Rust libraries behind `fm-archive`:

- `zip` (MIT) for ZIP browsing, streaming entry reads, creation, transactional rewrites, AES and
  legacy ZipCrypto interoperability;
- `sevenz-rust2` (MIT) for 7z browsing, creation and AES support;
- `tar` (MIT OR Apache-2.0), with Rust compression stream crates, for tar and compressed tar;
- `rars` (MIT OR Apache-2.0) for read-only RAR 4/5 browsing and reads. It does not expose safe
  archive rewriting, so mutation remains unavailable.

The implemented capability matrix is:

| Format | Detection | Browse/read | Passwords | Create/mutate |
| --- | --- | --- | --- | --- |
| ZIP | content signature | yes | AES and ZipCrypto read | yes, transactional rewrite |
| 7z | content signature | yes | AES read | create; existing archives are typed read-only |
| tar | `ustar` header | yes | not applicable | no; typed read-only result |
| tar.gz | gzip signature | yes | not applicable | no; typed read-only result |
| standalone gzip | gzip signature plus decompressed-header check | single member | not applicable | no; typed read-only result |
| tar.bz2 | bzip2 signature | yes | not applicable | no; typed read-only result |
| tar.xz | xz signature | yes | not applicable | no; typed read-only result |
| RAR 4/5 | content signature | yes | dependency-supported read | no; typed read-only result |

`fm-archive` exposes a truthful per-format capability matrix. Mutation always creates a complete
temporary replacement beside the source archive and publishes it atomically; archive-library
in-place update APIs are not used. Passwords live in an in-memory backend-session credential store
and are never encoded into locations, events, history, diagnostics or persisted state.

Before merging, dependency features and transitive licenses are checked from the resolved Cargo
lockfile. Default features not required by the supported matrix are disabled.

## Alternatives

- **libarchive**: permissively licensed and broad, but adds a C library and cross-platform packaging
  burden. It remains a fallback if the Rust libraries cannot meet required ZIP/7z behavior.
- **UnRAR or wrappers embedding it**: rejected because its usage restriction does not meet this
  task's open-source policy, even where the Rust wrapper itself is MIT/Apache-2.0.
- **Shelling out to 7-Zip, WinRAR or system tools**: rejected because availability, version,
  sandboxing and credential handling would differ between hosts.
- **Implementing codecs or encryption locally**: rejected explicitly by task 0076.

## Consequences

- ZIP and 7z are creation targets; ZIP is the only mutable archive format. Tar-family support is
  implemented without encryption.
- RAR files are browse/read-only; mutations return a typed unsupported-capability result.
- Feature coverage may differ by format, and UI actions follow advertised capabilities.
- Synchronous archive codecs run on blocking workers so application async executors remain
  responsive and cancellable at provider-defined safe points.

## Revisit conditions

- A permissively licensed Rust library gains safe RAR rewriting, or 7z in-place mutation, making the
  read-only rows in the matrix unnecessary.
- ZIP or 7z requirements outgrow what `zip`/`sevenz-rust2` can express, at which point libarchive
  becomes the fallback already identified above.
- A dependency's license or default feature set changes in a way the pre-merge lockfile check
  rejects.
