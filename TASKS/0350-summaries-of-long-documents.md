# 0350 Summaries of long documents

Status: done
Priority: high
Subsystem: backend
Depends on: 0345

## Context

`summarize_file` failed on `12_NAGL_Hackett_RussiaUkraineWar2023.pdf` (6.4 MB, 422 pages) with
"too large to read", so the model fell back to splitting the PDF with python.

- The PDF is well under the 64 MB source budget. In the debug build the docling conversion took
  32 s and hit the 30 s **time** budget; a release build converts it in 2.8 s (1.09 MB of text).
- `documents.rs` reported every `OverBudget` kind as "too large to read", so the model (and the
  person) were told the wrong reason.
- The 30 s deadline suits background indexing. A person who asks for a document waits on it, and
  a large document may need longer, above all in a debug build.
- Even when conversion succeeds, `summarize_file` summarised at most 12 parts (288 KB at the
  default 24 KB part, 1.15 MB at most) and dropped the rest, so a long book was summarised from
  its start only.

## Acceptance Criteria
- A conversion over a budget names the budget: the time it took longer than, the size it is
  larger than, or the other limit by name.
- Documents read by the agent's file tools (`read_file`, `summarize_file`, `@file`) get a deadline
  that grows with the file: 30 s plus 1 s per 100 KB, at most 5 minutes. Indexing keeps its
  default budgets.
- `summarize_file` summarises a long text in parts and combines the part summaries in rounds,
  in groups that fit one call, until one summary is left. There is no fixed cap of 12 parts; the
  text read is bounded by a much higher part count, and the result still says when the rest was
  not read.
- Progress names each part and each combining round.
- Tests: the budget wording, the size-scaled deadline, a summary that needs more than one
  combining round.

## Implementation Notes
- `crates/fm-application/src/agent_host/documents.rs`: `converted_text`, `Documents::new`.
- `crates/fm-application/src/document_conversion.rs`: budgets are set at construction; the
  deadline must be chosen after `inspect` gives the size.
- `crates/fm-agent/src/summarize.rs`: `summarize`, `MAX_SUMMARY_PARTS`.
- No change to the semantic worker closure, so no Gemma re-qualification.

## Agent Notes
- 2026-11-24 claude: diagnosed with a temporary probe test (removed): release 2.8 s converted,
  debug `OverBudget time 30000` after 32.3 s.
- 2026-11-24 claude: done.
  - `document_conversion.rs`: `DocumentConversionService::for_reading` with `Deadline::BySize`;
    `deadline_for_size` = 30 s + 1 s per 100 KB, at most 300 s (the 6.7 MB PDF gets 95 s). Only
    the timeout changes; indexing keeps `ConversionBudgets::default()`. The agent's `Documents`
    (read_file, summarize_file, `@file`, skill documents) use it.
  - `agent_host/documents.rs`: `over_budget` names the limit (time, source size, unpacked size,
    pages/slides/sheets, others by name) instead of "too large to read".
  - `fm-agent/src/summarize.rs`: part summaries are combined in rounds (`group_summaries`, groups
    that fit one call, at least two per group so each round shrinks; a lone leftover is carried
    over). Intermediate prompts and progress name the parts covered ("combining parts 1-2 of 8").
    `MAX_SUMMARY_PARTS` 12 → 48: it now bounds calls and time, not one call's input.
  - Not done: a "Converting…" progress line separate from "Reading …" (the existing report
    already shows before conversion starts).
  - Verified: new tests `a_document_someone_reads_gets_a_deadline_that_grows_with_its_size`,
    `a_conversion_over_a_budget_names_the_budget`,
    `summaries_too_long_for_one_call_are_combined_in_rounds`,
    `a_summary_left_alone_in_a_round_is_carried_to_the_next` pass; 7/7 summarize tests;
    `pnpm run lint` clean; `cargo nextest run --workspace` all passed except
    `ocrmypdf::timeout_terminates_ocr_descendants` (crate untouched; passes alone, timing under
    load). Not checked in the running app.
