# 0326 Natural-language search

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0322, 0324

## Context
Find dialogs and quick filters need field-by-field input. Typing "invoices from 2024 over 1 MB" or
"photos of the beach taken last summer" should produce a structured filter the person can read and
edit before it runs. The LLM only translates; it never executes the search on its own.

## Acceptance Criteria
- A "Describe what you're looking for" field in the search dialog and an option in the quick filter.
- The chat role (0322) translates the sentence into the existing structured search query (name, type,
  size, date range, metadata, content terms, semantic query). Unknown or unsupported fields are
  reported, not guessed.
- The translated query is shown as editable chips or fields, with the original sentence, before the
  search runs. Running it without edits is one keystroke.
- The model receives the sentence and the list of supported fields only, never file names or
  contents.
- If no LLM role is configured, the field is hidden and nothing is sent.
- Translation failures or empty results fall back to plain search with the original sentence as the
  name term, and say so.
- Browser and Tauri hosts behave the same.
- Tests: query schema validation, unknown-field reporting, no-LLM hiding, fallback path.

## Implementation Notes
- Use a strict output schema (JSON with the query type's fields) validated in Rust before anything
  is shown; reject anything outside it.
- Reuse the existing search query model rather than a parallel one.

## Agent Notes
- 2026-11-12 claude: Created from design discussion. Not started.
