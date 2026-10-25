# Try themes on a real corpus

Corpus themes (task 0284) cluster the indexed chunk embeddings of your library, label each cluster
with keywords, and show a 2D map. They need the real worker and a real embedding model, so use a
debug desktop build with a developer bundle. The mock runtime and `pnpm dev:tauri` (simulated
semantic lifecycle) only show fixture themes.

## 1. Build and start the app

```bash
pnpm install
pnpm dev:tauri:semantic          # builds the developer bundle, then runs the debug Tauri app
# optional: EmbeddingGemma instead of multilingual E5
pnpm dev:tauri:semantic:gemma
```

`dev:tauri:semantic` runs `scripts/build-semantic-developer-bundle.mjs` (worker, native Zvec runtime,
model packs, catalog signed with the public development key; Apple-silicon macOS, x86-64 Windows,
x86-64/arm64 Linux) and starts Tauri with `PROCYON_SEMANTIC_DEVELOPER_BUNDLE=<bundle dir>`.
To reuse a bundle (or another machine's, same OS and architecture) point the variable at it
yourself and run a debug build: `PROCYON_SEMANTIC_DEVELOPER_BUNDLE=target/semantic-developer-bundle/<platform>-<arch> pnpm dev:tauri`.
Release builds reject the variable. The first build downloads model files and takes a while.

## 2. Install components and index a folder

1. **Settings > Semantic**: read the development-only disclosure, install the offered components.
2. Pick a folder holding a few hundred or more documents of mixed topics (text, Markdown, PDF,
   DOCX, ...). JSON Lines feeds (`.jsonl`/`.ndjson`, for example a news or RSS export, one record
   per line) are indexed too: one record becomes one chunk, long files are split into bounded
   windows, and there is no file size limit.
3. Enrol the folder: **Include folder for semantic search** in the Semantic Search dialog (or the
   enrolment controls in Settings), then confirm **OK, index folder**. The debug host runs one bounded indexing pass immediately; watch progress in Settings.
   If it fails, fix the reported cause and repeat **Include folder**.
4. Directory watchers keep the index current when files change; the Themes view flags a stale
   analysis and offers **Analyse again**.

Themes need a real embedding model. With the compact token-hashing fixture the view reports
"embedding model is too basic" instead of themes. Check the active model in Settings > Semantic.

## 3. Open Themes

Command palette > **Explore themes** (listed only when the worker reports knowledge search; debug
builds are exempt from the release qualification gate). Themes open as a pane view in a new tab of the active pane, like Disk usage.

- Scope switch: entire library or the folder shown when you opened the tab.
- **Map**: one point per passage (a stride sample of up to 2000), coloured by theme. Hover shows the
  passage; click opens its source (text and JSONL in the editor at the record line, PDF/slides at
  the page) in the other pane.
- **Theme list**: keywords, size, share. Select one for example passages, strongest documents, and
  SKOS concepts (only shown when the library has concept annotations).
- **Search within this theme** opens a Knowledge Search tab limited to the theme's sources, with
  the theme keywords as the subject.
- Stale banner: the status is polled every 15 s and compared with the analysis index revision.

## What to expect, and limits

- Results are deterministic for the same index, model and configuration, and cached; an unchanged
  index returns instantly. If at most 20% of chunks changed, the previous fit is kept (stable theme
  ids), otherwise it refits.
- Large corpora: the fit uses a stride sample above a size threshold; every other chunk is assigned
  to the nearest centroid. The map is a bounded sample; the response is thinned further if it would
  exceed the 1 MiB IPC message limit.
- Analysis runs under the worker's 5 minute stream deadline. No LLM is involved; labels are
  c-TF-IDF keywords, so they are terse and language-dependent. Quality depends on the embedding
  model; short JSONL records cluster better than long mixed documents.
- Theme-filtered search is **document granularity** (sources contributing to the theme), not
  chunk granularity; a document with one chunk in the theme still qualifies. Entire-library scope
  covers the requesting workspace's tenant only.
- Concept chips are empty unless the 0242 annotation pipeline produced concept ids for the chunks;
  labels are derived from the concept identifier tail.
- `fm-server` serves `/api/v1/semantic/themes/*` only when attached to a desktop profile
  (`--semantic-desktop-profile`, loopback, authenticated); otherwise the routes answer 503. The
  signed production worker predates the theme IPC route and reports the capability unavailable
  until a new semantic component release.
- Not verified here: real-browser rendering of the canvas and a run against a real model; check
  those first when you try it.
