# Locations

Locations are provider-neutral, persistent addresses. They are not native `PathBuf` values:

```text
Location {
    provider_id: "local",
    uri: "file:///Users/erik/My%20Documents"
}
```

The local provider ID is `local`; its URI scheme is `file`. Keeping these identifiers distinct
lets provider dispatch remain stable while using the standard file-URI spelling in bookmarks and
history. Other registered schemes include `archive`, `search`, `sftp`, `ftp`, `ftps`, `webdav`,
`s3`, and `onedrive`.

`Location::parse()` only checks the structural `scheme://` prefix and keeps the URI opaque. It
retains the persisted aliases `file -> local` and `ftps -> ftp`, but does not know the set of
installed providers or their URI grammars. Each `FileSystemProvider` declares its schemes and
validates its own locations. `ProviderRegistry::parse()` is the admission boundary for raw URIs:
it routes by scheme, assigns the owning provider ID, and delegates validation. Registry resolution
also validates already-deserialized `Location` values before returning a provider.

This split lets a new provider register a scheme without changing `fm-domain`. Conventionally
hierarchical opaque URIs can also use `Location::join`, `parent`, and `name`; providers with a
specialized URI shape retain their own path rules.

## Local URI syntax

- POSIX paths use an empty authority, for example `file:///Users/erik/Documents`.
- Windows drive paths retain the drive as the first segment, for example
  `file:///C:/Users/Erik/Documents`.
- Windows UNC paths use the server as the authority, for example `file://server/share/dir`.
- Each native path component is encoded as one URI segment. Bytes outside the URI unreserved set
  are percent-encoded with uppercase hexadecimal digits; the drive-letter colon is retained.
- Unicode is preserved byte-for-byte on Unix, including decomposed names used by macOS.
- Long Windows paths gain the `\\?\` or `\\?\UNC\` native prefix when converted back once the
  path reaches the legacy Windows path limit.

Local-provider admission rejects null bytes, empty path segments, reserved Windows device names,
and a provider ID or scheme that does not belong to the local provider. `join`, `parent` and
`name` operate on complete decoded segments rather than concatenating strings.

Normalization is lexical and performs no filesystem access. Callers supply an allowed root;
`.` and `..` are resolved and any result outside that root is rejected.
