# File-copy metadata preservation

File copy writes bytes into a private destination-side temporary file and publishes that
file only after the stream is closed. For a non-overwriting copy, publication uses an atomic
same-filesystem hard-link operation followed by removal of the private name; an existing destination
therefore wins the race without being overwritten. Explicit overwrite uses the platform's rename
replacement semantics.

The local provider preserves the source file's logical contents, last-access time, last-modified
time, and permission object. On Unix this includes the mode bits. Creation/birth time, ownership,
ACLs, extended attributes, alternate data streams, Finder metadata, and platform file flags are not
currently copied. Sparse-file layout is best-effort: the logical bytes and length are preserved, but
the streaming fallback may allocate holes.

For directory trees, directories are planned iteratively and their metadata is applied in reverse
order after their children. The request's `symlinkPolicy` is explicit: `copyLink` (the default)
copies the link object without following it, while `copyTarget` resolves the target and uses stable
entry identities to stop directory cycles.

The provider advertises timestamp and permission preservation separately. A future provider may
omit either capability and document its own supported subset. Server-side cloning is also
capability-gated. The local provider uses its native filesystem copy for small files. On macOS,
files of at least 1 MiB first ask `cp -c` for an APFS clone and fall back to the native copy when the
volume does not support it. Read-only sources use the bounded-memory streaming path so the private
destination remains writable until timestamps have been applied; permissions are applied last.

Temporary names are used because a streaming or native copy can be interrupted after only part of
the file exists. Publishing the private name only after the copy closes prevents observers from
mistaking partial bytes for a completed destination. They are not needed for atomic metadata-only
operations such as same-volume rename.
