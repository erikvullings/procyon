# 0364 Command approvals that explain what the command does

Status: done
Priority: medium
Subsystem: frontend, backend, agent
Depends on: none

## Context

Maintainer (2026-11-25): agents ask for consent to run a piece of Python or an advanced bash
script that cannot be understood at a glance, so the approval is meaningless. If a command needs
approval, say what it does; and do not ask when there is nothing to decide.

Today: strictly read-only commands run without asking (`read-only-command.ts`); in auto-approve
mode (0356) everything but very dangerous commands runs (`dangerous-command.ts`); otherwise the
approval panel shows only the command line, and `run_command` takes only `command`.

## Acceptance Criteria
- `run_command` takes a required `explanation`: one or two sentences, in the person's terms, of
  what the command does and why (what it reads, changes, deletes or downloads). The tool
  description says so; a call without it gets a guiding error (0360 style).
- An effects summary computed by Procyon, not taken from the model, shown next to the model's
  explanation, so a wrong or misleading explanation is visible:
  - shell: from the existing tokenizer: programs run, folders written or deleted (resolved
    against the working folder and home), network use (curl, wget, ssh, git push…), privilege
    (sudo), and what `dangerOf` reports;
  - inline scripts (`python -c`, `python - <<EOF`, `node -e`, `bash -c`, heredocs) and scripts
    the command runs from the working folder: a static scan of the script text for file writes
    and deletes (`open(..., 'w')`, `os.remove`, `shutil.rmtree`, `Path.write_*`, `fs.rm*`…),
    subprocesses, network (`requests`, `urllib`, `http`, `socket`, `fetch`) and dynamic code
    (`eval`, `exec`, `__import__`, `getattr` on modules) — the last makes the summary say "cannot
    be read safely";
  - a script longer than about 20 lines also gets a short plain-language description from a side
    model call (`SideModel`), labelled as the model's reading.
- Fewer prompts, not more: a command whose effects summary is read-only (no writes outside a temp
  folder, no network, no subprocess, no dynamic code) runs without asking, Python and Node
  included, under the existing "run read-only commands without asking" setting. Everything that
  asks shows the explanation and the effects first, the full command folded below.
- Auto-approve keeps its rule (only very dangerous commands ask), and those prompts also show the
  explanation and effects.
- Tests: effects of typical shell lines and scripts (read-only python, a script that deletes, one
  with network, one with `exec`), read-only scripts run without asking, explanation required,
  side description only for long scripts.

## Implementation Notes
- The effects scan is a heuristic and must only ever widen what asks: anything it cannot read is
  "needs approval", never "read-only".
- Keep the scan in the frontend next to `read-only-command.ts` and `dangerous-command.ts`
  (they share the tokenizer); the side description is a backend call.

## Agent Notes
- 2026-11-25 claude: planned.
- 2026-11-25 claude: done.
  - `run_command` requires `explanation` (fm-agent `CommandRequest.explanation`, at most 600
    characters; a missing or blank one gets a guiding error). Shown as "The assistant's reason".
  - Maintainer (mid-task): what the command does must come from a fresh call that is not
    influenced by the conversation. So the description is a separate model call
    (`terminal_assist::describe_command`, Tauri `terminal_assist_describe_command`) that sees only
    the command and the scripts it runs, never the conversation or the assistant's explanation;
    the terminal asks for it only when it shows an approval, so commands that run at once cost no
    call. It replaces the planned "long scripts only" description.
  - Backend `terminal_assist/command_details.rs`: finds the script files a line runs (after an
    interpreter, or run directly as `./x.sh`; not `-c`/`-m` code) and reads them from the working
    folder through the folder tools (sandboxed, at most 3, 32 KiB each, truncation flagged);
    `TerminalAssistReplyDto.run_details { explanation, scripts }`.
  - Frontend `command-effects.ts` (shares the tokenizer exported from `dangerous-command.ts`):
    `effectsOf` (programs, writes, deletes, network, privilege, scripts) and `scanScript` for
    Python and Node (writes, deletes, network, subprocesses, dynamic code, paths outside the folder
    or the environment, modules outside a read-only list). `isReadOnlyScriptCommand`: one
    `python`/`node` command, no pipe or redirect, plain arguments inside the folder, inline code or
    a fully read script that only reads → runs without asking under the existing read-only
    setting. Anything it cannot read in full asks.
  - Approval panel: the assistant's reason, Procyon's effect lines, the separate description
    (with a "asking…" line until it arrives), and a long command or one that runs scripts folded
    under "Show command". i18n in 8 locales; `terminal.scriptEffects` allowlisted (placeholders
    only).
  - Not done: scripts run through other programs (`make`, `npm run`) are not read; shell scripts
    run as files are listed but not scanned (they ask). Not tried in the desktop app: the terminal
    exists only there, so the browser preview cannot show it.
  - Verified: fm-agent 111, terminal_assist 94 (new: details with script, description call sees
    only the command), command-effects 8 and drawer 2 new; all Rust (1975) and frontend (4228)
    tests pass; `pnpm lint` clean.

