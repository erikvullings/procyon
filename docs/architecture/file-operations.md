# File-operation safety and naming

Directory copy planning is iterative, cancellable, and computes item and byte totals before writes
start. A destination equal to or nested below its source is rejected during planning. Execution
continues past individual child failures for copy and permanent delete, recording the affected entry
and completing with warnings; move's copy/delete fallback stays strict so a partial destination can
never authorize source deletion.

Pause and cancellation are cooperative scheduler signals. Executors retain their operation locks
and planned progress while paused, checking the pause token between plan items and at streaming
chunk boundaries. Cancellation wakes paused and conflict-waiting jobs, propagates the same
`CancellationToken` into provider calls, and is also observed during tree planning. Streaming copies
write to private temporary destinations and publish the final name only after a successful close;
cancellation removes the temporary entry and terminates as `Cancelled`, never `Failed`.

Move uses provider rename only when the provider reports the source and destination directory are
on the same filesystem. Otherwise it reuses recursive copy, verifies that the destination root is
present, and only then deletes the source. The forced-fallback integration path covers this logic;
an actual second-volume move still requires manual platform testing.

Duplicate places every selected entry beside its source. Names are deterministic: `report.pdf`
becomes `report.pdf (1)`, then `report.pdf (2)`. The suffix follows the complete original name, so
`archive.tar.gz` becomes `archive.tar.gz (1)` and dotfiles such as `.env` become `.env (1)`.

Permanent delete enumerates directory trees without following symbolic links and shows the exact
planned totals before confirmation. Read-only entries require an explicit override. Cancellation
is observed between entries, and the terminal progress and audit record contain the same completed
item count. Audit JSON Lines records operation identity, source locations, and counts, never file
contents.

## Split and combine (task 0257)

`splitFile` cuts a file into `<name>.001`, `<name>.002`, ... (three digits, growing past 999; at
most 10,000 parts per file) and `combineFiles` joins them again. Both stream through the VFS
provider traits in 256 KiB chunks, so size is bounded only by the target and either side may be
any provider. Each output is written to a private `.fm-split-*.tmp` / `.fm-combine-*.tmp` sibling
and published with `commit_copy`, which refuses an existing entry; conflicts use the shared
conflict dialog (skip, rename to `name (copy N).ext`, or overwrite). Overwriting a split also
removes stale higher-numbered parts of the replaced set and makes the job non-undoable. Failure or
cancellation removes every part already published by the failing item.

Combine detects the set from any selected part by listing its folder: all `<base>.NNN` files form
the set, and a gap (including a missing `.001`) is rejected before anything is written. A
manifest, when present, is verified while joining, before the output is published.

The manifest is `<name>.blake3`, plain UTF-8 text whose hash lines are accepted by `b3sum -c`:

```text
# procyon-split 1
# name: movie.mkv
# size: 2500000000
# part-size: 1000000000
# parts: 3
# blake3: <hex of the whole file>
<hex>  movie.mkv.001
<hex>  movie.mkv.002
<hex>  movie.mkv.003
```

With a manifest, a missing trailing part, a damaged part and a wrong total are all detected. Without
one only gaps can be detected, so a lost last part is not noticed. Undo removes exactly the files
the job created (verified by their known BLAKE3 hashes) and leaves the source or parts untouched.
