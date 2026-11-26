# 0365 "Look things up" toggle, and local pages the person names

Status: done
Priority: medium
Subsystem: frontend, backend, agent
Depends on: 0363

## Context

Maintainer (2026-11-25), after 0363:
- "Let the assistant look in this folder" no longer describes the chat toggle: it also turns on
  notes, skills and web pages. "Trust this folder" was suggested; trust usually means letting a
  folder's code and settings act (editors), which this toggle does not, so it becomes "Let the
  assistant look things up", with a tooltip naming what it covers. (The folder's
  `AGENTS.md`/`CLAUDE.md` instructions load with it; a separate trust switch for those is a
  possible follow-up.)
- `web_fetch` refuses addresses on this machine and the local network, but the person may be
  developing something there and want the assistant's opinion. The refusal exists because there
  is no prompt per page: a page or file the model reads could send it to a local service
  (Ollama, a dev server, a router) and its answer would reach the chat endpoint.

## Acceptance Criteria
- Chat toggle reads "Let the assistant look things up" (8 locales), with a tooltip: "Lets the
  assistant read this folder, your notes and skills, and public web pages".
- A local address (loopback, private network, `.local`, `localhost`) is read when the person
  named its host in a message they typed in this conversation or terminal session — the
  question text only, never attached context, files or pages, which a third party could have
  written. A mention with a port allows that port; one without allows any port of the host.
- Otherwise the tool result tells the model to ask the person to name the address; once they
  do, it is allowed for the rest of the conversation. No new approval mechanism.
- Local pages keep `http` (dev servers rarely serve https); public pages are still upgraded.
- Always refused: link-local addresses (cloud metadata lives there: `169.254.169.254`,
  `fd00:ec2::254`) and metadata host names (`metadata.google.internal`, `100.100.100.200`).
- Redirects are checked hop by hop with the same rule.
- Tests: host extraction from messages (URLs, `host:port`, bare `localhost`, IPs, `.local`,
  nothing from context), allowed and refused local fetches against a local test server,
  metadata always refused, http kept for local, toggle text.

## Agent Notes
- 2026-11-26 claude: done.
  - The chat toggle already read "Let the assistant look things up" in all 8 locales (only the
    key is still `lookInFolder`); added the tooltip `chat.lookThingsUpHint`.
  - `agent_host/web.rs`: `named_hosts(texts)` takes URLs and bare `localhost`, local IPs and
    `.local`/`.lan`/`.home.arpa`/`.localhost` names (with an optional port) from the person's own
    words; `WebReader::allowing(named)`. `checked_addresses` refuses link-local and cloud
    metadata (`169.254/16`, `fe80::/10`, `fd00:ec2::254`, `100.100.100.200`,
    `metadata.google.internal`) always; a local host that was not named gets a result telling
    the model to ask the person to name it; a named one is read, over plain http when asked so
    (public http is still upgraded to https). Redirects are checked hop by hop.
  - Terminal: the session's questions (`Sessions::questions`), built after the current question
    is recorded. Chat: the conversation's person messages, content only, never attached context.
  - Verified: fetcher 8 tests (extraction, named vs unnamed, metadata even when named, a named
    local server read over http against a real 127.0.0.1 listener), fm-application chat,
    terminal and agent_host 180, frontend chat/i18n 163; clippy and Biome clean.

