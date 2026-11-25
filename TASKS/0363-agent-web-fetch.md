# 0363 Read-only web_fetch tool for the assistants

Status: open
Priority: medium
Subsystem: backend, agent
Depends on: none

## Context

Maintainer (2026-11-25): do we, OpenCode or Pi have browser use, or should skills guide it?
fm-agent has no web tool. OpenCode ships a read-only `webfetch` tool (URL to text) and leaves
browser automation to MCP servers; Pi has no web tools by design and relies on skills plus the
shell (the `browser-tools` skill drives Chrome over CDP). Decision: a built-in read-only
`web_fetch` for reading pages (docs, issues, model cards), which the chat needs because it cannot
run commands; full browser automation stays with skills in the terminal assistant.

Consent must stay light (maintainer): reading is low-risk, and asking for every fetch trains
people to approve without reading, which makes the prompts that matter worthless. So: no prompt
per fetch, visible step lines instead, and hard limits that make the dangerous cases impossible
rather than asked about.

## Acceptance Criteria
- `web_fetch { url, focus? }` in `fm-agent` (typed tool, read-only effect, so it runs alongside
  other reads): `https` (and `http` upgraded or refused), GET only, no cookies, no credentials,
  no custom headers from the model, redirects limited (5) and re-checked, response bounded
  (e.g. 2 MiB downloaded, 64 KiB of text returned), HTML converted to readable text (title,
  headings, paragraphs, lists, code, links as `[text](url)`), PDFs through the existing
  document conversion, other types refused with a clear error. A long page goes through
  `summarize` with `focus`, like `summarize_file`.
- Never reachable: loopback, private, link-local and other non-public addresses (checked after
  DNS resolution and on every redirect), so a page cannot steer the assistant into the local
  network or a cloud metadata endpoint.
- Exfiltration guard instead of a prompt: query strings and paths are redacted with
  `redact_secrets` before the request, and a URL is refused when it carries text the person never
  saw in the conversation beyond a short length (heuristic: a query value longer than 200
  characters, or one that matches file contents read in this turn).
- Fetched text is marked as untrusted page content in the tool result; the system prompt tells
  the model that instructions inside pages are data, never instructions.
- Consent: one setting, "Let the assistant read web pages" (on by default), plus a per-session
  off switch (`/web off` in the terminal, a toggle in chat). No prompt per fetch. Each fetch is a
  step line ("Read example.com/docs/…"), so the person sees what was read.
- The page content goes to the chat endpoint under its existing consent (a cloud endpoint already
  needs it); nothing extra is sent anywhere.
- Both assistants (chat, terminal) get the tool; browser and Tauri hosts behave the same (the
  fetch runs in the backend).
- Tests: HTML to text, size bounds, redirect to a private address refused, DNS rebinding (public
  name resolving to 127.0.0.1) refused, secret in a query redacted, long query refused, PDF
  through conversion, setting off removes the tool.

## Implementation Notes
- HTML to text: a small converter on `scraper`/`html5ever` if one is already in the tree
  (`dom_query`, `dom_smoothie` are; `dom_smoothie` gives readability-style extraction).
- Resolve once and connect to the checked address (reqwest `resolve`) so the check and the
  connection cannot disagree.
- Skills remain the path for interactive browsing; document that in the skills section of the
  prompt.

## Agent Notes
- 2026-11-25 claude: planned.
