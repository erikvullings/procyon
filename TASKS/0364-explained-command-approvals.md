# 0364 Command approvals that explain what the command does

Status: open
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
