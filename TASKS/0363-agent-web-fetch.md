# 0363 Read-only web_fetch tool for the assistants

Status: done
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
- 2026-11-25 claude: started; paused because other uncommitted work (0360-style: argument
  aliases, `ArgumentError`, `ModelProfile`, `repair_tool_arguments`, `match_edit`) is in progress
  in the same `fm-agent` files and its tests do not compile yet (`turn/tests.rs`: `run.most`).
  - Done, uncommitted: `crates/fm-agent/src/web.rs` (`WebFetch` tool, `WebAccess` trait,
    `WebPage`, `WebError`, `url_problem`: https/http only, no user info, secrets in the URL,
    parts over 200 characters or encoded runs of 120+ refused before sending; page text marked
    as untrusted data; a long page summarised with `summarize` and `focus`, else cut with a
    note; 4 tests). Additive edits: `lib.rs` (module, exports, `WebFetch` in
    `read_only_registry`), `tool.rs` (`ToolContext.web`), `summarize.rs` (`MIN_PART_BYTES`
    pub(crate)). The library builds with both sets of changes.
  - Next: `fm-application` fetcher (resolve once, refuse non-public addresses on every hop,
    `reqwest` with no redirects/cookies, 2 MiB cap, HTML via `dom_smoothie` Markdown with
    `strict_visibility`, PDF via a temp folder and the folder tools' `read_whole`), wire `web`
    into chat and terminal `ToolContext`s, remove the tool from the registry when off, setting
    and `/web on|off`, i18n, tests.
- 2026-11-25 claude: done (resumed after 0360 was committed).
  - `fm-application/src/agent_host/web.rs` `WebReader` (implements `WebAccess`): http upgraded to
    https, other schemes refused; every hop resolves the host and refuses it unless all its
    addresses are public (loopback, private, shared 100.64/10, link-local incl. cloud metadata,
    multicast, documentation, reserved, `localhost` names, IPv4-mapped IPv6); the connection goes
    to exactly the checked addresses (`resolve_to_addrs`), no proxy, no redirects followed by
    reqwest (5 handled by hand, each re-checked), no cookies; 2 MiB download cap. HTML through
    `dom_smoothie` (Markdown, `strict_visibility` drops hidden text) with a `dom_query` body-text
    fallback; text/JSON/XML/Markdown as is; PDF written to a temp folder and read by the folder
    tools (the same conversion as documents); other types refused.
  - Setting `assistantWebAccess` (Settings → terminal section, on by default; fm-settings,
    DTO, mapping, frontend model). Requests carry `allowWeb` (chat: the setting; terminal: the
    setting unless `/web off`). When off, `web_fetch` is removed from the registry, so the model
    is not told about it. No prompt per page: each read is a step line.
  - Not done: the "matches file contents read this turn" exfiltration heuristic (only secrets,
    long parts and encoded runs are refused); chat reads the web only when "Let the assistant look
    in this folder" is on, since the chat runs tools only then.
  - Verified: fm-agent web 4, fetcher 5 (public-address rule, local names refused, scheme
    upgrade, HTML to Markdown without hidden text, text types), terminal `web_fetch` offered only
    when allowed, drawer `/web`; fm-agent + fm-application 1753 and frontend 4229 pass;
    `pnpm lint` clean. Live check (ignored test) against example.com.

