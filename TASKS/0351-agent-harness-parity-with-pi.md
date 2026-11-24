# 0351 Agent harness: close the main gaps with Pi

Status: open
Priority: medium
Subsystem: backend, agent
Depends on: none

## Context

A comparison of `fm-agent` with the Pi coding agent (`earendil-works/pi`, formerly
`badlogic/pi-mono`) on 2026-11-24 showed that fm-agent already matches or exceeds it on file and
shell tools, skills, `AGENTS.md`/`CLAUDE.md` instructions, multiple providers, document
conversion, `summarize_file`, a notes memory, secret redaction and sandboxed paths. Pi has no
built-in memory and no sandbox.

Four gaps matter for a reusable harness:

| Gap | Pi | fm-agent today | Subtask |
| --- | --- | --- | --- |
| Editing files | `edit`: exact-string replace | `write_file` creates new `.md`/`.txt` files only | 0352 |
| Long sessions | compaction: older turns summarised (auto and `/compact`) | `fit_messages` drops older turns and leaves a note | 0353 |
| Sessions | saved, resumable, forkable JSONL tree | terminal sessions in memory only; chat conversations saved (0324) without tool history | 0354 |
| Steering | messages queued while the agent works are injected between steps | only Stop, then a new question | 0355 |

Not in scope, by design in both harnesses: sub-agents and plan mode. Pi's extension API (40 hooks,
UI, provider payload rewriting) is not copied; where a subtask needs an extension point, it adds a
narrow typed seam to `fm-agent`, not a general hook system.

## Acceptance Criteria
- 0352, 0353, 0354 and 0355 are done.
- Each new capability lives in `fm-agent` (layer 0, no workspace dependencies) where it is
  host-neutral, with `fm-application` hosts (terminal assistant, chat) as thin adapters.
- Browser and Tauri hosts behave the same (spec rule 9).
- `crates/fm-agent` docs (crate-level comment, CLAUDE.md's capability paragraph) name the new
  seams.

## Implementation Notes
- Order: 0352 and 0355 are independent and small; 0353 before 0354, since a saved session should
  store its compaction summary rather than re-summarise on resume.
- Keep the harness reusable in other applications: no Procyon types in `fm-agent` signatures.

## Agent Notes
- 2026-11-24 claude: planned from the Pi comparison. Only Pi's README and extensions docs were
  read for it; its tool, session, compaction and steering behaviour is as known from earlier
  releases, so check `docs/sessions.md`, `docs/compaction.md` and `docs/rpc.md` in the Pi repo
  before copying a detail.
