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
becomes `report copy.pdf`, then `report copy 2.pdf`. The suffix precedes the full extension, so
`archive.tar.gz` becomes `archive copy.tar.gz`; dotfiles such as `.env` become `.env copy`.

Permanent delete enumerates directory trees without following symbolic links and shows the exact
planned totals before confirmation. Read-only entries require an explicit override. Cancellation
is observed between entries, and the terminal progress and audit record contain the same completed
item count. Audit JSON Lines records operation identity, source locations, and counts, never file
contents.
