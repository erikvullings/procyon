# 0356 Assistant auto-approve mode

Status: done
Priority: high
Subsystem: frontend, agent
Depends on: none

## Context

Part of 0351, added at the maintainer's request (2026-11-24): "I don't like confirming on each
command, especially when nothing exciting is asked. Perhaps only request permissions for very
dangerous commands, like a rm of the whole disk or the user folder (but not a single file or
`/tmp` entries)."

Today only strictly read-only commands run without asking (`read-only-command.ts`, task 0336);
every other command, every file creation (0338) and every edit (0352) asks.

## Acceptance Criteria
- A per-terminal-session auto-approve mode, off by default, turned on and off with `/auto` and
  with a toggle in the assistant's controls; the controls show when it is on.
- While it is on, commands run without asking, and file creations and edits are approved for the
  session, except a command that is **very dangerous**, which still asks:
  - deleting, moving away, overwriting, or recursively changing owner/permissions of the
    filesystem root, the home folder or an ancestor of it, a system folder (`/System`, `/usr`,
    `/bin`, `/sbin`, `/etc`, `/var` except `/var/tmp` and `/var/folders`, `/Library`,
    `/Applications`, `/opt`, `/private` except its tmp folders, `C:\`, `C:\Windows`,
    `C:\Program Files`...), or the session's working folder itself;
  - a target the check cannot resolve: a variable, command substitution, `~user`, or a glob at
    one of those folders;
  - disk-level tools (`dd` to a device, `mkfs`, `diskutil erase*`/`partitionDisk`, `fdisk`,
    `format`), `sudo`/`doas`, `shutdown`/`reboot`/`halt`, a fork bomb, piping a download into a
    shell (`curl … | sh`).
  - Not dangerous: `rm` of a single file or a folder inside the working folder or below the home
    folder, entries under `/tmp`.
- The person sees each auto-approved action as a step line in the terminal, so nothing happens
  unseen.
- Tests: a table of dangerous and harmless lines, including quoting, `--` and `-rf` spellings,
  `~`, `$HOME`, `..` that climbs to home, and chained commands (`cd / && rm -rf *`).

## Implementation Notes
- `frontend/src/features/terminal/terminal-drawer.ts`: `presentReply` decides between auto-run
  and the approve panel; `answerWrite` answers a write request.
- New pure module `dangerous-command.ts` beside `read-only-command.ts`.
- Unknown is dangerous: a line the check does not understand asks, as today.

## Agent Notes
- 2026-11-24 claude: done.
  - `frontend/src/features/terminal/dangerous-command.ts`: `dangerOf(line, { cwd, home })`
    returns a `DangerKind` or `undefined`. Own shell tokenizer (quotes, escapes, `;`/`&&`/`||`/`|`,
    redirections, `$HOME`/`~` expansion, Windows drive paths keep backslashes); follows `cd` within
    the line; unwraps `env`/`nice`/`timeout`/`command`; reads `sh -c`/`eval` strings recursively.
    Globs and `find` start folders count as "the entries of" their folder, so `rm -rf /tmp/*` and
    `rm -rf *` in a project go, `rm -rf ~/*` asks. Inside the known home folder anything goes except
    the home folder and the working folder themselves; without a known home, `/Users/x` and
    `/home/x` ask.
  - `terminal-drawer.ts`: `LiveTerminal.autoApprove` (per tab, never saved), `/auto [on|off]`, an
    "Auto" toggle in the status line (shown once the assistant was used, or while on), the approve
    panel says why a dangerous command still asks. `homeDirectory` attr from the shell state.
  - Backend: `TerminalAssistRequestDto.auto_approve`; a write is created without asking when set
    (still only when the file-creation setting is on). Edits (0352) follow the same flag.
  - The terminal's own command echo is the visible trace of an auto-run command; created files
    show their usual step line.
  - Verified: 80 `dangerous-command.test.ts` cases; drawer test "in auto-approve mode runs a
    command at once, but still asks for a dangerous one"; backend
    `auto_approve_creates_files_without_asking`; full frontend suite 4192+ passed; tsc and Biome
    clean; i18n in all 8 locales ("Auto" allowlisted).
