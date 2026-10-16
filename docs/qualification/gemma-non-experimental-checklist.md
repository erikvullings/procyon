# EmbeddingGemma 2 non-experimental qualification checklist

This checklist covers the five open items that keep the opt-in EmbeddingGemma 2 package
**experimental** (see
[the experimental release notes](../evaluations/embeddinggemma-experimental-release-2026-10-15.md)
and [task 0248](../../TASKS/0248-gemma-non-experimental-qualification-tooling.md)). Run it on the
installed desktop build that is a candidate for promotion. Every item needs a recorded result in
the [results table](#results) before anyone may remove the experimental label. A single `Fail` or
`Blocked` keeps Gemma experimental.

## Privacy rules for every step

- Run everything on your own machine against your own library. None of the tooling uploads,
  copies, or reads document content.
- `scripts/measure-semantic-worker-resources.mjs` records only process memory, the byte size of
  temporary files the worker holds open, and free disk space. It does not record file names or
  temporary paths.
- `scripts/semantic-quality-compare.mjs` compares the file paths you type into a local JSON file.
  It never opens those files and makes no network requests.
- Keep the generated `samples.csv`, `summary.json`, and quality JSON files local. Before you share
  results, copy only the numbers into the results table. Query wording and file paths can be
  sensitive, so summarize them instead of pasting them.

## Preparation

1. Record the exact desktop version (**About Procyon**) and the Gemma component details
   (**Settings > Semantic > Model and storage details > Exact active model**).
2. Back up the whole Procyon configuration directory before items 1 and 4. Close Procyon first.
   - macOS: `~/Library/Application Support/procyon/`
   - Windows: `%APPDATA%\procyon\`
   - Linux: `${XDG_CONFIG_HOME:-~/.config}/procyon/`

   For example, on macOS run
   `ditto ~/Library/Application\ Support/procyon ~/procyon-backup-$(date +%Y%m%d)`.
3. From a repository checkout, run `pnpm install`. The measurement and comparison scripts need
   only Node 22 or later; neither builds nor runs Rust.

### Where the worker runs and spools

| Item | Location |
| --- | --- |
| Worker runtime directory | `<config>/procyon/semantic/worker-runtime/` |
| Worker process ID | `worker-runtime/worker.pid` |
| Observed backend of the last Gemma worker launch | `worker-runtime/gemma-backend` |
| Installed worker executable | `<config>/procyon/semantic/workers/procyon.semantic.worker/<version>/<artifact>/payload` |
| PDF helper processes | The same `payload` executable, started as children with `--pdf-*-child` arguments |
| PDF ingestion spool | Anonymous `tempfile::tempfile()` in the OS temp directory (`$TMPDIR` on macOS, usually `/var/folders/.../T/`; `/tmp` on Linux; `%TEMP%` on Windows) |
| Staged PDF page images | A second anonymous temp file in the same directory, created by the PDF visual-page child |

On macOS and Linux both spools are unlinked as soon as they are created, so `ls` and
directory-size tools never show them. The measurement script therefore sums the sizes of open
temp-directory descriptors held by the worker process tree, using `lsof` on macOS and `/proc` on
Linux. On Windows they are named delete-on-close `.tmp*` files in `%TEMP%`. The script sums those
best-effort, so other programs' `.tmp*` files can inflate the Windows figure.

Code limits that set the expected ceiling:

- A PDF is spooled only when it is larger than the worker's in-memory stream limit, or when it is
  a consented PDF that is rendered for page images. At most one PDF is spooled at a time.
- `MAX_PDF_INGESTION_BYTES` is 200 MiB, and staged visual pages are capped at 256 MiB
  (`MAX_STAGED_VISUAL_BYTES`).
- The expected worst-case temp-disk use is therefore about 456 MiB, plus the operating system's own
  temp usage.

## Forcing the Gemma worker onto CPU

Setting `PROCYON_GEMMA_FORCE_CPU` to `1`, `true`, `yes`, or `on` (case-insensitive) in the
environment of the process that launches the worker makes Procyon start Gemma workers with
`--gemma-force-cpu`. That process is the desktop app, or `pnpm dev:tauri*` in development. The
worker then skips Metal/GPU detection and runs text, image, audio, and video encoding on the native
CPU backend, applying the CPU page-image budget. Any other value, or no value at all, leaves
automatic selection unchanged. Production workers accept the flag.

The switch applies only when a worker is **launched**. An already-running worker is reused until it
exits. Quit Procyon and confirm that no `payload` process is left (`pgrep -fl payload` on macOS and
Linux) before you change the setting.

macOS apps started from Finder or the Dock do not inherit your shell environment. Use one of these:

- Start the app from Terminal:
  `PROCYON_GEMMA_FORCE_CPU=1 /Applications/Procyon.app/Contents/MacOS/Procyon`
- Or set it for the login session, then start Procyon normally:
  `launchctl setenv PROCYON_GEMMA_FORCE_CPU 1`. Remove it afterwards with
  `launchctl unsetenv PROCYON_GEMMA_FORCE_CPU` and restart Procyon.

On Windows, run `set PROCYON_GEMMA_FORCE_CPU=1` and then start `Procyon.exe` from the same
`cmd.exe`. On Linux, prefix the launch command.

To verify the switch, check all three of these:

1. `worker-runtime/gemma-backend` contains
   `google/embeddinggemma-2 (native CPU) [CPU forced by PROCYON_GEMMA_FORCE_CPU]`.
2. **Settings > Semantic**, in a folder's details, shows **Last worker launch: CPU (forced by
   PROCYON_GEMMA_FORCE_CPU)**.
3. `ps -axo command | grep payload` shows `--gemma-force-cpu` on the worker command line.

## 1. Resource ceilings

Goal: measure the peak resident memory of the worker process tree and the peak temp-disk use of the
PDF spools during large, scanned-PDF indexing, on both Metal and CPU.

1. Prepare a test folder that holds only material you are happy to index. Include at least one
   large scanned PDF (100 MiB or larger; a 155 MiB book-scan PDF is a good worst case), plus a
   handful of ordinary documents. Do not enrol it yet.
2. In a terminal, start the sampler before indexing:

   ```bash
   pnpm semantic:measure:worker --interval 500 --out ~/procyon-measure/metal
   ```

   It re-reads `worker.pid` at every sample, so it waits for the worker to start and follows
   restarts. Use `--pid <pid>` or `--match payload` if the default runtime directory does not
   apply, and `--temp-dir <dir>` if you changed `TMPDIR`. Use `--help` for all options.
3. In Procyon, open the Gemma library and enrol the test folder with page-image indexing enabled.
   Wait until **Settings > Semantic** reports the folder as indexed (or failed or skipped with a
   recorded reason).
4. Wait about 30 seconds, then press `Ctrl+C`. The script writes `samples.csv` and
   `summary.json`.
5. Repeat steps 2–4 with CPU forced (see above) into `~/procyon-measure/cpu`. First remove the
   folder from the library and recreate it, or use a second copy of the folder, so the PDF is
   indexed again.
6. Copy `peakTreeRssBytes`, `peakSingleProcessRssBytes`, `peakOpenTempBytes`, and
   `maximumFreeDiskDropBytes` from each `summary.json` into the results table.

Pass criteria:

- The worker tree never exceeds the documented memory ceiling. Propose the ceiling as the observed
  Metal and CPU peaks plus 25 % headroom, rounded up to the next 0.5 GiB. If the proposed ceiling is
  above 4 GiB, mark the item **Fail** and file an issue.
- `peakOpenTempBytes` is at most 456 MiB (the 200 MiB spool plus 256 MiB staged pages).
- The free-disk drop is explained by that spool plus the index growth.
- Indexing finishes without the worker being killed, and the large PDF ends either indexed or with
  an explicit skip or budget reason.

Notes:

- These are sampled values. A sub-interval spike can be missed, so use `--interval 250` for the
  final run.
- On macOS RSS includes shared framework pages. Treat the numbers as an upper estimate, not as a
  kernel high-water mark.

## 2. CPU fallback on a GPU-capable machine

Goal: on a Metal-capable Mac, confirm that Gemma works correctly when the GPU is not used.

1. Quit Procyon, confirm that no worker is running, and start Procyon with
   `PROCYON_GEMMA_FORCE_CPU=1`.
2. Verify the switch with all three checks above.
3. Run five text searches you have already run with Metal. Note whether the expected file appears
   in the top 5 for each.
4. Index one small image and one short PDF with page images enabled. Confirm that they become
   searchable, and that PDFs with more than four page images report the CPU page-image budget.
5. Quit Procyon, unset the variable, and restart. Confirm that `gemma-backend` reports Metal again
   and that the same searches still work.

Pass criteria:

- Every check passes.
- No crash, hang, or index corruption.
- Search results are usable on CPU: the expected file for each query is still in the top 5.
- Switching back restores Metal without reindexing.

## 3. Cross-host citation navigation

Goal: Search and Ask citations opened from the browser host (fm-server) navigate to the same file
and location as in the desktop app, against the same library.

Precondition: the browser host must be able to reach the desktop's semantic library. At the time of
writing, `fm-server` starts with an unavailable semantic capability and an administrator-provisioned
library that has no roots. Check before investing time:

1. Quit the desktop app. Run `pnpm dev:server` in one terminal and `pnpm dev:http` in another, then
   open the printed URL.
2. Run `curl -s http://127.0.0.1:8787/api/v1/semantic/knowledge/capabilities` and
   `curl -s http://127.0.0.1:8787/api/v1/semantic/library/capabilities`.
3. If they report the capability as unavailable or administrator-provisioned without roots, record
   item 3 as **Blocked: fm-server has no semantic worker attachment** and stop. Promoting Gemma then
   requires either attaching the server to a provisioned worker and library, or an explicit
   release-owner decision that browser hosts are out of scope for Gemma.

If the precondition holds:

4. In the desktop app, run three Search queries and one Ask question whose citations point to
   (a) a text file line, (b) a PDF page, and (c) a PDF page image. Note the target file and location
   for each.
5. Repeat the same queries in the browser host and open each citation.

Pass criteria:

- Each citation opens the same file, with the same page, line, or block label, in both hosts.
- No citation fails to resolve or reveals a different path.
- The browser never shows an absolute path that the desktop hides.

## 4. Upgrade and rollback

Goal: a Gemma library created by v0.5.0-2 survives an upgrade. Rolling back to v0.5.0-2, or
switching to E5, does not corrupt either library.

1. Install v0.5.0-2 and create a small Gemma library (about 20 files, including one PDF with page
   images). Record three queries and their top-3 results. Back up the configuration directory
   (snapshot A).
2. Quit Procyon, install the candidate build over v0.5.0-2, and start it.
3. Confirm the Gemma library is still active, with the same root status and file counts.
4. Confirm the three queries return the same top-3 files without a full reindex. Expect at most
   the components to be refreshed.
5. Add one new file and confirm it is indexed. Back up the configuration directory again
   (snapshot B).
6. Switch to E5 with **Settings > Semantic > Change embedding model**. Run one E5 query, then switch
   back to Gemma.
   - Both libraries must keep their own index; the E5 library is never converted.
   - The Gemma queries must still match step 4.
7. Roll back: quit Procyon, reinstall v0.5.0-2 over the candidate, and start it.
   - Expected: the library opens, or shows an explicit "requires a newer version" or
     rebuild-required state.
   - It must never be a crash, silent data loss, or an index that returns wrong files.
8. If step 7 fails, restore snapshot A and confirm that v0.5.0-2 works again.

Pass criteria:

- Steps 3–6 pass.
- Step 7 produces either a working library or an explicit, recoverable state.
- Restoring a snapshot always recovers.
- No step modifies the E5 index.

## 5. Model quality (Gemma vs E5)

Goal: on your own documents, Gemma's top-k results are at least as good as E5's.

The existing `semantic_production_evaluation` example
(`crates/fm-application/examples/evaluate_semantic_production.rs`) runs a fixed, embedded corpus,
so it cannot score your own files and queries. Use it as a reproducible baseline, and use
`semantic-quality-compare` for this item.

1. Enrol the **same** folder in both the E5 and the Gemma libraries, and wait until both are fully
   indexed.
2. Run `pnpm --silent semantic:quality:compare --template > ~/procyon-quality.json`. Write about 20
   queries in your own words, without copying sentences from the documents. For each query, list
   the files that correctly answer it, as paths relative to the enrolled root.
3. With Gemma active, run each query in Search and record the ranked file paths of the first 10
   results under `results.gemma`. Switch to E5 and record the same under `results.e5`. Repeated
   excerpts from the same file are collapsed automatically.
4. Run `pnpm --silent semantic:quality:compare ~/procyon-quality.json` (add `--json` for
   machine-readable output) and copy the table into the results section.

Pass criteria, all of:

- Gemma MRR ≥ E5 MRR − 0.05.
- Gemma hit@5 ≥ E5 hit@5.
- Gemma misses (no correct file in the top 10) ≤ E5 misses.
- Any query that Gemma loses badly (E5 rank 1–3, Gemma missing) is reviewed and explained in the
  notes.

With about 20 queries, one query changes hit@k by 0.05, so treat small differences as ties. This
is a sanity check on real use, not a statistically significant benchmark.

## Results

| # | Item | Platform / build | Measured values | Result (Pass / Fail / Blocked) | Date | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 1a | Resource ceilings, Metal | | Peak tree RSS:  · Peak single RSS:  · Peak temp:  · Free-disk drop: | | | |
| 1b | Resource ceilings, CPU forced | | Peak tree RSS:  · Peak single RSS:  · Peak temp:  · Free-disk drop: | | | |
| 1c | Proposed memory ceiling | | | | | |
| 2 | CPU fallback | | Backend file: · Queries in top 5: /5 · Page-image budget shown: | | | |
| 3 | Cross-host navigation | | Capabilities: · Citations matched: /4 | | | |
| 4 | Upgrade and rollback | v0.5.0-2 → | Steps 3–6: · Rollback state: | | | |
| 5 | Model quality | | Gemma MRR / hit@5 / misses: · E5 MRR / hit@5 / misses: | | | |

Promotion decision (release owner): ______________________  Date: __________
