# 0212 Knowledge search toolbar and PDF sections

Status: done
Priority: high
Subsystem: frontend, kg
Depends on: 0211

## Context

Live testing of the Search Knowledge pane shows that the query composer still occupies too much
vertical space, scrolls away with the results, and exposes settings that should be secondary. The
results' native list markers are not reliably visible, document boundaries remain too subtle, and
PDF evidence navigation can close an already-open source because page extraction only understands
direct provenance instead of the chunker's `exact` and `span` wrappers.

PDF conversion already uses Docling's geometry-aware text-layer extraction, which identifies
headings and excludes `PageFurniture` nodes. The structural chunker currently packs units by
top-level boundary only and takes the first unit's parent path. A chunk beginning with a heading can
therefore lose that heading from `section_path` and absorb later sections on the same page.

## Acceptance Criteria

- Replace the labelled multi-row subject composer with a filter-style, single-row toolbar matching
  the pane row height: search icon, query field, Search action, then a cog settings action.
- Put knowledge needs and all advanced search controls in a settings modal; do not render either
  group below the query field.
- Persist selected knowledge needs across searches and application restarts, while validating
  stored values and retaining safe defaults if browser storage is unavailable or malformed.
- Keep the query toolbar fixed while only the result/document region scrolls.
- Render an explicit visible number and strong boundary for every relevance-ranked document.
- Show PDF evidence navigation as a clear `Page #` link and make wrapped `exact`/`span` provenance
  navigate an already-open preview instead of closing it.
- Make structural chunks respect heading paths: a heading starts a new chunk, its own text becomes
  part of that chunk's section path, and sibling sections on one page are not merged.
- Retain Docling's page-furniture exclusion and document the deterministic fallback limitation.
- Add focused frontend and Rust regressions for persistence, modal/toolbars, result scrolling and
  numbering, wrapped page navigation, and heading-aware chunk boundaries.

## Implementation Notes

- Keep settings local to the Search Knowledge feature; they are UI preferences, not semantic
  library policy. Use the existing versioned local-storage precedent and never fail the pane when
  storage is unavailable.
- Use `ModalPanel`, `IconButton`, Tabler icons, and existing pane/filter sizing tokens.
- Preserve the existing canonical query draft, parser, scope, retrieval-mode, trace, and optional
  answer contracts.
- Bump the structural chunker version when its output changes so retained indexes rebuild instead
  of mixing old and new section semantics.

## Agent Notes

- 2026-09-09 Copilot: Confirmed the close-on-section bug is caused by
  `knowledgeEvidencePage` ignoring `ChunkProvenance::Exact` and `ChunkProvenance::Span`; the viewer
  interprets the resulting undefined page as a request to toggle the already-open document closed.
  Confirmed deterministic Docling conversion already maps heading nodes and removes page furniture;
  the loss occurs in the chunker because it groups by page and copies the first unit's parent path.
- 2026-09-09 Copilot: Replaced the composer with a fixed filter-style toolbar and moved knowledge
  needs plus advanced controls into a cog-opened settings modal. Knowledge needs now persist in
  validated, versioned local storage and safely fall back to no selected needs when storage is
  malformed or unavailable.
- 2026-09-09 Copilot: Made the results body the pane's sole scroll owner, added explicit document
  numbers and boundaries, and separated section headings from underlined provenance links. PDF
  evidence now exposes `Page #` links, while recursive `exact`/`span` page extraction preserves an
  already-open preview and navigates it to the requested page.
- 2026-09-09 Copilot: Made structural chunks stop at section-path changes, promoted a heading into
  its own chunk section path, and bumped the active chunker identity to `structural/3` so retained
  indexes rebuild. Documented that deterministic Docling excludes page furniture, while baseline
  fallback cannot synthesize missing heading hierarchy.
- 2026-09-09 Copilot: Review hardening made repeated same-name headings start distinct chunks,
  aligned the production-bundle script with `structural/3`, kept Escape in the settings DSL scoped
  to that modal, and restored a visible keyboard-focus outline to the query field.
- 2026-09-09 Copilot: Verified 117 Search Knowledge/theme tests, the full 158-test app-shell suite,
  238 semantic conversion/component/Docling tests, 4 production-bundle script tests, frontend
  typechecking, repository-wide Rust and frontend lint, the Impeccable layout scan, and a rendered
  two-pane mock session at the compact 20px density. The pane root had no scroll, the document body
  scrolled without horizontal overflow, and the toolbar position remained fixed.
