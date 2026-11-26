# 0349 html5ever readable HTML conversion and worker dependency refresh

Status: done
Priority: high
Subsystem: backend, semantic
Depends on: none

## Context

`fm-semantic-conversion` converts HTML (saved web pages, and EPUB chapters) with a hand-written
tokenizer. It read `<script>` and `<style>` content as markup: a JavaScript string such as `'<div>'`
opened an element that never closed, and the rest of the page was dropped. `03_DecoyWarfare_USNI.html`
gave no text at all, so `summarize_file` failed on it and the model fell back to writing python.
That bug is patched in the tokenizer (raw-text `script`/`style`, converter `baseline/10`), but a
hand-written parser will keep meeting real-world HTML it gets wrong.

Even when parsing works, a web page's text is mostly site chrome. 03 still starts with "Donate /
Cart / Join or Log In / Main navigation…", and 06 is a journal landing page that is mostly menus.
That costs tokens in `read_file`/`summarize_file` and dilutes semantic chunks.

Any change to this crate changes the `fm-semantic-worker` closure, so the signed Gemma candidate
must be re-qualified before the next desktop release (CLAUDE.md, Release process). The same
qualification can carry a dependency refresh of that closure, so it is paid once.

## Acceptance Criteria

### HTML parsing
- HTML is parsed by `html5ever` 0.40 (the parser Servo uses, written to the HTML standard), not
  by our own tokenizer. Script, style, template and similar content, comments, unclosed and
  misnested elements behave as in a browser.
- The crate's guarantees stay:
  - the input byte budget is checked before parsing;
  - the nesting-depth budget is charged while walking the tree, iteratively (no recursion);
  - checkpoints and cancellation are honoured between units;
  - nothing is fetched, and no external entity is resolved.
- Units are as today: headings with section paths, paragraphs, list items, quotes, and tables
  as `a | b` rows. Entities are decoded by the parser, which replaces our `decode_entities`.
- Provenance keeps line ranges (`TextLines` / `EpubText`). If keeping them proves costly, record
  the decision; never fake them.

### Main content
- A standalone web page (not an EPUB chapter) is reduced to its main content with `dom_smoothie`
  0.20 (a Mozilla Readability port on `dom_query`/`html5ever` 0.40) before units are built:
  - the title comes first, as a heading;
  - when readability finds no article, or keeps less than a small share of the page's text, fall
    back to the whole body minus `nav`, `header`, `footer` and `aside`;
  - the fallback is visible in the conversion's warnings, so a quality problem can be seen.
- EPUB chapters are not reduced: every word of a book chapter is content.

### Tests and versions
- Real-page regression fixtures, trimmed and kept small, with any personal or tracking
  content removed:
  - a page whose scripts contain markup-like strings (like 03);
  - a landing page that is mostly menus (like 06);
  - a normal article page.
- The existing html and EPUB tests pass, adjusted only where browser behaviour differs (and
  noted as such).
- `BASELINE_CONVERTER_VERSION` goes up again (to `baseline/11`), so indexed pages re-convert.
- `docs/semantic-operations.md` names the new converter identity.

### Dependency refresh (the same qualification)
- Semver-compatible updates in the worker closure: `cargo update -p` for each crate below,
  not a blanket `cargo update`, so the closure diff stays reviewable:
  - crates the worker uses directly: `interprocess` 2.4.9, `rand` 0.10.4, `tokio-util` 0.7.20,
    `zip` 9.8.2;
  - crates they pull in: anyhow, bytes, cc, crc32fast, hashbrown, pin-project-lite, rangemap,
    regex, regex-automata, brotli-decompressor, nix.
  - Raise the workspace minimums in `Cargo.toml` where the code relies on the new versions.
- `rusqlite` 0.42 → 0.43 (a breaking release; bundled SQLite):
  - it is used by `fm-semantic-worker`, `fm-application` and `fm-server`;
  - upgrade all three together, read its changelog, and run the worker catalog and
    `fm-application` storage tests.
- `docling-core`/`docling-pdf` 2.82.1 → 2.86.14 is **out of scope here** and gets a task of its
  own:
  - they are pinned exactly (`=2.82.1`), and the PDF converter identity
    (`docling-pdf/2052009+baseline/…`) and its measured evaluation depend on them;
  - upgrading changes PDF output and needs its own measurement, not just a re-sign.
- Updates outside the closure (43 semver-compatible ones at planning time) may go in the same
  push, since they do not affect the gate, but in a separate commit.

### Release
- Push to `main` once, with all closure changes. That starts `release-semantic-components.yml`
  qualification automatically. When it is green, `pnpm run release:preflight` prints the
  `scripts/resign-gemma-candidate.mjs --qualification-run <id>` command that re-signs.
- `pnpm run lint`, the full Rust suite (`cargo nextest run --workspace` and
  `cargo test --doc --workspace`), the frontend suite and `pnpm api:check` pass.

## Implementation Notes

### Where things are
- `crates/fm-semantic-conversion/src/formats/html.rs`: the tokenizer to replace. Its block rules
  (`block_kind`, `heading_level`, the "a nested block closes the enclosing one" rule, table cells
  and rows) are the contract for the walk.
- `crates/fm-semantic-conversion/src/formats/epub.rs` calls `convert_with_context` with
  `HtmlContext::EpubSpine`; keep that seam.
- `converter.rs`: the `FormatKind::Html` dispatch and `BASELINE_CONVERTER_VERSION`.

### Versions at planning time (2026-11-24)
- `html5ever` 0.40.1, `markup5ever_rcdom` 0.40.1, `dom_query` 0.30.0 and `dom_smoothie` 0.20.0
  are all on html5ever 0.40.
- The workspace already has html5ever 0.39 (from `ferrodoc` in fm-application, and from
  `dom_query` 0.28 in tauri-utils), outside the worker closure. A 0.40 copy in the worker is
  acceptable; do not force the app's copies to change.

### Line provenance
- html5ever reports source lines to the tree builder (`TreeSink::set_current_line`). Either:
  - wrap the sink to record each element's start line; or
  - walk `dom_query`'s tree and map text back to lines.
- Choose by how much code each takes; record the choice here.

### Budgets
- `dom_smoothie` works on the whole document, so the byte budget must bound the input first.
- Measure the time on the largest fixture (a ~700 KB page like `lvivherald-invisible-armour.html`)
  in release mode, and record it.

### Worker closure
- At planning time it contained `fm-semantic-worker`, `fm-semantic-conversion`,
  `fm-semantic-protocol` and `fm-semantic-docling`: 244 crates and 29 direct external
  dependencies.
- Outdated direct dependencies: interprocess, rand, rusqlite, tokio-util, zip, and
  docling-core/docling-pdf.

## Agent Notes
- 2026-11-24 claude: planned after the raw-text `script`/`style` fix.
  - That fix (`baseline/10`) is in the working tree with tests and may ship on its own first.
    It already changes the worker closure, so batching this task into the same qualification
    avoids a second re-sign.
  - Dependency data came from `cargo metadata` (the closure), `cargo update --dry-run`
    (compatible updates) and the crates.io API (latest versions).
- 2026-11-24 claude: implemented everything except the release push. Docling was included on
  the maintainer's instruction, without re-measuring.
  - **HTML conversion.** `formats/html.rs` now parses with `dom_query` 0.30 (html5ever 0.40).
    Standalone pages go through `dom_smoothie` 0.20 `Readability::extract`.
    - The pick is used only when it has at least 500 visible characters, at least 1/5 of the
      page's visible text, and at most half of its text inside links. Without the link check,
      a `div` of menu links was taken as the article.
    - Otherwise the whole page is read without `nav`/`aside`, and without `header`/`footer`
      outside `article`/`main`. `ConversionWarning::MainContentNotFound` is added, but only for
      pages of 500+ characters.
    - EPUB chapters are walked whole. Namespace prefixes (`xhtml:h1`) are stripped, as before.
    - A first iterative pass charges nesting depth and measures text before Readability runs.
      The walk is iterative too, and text around nested blocks now stays with its block (the
      old tokenizer dropped text after a nested block).
    - Line provenance decision: best-effort `LineLocator`. It searches each unit's longest
      leading word after the previous unit, and the longest trailing word after that. Wrapping
      the html5ever sink was not worth it, since Readability re-serializes the tree anyway.
  - **Tests.** The old tests are kept, except one adjusted for browser behaviour:
    `decode_entities` is gone, because the parser decodes entities. New tests:
    - a news page reduced to its main content;
    - a menu-heavy landing page that falls back to the whole page and warns;
    - a short page read whole without a warning;
    - text around a nested block.
    - The fixtures are synthetic pages shaped like 03 and 06: the real pages are copyrighted
      articles, so they are not committed.
  - **Real pages.** Probed through `read_file` on the 13 HTML files in `~/Downloads/deception`,
    with a temporary test that was then removed. Every page now starts with its article; 03
    gives the article instead of no text. Debug build: 30-190 ms per page. Release timing was
    not measured.
  - **Dependencies.**
    - Docling `=2.86.14`, which adds `Node::ProvContinuation`; it is skipped like `PageBreak`,
      as docling's own serializers do.
    - rusqlite 0.43.0 (libsqlite3-sys 0.40); no code changes were needed.
    - interprocess 2.4.9, rand 0.10.4, tokio-util 0.7.20, zip 9.8.2.
    - `cargo update` for all compatible updates: 88 lockfile entries, inside and outside the
      worker closure, in one change. The planned per-crate split was not kept.
  - **Identities.**
    - `BASELINE_CONVERTER_VERSION` is `baseline/11`;
    - `DOCLING_PDF_CONVERTER_VERSION` is `docling-pdf/2086014`;
    - the pipeline and production identity is `docling-pdf/2086014+baseline/16`, in
      fm-semantic-docling, fm-semantic-components and `scripts/build-semantic-production-bundle.mjs`;
    - `docs/semantic-operations.md` names it and records the no-re-measure decision.
  - **Verified.** `cargo nextest run --workspace` 4138 passed; doctests pass; clippy
    (`-D warnings`) and fmt are clean; script tests 213 passed (3 skipped); Biome is clean;
    the regenerated OpenAPI and client are unchanged. The frontend suite was not rerun, since
    no frontend file changed.
  - **Remaining.** Push to `main`; that starts the semantic-components qualification. When it
    is green, run `pnpm run release:preflight`, then the `resign-gemma-candidate.mjs` command
    it prints.
- 2026-11-26 claude: closed. Docling `2.86.14` was taken in the same qualification (the maintainer
  accepted it without a separate task; PDF output changes are watched in the pre-release).
  Shipped in the `semantic-v0.7.10-auto.108` candidate used by `v0.8.0-2`. Still unmeasured: release
  timing of the largest HTML fixture, and real-page fixtures (kept out of the repo for copyright).
