# 0358 Assistant session memory through the semantic index

Status: done
Priority: medium
Subsystem: backend, agent, semantic
Depends on: 0353, 0354

## Context

Maintainer's question (2026-11-25): since Procyon already has a semantic index (Zvec), should
sessions be stored there too, so the agent can use them as its memory?

Yes, but not by writing into Zvec directly. The semantic boundary (CLAUDE.md, "Semantic worker
boundary") keeps filesystem authority and consent in Procyon: the worker indexes what the semantic
library's consented folders hold, it never takes arbitrary data, and a folder exclusion revokes
query scope immediately. Notes already follow that path: they are Markdown files in the notes
folder, indexed when that folder has consent, and `search_notes` queries them through the
knowledge search scoped to the folder (`notes/semantic.rs`), with a text fallback.

Session memory should take the same path:
- each finished or compacted session leaves a small Markdown record (its compaction summary from
  0353 plus metadata: date, folder, questions asked, files touched), redacted like prompts;
- the records live in a `sessions/` folder under the notes folder (0337), so they are indexed
  exactly when the person allowed the notes folder to be indexed, can be read, edited and deleted
  as files, and disappear from search when deleted or excluded;
- the agent searches them with the existing notes search, semantically when available, by text
  otherwise.

This avoids a second storage path into Zvec, a second consent model and a second retention
policy, and keeps the memory inspectable.

## Acceptance Criteria
- A session record is written when a terminal session is compacted (0353) or closed after at
  least one answer, and when a chat conversation is compacted (0354): one Markdown file per
  session, updated in place, with front matter (`kind: session`, `started`, `updated`, `folder`,
  `host: terminal|chat`) and the latest summary. Off by default; a setting turns it on
  ("Remember assistant sessions in my notes folder"), and nothing is written without a notes
  folder.
- `search_notes` results say whether a hit is a note or a past session; a `kind` filter lets the
  model ask for sessions only (`search_notes { query, kind: "session" }`), and the prompt tells
  the model it may look up earlier sessions when the person refers to past work.
- Records never hold raw file contents or command output: only the summary, which is redacted.
- Deleting a record, or turning the setting off and removing the folder, removes it from search
  (by the existing index cleanup).
- Tests: record written on compaction and close, updated not duplicated, redaction, search kind
  filter, nothing written when the setting is off or there is no notes folder.

## Implementation Notes
- Writer: `NotesService` owns writes to the notes folder only through reviewed proposals today
  ("it never writes notes"). Session records are not notes; add a narrow `SessionRecords` writer
  next to it that writes only `sessions/*.md` under the notes root, through the VFS provider.
- Index: no change to the worker; the notes folder's existing consent and ingestion cover the new
  files.
- Retention: keep at most N records (setting), oldest removed.

## Agent Notes
- 2026-11-25 claude: planned. Direct Zvec writes were considered and rejected for the reasons
  above (consent, authority, inspectability).
- 2026-11-26 claude: done.
  - Writer `notes/sessions.rs` `SessionRecords` (next to `NotesService`, which still never writes
    notes): one `sessions/<started-date>-<slug>-<id tag>.md` per session, found again by a stable
    SHA-256 tag of the session id and rewritten in place. Front matter `kind: session`, `host`,
    `folder`, `started`, `updated`, `source: procyon-assistant`, tag `assistant-session`; body =
    summary, the questions (first line each, at most 20) and the file paths the tools touched
    (paths only). The whole file is redacted. Lists only `sessions/`, never walks the notes
    folder. Retention: at most 200 records; only records it wrote (`kind`+`source`) are pruned,
    to the trash.
  - Setting `assistantSessionMemory` ("Remember assistant sessions in my notes folder", off by
    default; Settings, 8 locales). `SessionRecords::for_settings` gives a writer only with the
    setting on and a notes folder set.
  - Terminal (`terminal_assist/records.rs`): a record after automatic compaction (0353) and
    `/compact`; on close (tab, `/new`, `/clear`, `/resume`, `/fork`) the session is dropped at
    once and, when the model answered, summarised in the background (`fm_agent::
    summarize_conversation`, 90 s limit) into its record. `Session` keeps started/folder/
    questions/files as they happen and rebuilds them on resume and fork.
  - Chat: a record when a conversation gets a new summary (0359 compaction).
  - `search_notes { query, kind: "note" | "session" }` (`fm_agent::NoteKind`), filtered before
    the limit; hits are labelled `[note]`/`[session]`; the description tells the model to search
    sessions when the person refers to earlier work. Session records are never offered as skills.
  - Index: unchanged; the notes folder's consent and ingestion cover `sessions/`.
  - Deviations: retention is the constant 200, not a setting (no UI asked for); chat records
    start at their first summary (conversations store no creation time).
  - Review (standards + spec sub-agents) fixed: close no longer blocks on the model; resume/fork
    keep the record's bookkeeping; `started:` key; pasted text below a question stays out;
    pruning only touches own records; shared `touched_paths` for both assistants; folder errors
    named; CLAUDE.md and the notes module doc mention the writer.
  - Verified: fm-agent + fm-application 1767 pass (new: records written/updated in place,
    redaction, first line only, pruning keeps the person's files, setting/notes-folder gate, close
    with and without an answer, `/compact`, chat compaction, kind filter, conversation summary);
    frontend settings/i18n/app 604 pass; clippy and Biome clean.

