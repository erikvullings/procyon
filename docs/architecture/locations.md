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
history. The `archive`, `search` and `sftp` schemes are reserved for planned providers and are
currently rejected as unsupported.

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

Parsing rejects null bytes, empty path segments, reserved Windows device names and a provider ID
that does not match the URI scheme. `join`, `parent` and `name` operate on complete decoded
segments rather than concatenating strings.

Normalization is lexical and performs no filesystem access. Callers supply an allowed root;
`.` and `..` are resolved and any result outside that root is rejected.
