# 0358 Assistant session memory through the semantic index

Status: open
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
