# Folder snapshot format (version 1)

Snapshots (task 0268) record a folder tree so it can be compared later, for example to verify a
backup copy or to detect silent corruption. They are UTF-8 text with one JSON object per line and
the conventional extension `.procyon-snapshot`.

```text
{"format":"procyon-snapshot","version":1,"root":"file:///data","createdAt":"2026-10-18T10:00:00+00:00","algorithm":"sha256","entryCount":3}
{"p":"docs","k":"d"}
{"p":"docs/a.txt","k":"f","s":120,"m":1700000000000,"h":"<sha-256 hex>"}
{"p":"b.bin","k":"f","s":4096,"m":1700000001000}
```

## Header (first line)

| Field | Meaning |
| --- | --- |
| `format` | Always `procyon-snapshot`. |
| `version` | Format version. A reader rejects a version newer than it knows. |
| `root` | URI of the folder that was recorded (informational; a comparison may target another folder). |
| `createdAt` | RFC 3339 creation time. |
| `algorithm` | `sha256` when files carry checksums, otherwise `null`. |
| `entryCount` | Number of entry lines. |

## Entries (one per line, sorted by path)

| Field | Meaning |
| --- | --- |
| `p` | Path relative to the root, `/` separated, not percent-encoded. |
| `k` | `f` for a file, `d` for a directory. |
| `s` | File size in bytes. |
| `m` | Modification time, milliseconds since the Unix epoch. |
| `h` | SHA-256 of the content (lower-case hex), when `algorithm` is `sha256`. |

Unknown fields are ignored, so later versions can add fields without breaking old readers.
Symbolic links are never recorded or followed.

## Comparison rules

An entry is **added** when it exists only now and **removed** when it exists only in the snapshot.
A file is **changed** when its size differs, its modification time differs by two seconds or more
(FAT stores two-second ticks), or - with checksums - its content differs. A file whose size and
modification time match but whose checksum differs is **corrupted**: the content changed without
any ordinary write, which is the signature of bit rot or a faulty copy.
