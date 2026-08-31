# 0124 Narrow Location URI parsing in fm-domain

Status: done
Priority: medium
Subsystem: backend
Depends on: none

## Context

`Location` (`crates/fm-domain/src/location.rs`, ~750 lines) carries provider-specific URI parsing logic. `Location::parse()` has an if-chain dispatching on scheme to `ParsedFileUri`, `ParsedArchiveUri`, `ParsedSftpUri`, `ParsedSearchUri`. This means the domain layer knows about every provider's URI format. Every time a new provider scheme is added, the domain layer must be updated.

Additionally, `Location::validate_name()` and `LocalFileSystemProvider::validate_directory_name()` have duplicated directory-name validation logic.

The domain layer should carry an opaque URI and delegate scheme-specific validation to the provider that owns it.

## Acceptance Criteria
- `Location::parse()` reduced to minimal scheme extraction without provider-specific parsing rules
- Provider-specific URI validation delegated to the provider's scheme registration (e.g. `LocalFileSystemProvider` validates `file://`, `SftpFileSystemProvider` validates `sftp://`)
- Duplicated `validate_name()` logic consolidated — single source of truth
- `fm-domain` no longer needs to be updated when new provider schemes are added
- All existing `Location` contract tests pass
- Zero behavioural changes to URI parsing outcomes

## Implementation Notes
- The provider registration in `ProviderRegistry` is the right seam: during registration, each provider declares the schemes it handles and a validation function
- `Location::parse()` becomes: split at first `://`, return `(provider_id, uri)` — nothing more
- `Location::to_native_path()` and `from_native_path()` are `file://`-only and already error for other schemes — this can stay in the domain or move to `fm-vfs-local` depending on how clean the boundary is
- This may require a small API change to the `FileSystemProvider` trait (scheme registration hook)
- Consider: does `Location` become a simple `{ provider_id, uri }` struct? It currently adds ~400 lines of parsing logic on top of that.

## Agent Notes
