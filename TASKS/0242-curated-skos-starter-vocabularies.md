# 0242 Curated SKOS starter vocabularies

Status: done
Priority: medium
Subsystem: frontend, semantic
Depends on: 0187

## Context

Most users do not have a SKOS source ready to import. Provide a small, genuinely sourced and
redistributable set of optional starter vocabularies, rather than silently installing a generic
taxonomy or treating generated suggestions as authoritative. This is a separate follow-up, not
a prerequisite for the EmbeddingGemma experimental release.

## Acceptance Criteria

- Settings offers a useful opt-in choice of version-pinned, attributed starter SKOS vocabularies
  backed by verifiable redistribution terms. Do not invent concepts or claim a partial extract
  is the full source.
- A chosen starter imports through the existing vocabulary API and remains unattached until
  the user explicitly attaches it to a workspace or root. Existing vocabularies remain reusable.
- Errors and duplicate imports are clear; import and attachment behave in mock, HTTP, and Tauri
  hosts. Source identity and license are documented and visible.
- Tests cover importing a starter, leaving it unattached, and handling a failed import.

## Implementation Notes

- `TASKS/0187-skos-concepts-and-virtual-folders.md` owns the existing vocabulary contract.
- Existing locally extracted candidate terms require review **inside an existing vocabulary**;
  they do not automatically create a complete SKOS source.
- No Gemma model selection or release-gate changes.

## Agent Notes

- 2026-10-11: Started separate follow-up. Confirmed official GEMET 4.2.3 RDF SKOS export
  (`https://www.eionet.europa.eu/gemet/en/exports/rdf/4.2.3`) and STW 9.20 SKOS export
  (`https://zbw.eu/stw/version/9.20/download/about.en.html`) each explicitly state CC BY 4.0.
  Will ship only excerpts traceable to official concept URIs and preserve attribution.
- 2026-10-11: Bundled four English excerpts (17 GEMET climate/energy, 53 STW technology, 46
  STW psychology, 15 STW defence economics) under
  `frontend/src/features/settings/skos-starters/`. `scripts/generate-skos-starters.py` verifies
  official pinned download hashes, source labels and in-excerpt hierarchy; Settings imports them
  through the existing client with source/license disclosure but never auto-attaches them. See
  `docs/semantic-operations.md` for the exact selection rule and attribution. The backend
  SKOS importer validates each asset; frontend tests cover opt-in import, reuse/duplicate, and
  failure. Workspace lint, full Rust suite (with bounded concurrency), frontend/scripts suites,
  doctests and frontend build passed. The first default-concurrency full Rust run hit two unrelated
  OCR descendant timing assertions; both passed isolated and in the bounded full rerun.
- 2026-10-11: User correctly challenged STW's economics-centric scope for technology,
  psychology and defence. Replaced all STW excerpts with more appropriate UNESCO Thesaurus
  subject collections and a narrowly named defence selection, and added natural sciences.
  UNESCO's official RDF snapshot (scheme modified 2026-08-31) is hash-pinned; its `/latest/`
  URL is mutable, so regeneration intentionally fails on upstream changes. Adapted UNESCO
  JSON assets remain under CC BY-SA 3.0 IGO with attribution and adaptation disclosure.
- 2026-10-11: Checked the linked `erikvullings/rag` session's worktree and its task 0103:
  that project derives a project-specific SKOS-like vocabulary from indexed chunks with TF-IDF,
  synonym clustering and optional hierarchy. It contains no bundled `.ttl`, `.rdf` or `.skos`
  starter dataset, nor a GitHub library that supplies a generic SKOS source. Its older task 0026
  lists hand-curated extraction **predicates**, not SKOS concepts. Do not present those as
  third-party reusable vocabularies. Further popular sources (EuroVoc, AGROVOC, LCSH) need
  independently verifiable versioned source bytes and redistribution terms before bundling.
- 2026-10-11: Revalidated the updated five-choice UI and all five JSON assets: pinned
  upstream-data check, Rust SKOS import test, workspace lint, frontend build, 2,360 frontend
  tests, 117 script tests and a serial full Rust run (2,818 pass). Initial -j 2 Rust run
  had one unrelated intermittent OCR child-process timing assertion; isolated and serial
  reruns passed.
