# EmbeddingGemma 2 non-experimental qualification checklist

This checklist covers the five open items that keep the opt-in EmbeddingGemma 2 package
**experimental** (see
[the experimental release notes](../evaluations/embeddinggemma-experimental-release-2026-10-16.md)
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
- Item 5 uses only the installed app and a deliberately chosen local test folder; do not use
  private files you do not want to index. Keep query wording, result lists, and file paths local.
- Keep the generated `samples.csv` and `summary.json` local. Before you share results, copy only
  aggregate counts and observations into the results table. Query wording and file paths can be
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

Precondition: the browser host reaches the desktop's library through the explicit, read-only
desktop attachment (task 0250). By default `fm-server` attaches nothing. The attachment joins the
desktop's already-running worker as the same OS user; it never launches, stops, or changes that
worker or library, and it requires a loopback bind and token authentication.

1. Start the desktop app with Gemma active and the qualification corpus enrolled, and leave it
   running. Its profile directory is `~/Library/Application Support/procyon` on macOS,
   `%APPDATA%\procyon` on Windows, and `~/.config/procyon` on Linux.
2. In one terminal, start the server **without** `--dev-mode-auth-disabled` (do not use
   `pnpm dev:server`), passing every enrolled folder, or a parent of all of them, as a root:

   ```bash
   cargo run -p fm-server -- \
     --semantic-desktop-profile "$HOME/Library/Application Support/procyon" \
     --root "$HOME/qualification-corpus"
   ```

   Note the printed access token. In another terminal run `pnpm dev:http`, open the printed URL,
   and sign in with the token.
3. Run, with `TOKEN` set to that token:

   ```bash
   curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/v1/semantic/library/capabilities
   curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8787/api/v1/semantic/knowledge/capabilities
   ```

   The library must report `desktopManaged` with only `viewStatus` and `viewFolderStatus`. If it
   reports `unavailable`, check that the desktop app is running with an active model and consented
   library, and that every enrolled folder lies inside a `--root`; the attachment fails closed
   otherwise. A release build reports knowledge search as available only when it was compiled with
   the knowledge-search release qualification flag.

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
   images). Record three queries and their top-3 results as the pre-upgrade baseline. Back up the
   configuration directory (snapshot A).
2. Quit Procyon, install the candidate build over v0.5.0-2, and start it.
3. Confirm the Gemma library remains active with the same roots and consent. The candidate changes
   the worker's converter/chunker/preprocessing pipeline, so it must explicitly discard and rebuild
   the Gemma-derived index from enrolled source files. Observe the reindex status through completion;
   do not expect the old Gemma index or its pre-upgrade top-3 results to survive unchanged. The E5
   index must remain intact.
4. After Gemma reindexing completes, repeat the three queries and record their top-3 results and
   citations against the pre-upgrade baseline. Confirm results resolve to eligible enrolled files.
   Record all ranking/content differences for release-owner review; do not mark exact top-3 parity as
   passed or treat this required pipeline reindex as a same-pipeline, no-reindex upgrade.
5. Add one new file and confirm it is indexed. Back up the configuration directory again
   (snapshot B).
6. Switch to E5 with **Settings > Semantic > Change embedding model**. Run one E5 query, then switch
   back to Gemma.
   - Both libraries must keep their own index; the E5 library is never converted or rebuilt.
   - Gemma must remain searchable without a second full reindex; record any result change caused by
     the new file separately from the post-upgrade baseline in step 4.
7. Roll back: quit Procyon, reinstall v0.5.0-2 over the candidate, and start it.
   - Expected: the library opens, or shows an explicit, recoverable rebuild-required state. The old
     worker may need to rebuild the Gemma index for its pipeline; record any reindex status.
   - It must never be a crash, silent data loss, or an index that returns wrong files.
8. If step 7 fails, restore snapshot A and confirm that v0.5.0-2 works again.

Pass criteria:

- Steps 3–6 complete, including the required one-time Gemma pipeline reindex; results and differences
  are recorded for release-owner review, not silently called top-3 parity.
- Step 7 produces either a working library or an explicit, recoverable state, including any
  required rollback reindex.
- Restoring snapshot A recovers the original Gemma index, E5 index, and consent state.
- No step modifies the E5 index.

## 5. On-device Gemma media search

Goal: qualify image, audio, and video retrieval against a small, **locally chosen** test set on
the installed candidate, including source attribution and honest coverage. Published text
benchmarks (see the [model card](https://huggingface.co/google/embeddinggemma-2) and
[existing labelled evaluation](../evaluations/embeddinggemma-judged-2026-10-10.md)) provide
context, not a local Gemma-vs-E5 promotion threshold. Do not run the `semantic:quality:compare`
script or require an E5 library for this item.

1. Make a separate, backed-up test folder with **at least three supported files per modality**:
   JPEG/PNG/WebP images, WAV/MP3/FLAC/AAC/M4A audio, and MP4/MOV video. Use files you own and
   are willing to index, with recognizable, different subjects, sounds, and scenes. Add at least
   two same-modality distractors for each target (the other two targets suffice). Avoid filenames
   that contain the query words. Keep an excluded or unenrolled folder with one distinctive
   media file for the privacy check. Do not record paths or queries in the shared results table.
2. In **Settings > Semantic**, keep the existing Gemma library and dimension. Under **Gemma media
   indexing**, enable **Include images**, **Include audio**, and **Include video (uses the vision
   encoder)**, then **Save media choices**. An existing library that started with images enabled
   and audio/video disabled does **not** need to be recreated. Include the test folder for
   semantic search; leave the privacy-check folder unenrolled or explicitly excluded. Wait for
   the next library scan and for the included folder to finish indexing. If a medium is skipped,
   fails, or the worker is unavailable, record the reason instead of counting it as a search miss.
3. Prepare **three independent text queries per modality** (nine total), each describing one
   known target without naming its file: visible subject/composition for images, audible content
   for audio, and a scene/action sampled in a video. Avoid exact title matches and reuse neither
   the query text nor a path in the shared table. With Gemma active, open the **Semantic Search**
   dialog from the toolbar button (tooltip **Semantic search & Ask** or **Semantic Search…**),
   enter the query under **What are you looking for?**, scope it to the included folder, and press
   **Search**. For each query, record locally whether the expected file appears among the **first
   five distinct files** in the result list, its rank, and whether the result's **Open image**,
   **Open audio**, or **Open video** button opens that same source. Do not treat a filename or
   excerpt-only lexical hit as proof of media retrieval; check the result's media label.
4. Inspect one successful result of each modality: **Image**, **Audio**, **Video** labels must
   match the source type and open the correct file. File-level image and audio evidence need
   not claim a particular region or second. Video evidence may describe a number of sampled
   frames and **partial temporal coverage**; it must not claim an exact matching time or frame.
   Current Search UI opens the source file, not a timestamped video segment. If a preview cannot
   be shown, the source must still open and the UI must not present an invented preview.
5. Search for the distinctive content of the unenrolled/excluded file in the same scope. Inspect
   the first five results and confirm that neither this source nor a path or preview from it is
   shown. If it appears, mark **Fail** regardless of retrieval counts. Do not disable an existing
   medium solely for this check: removing consent deletes its indexed vectors.

Pass criteria (all required):

- Image **3/3**, audio **3/3**, and video **3/3** expected targets in the top five distinct files,
  with the matching media label and the correct file opened for every hit. Record counts by
  modality, plus any failures or skips, not source names.
- The sampled evidence observations in step 4 are accurate: video coverage is explicitly partial
  when reported, and no result implies an exact timestamp/region that the app cannot resolve.
- The unenrolled/excluded source is absent; no content, path, or preview escapes its scope.
- Any failed indexing, crash, unusable source link, mislabeled evidence, or unauthorized result is
  **Fail**. Mark **Blocked** only when the installed candidate cannot perform the run (for
  example, the Gemma worker is unavailable); record the reason. Do not silently omit a modality
  or count an unindexed file as a pass.

## Results

| # | Item | Platform / build | Measured values | Result (Pass / Fail / Blocked) | Date | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 1a | Resource ceilings, Metal | v0.5.0-2 · macOS ARM64 · Gemma FP32 Metal | First 20 min: peak tree RSS 6,434,422,784 B (5.99 GiB; 12 processes at peak, 13 max) · peak single RSS 2,760,671,232 B (2.57 GiB) · peak temp 79,048,531 B (75.4 MiB) · free-disk drop 26,345,328,640 B (inconclusive). Continuation 20 min: peak RSS 2,995,748,864 B (1 process) · peak temp 0 B. | Fail | 2026-10-16 | Provisional installed run while reindexing roots, including a 166 MiB / 354-page scanned PDF. Indexing was still active at 40 min. Concurrent local builds confound the free-disk drop; do not attribute it to worker/index. CPU not measured. |
| 1b | Resource ceilings, CPU forced | | Peak tree RSS:  · Peak single RSS:  · Peak temp:  · Free-disk drop: | | | |
| 1c | Proposed memory ceiling | macOS ARM64 · Gemma FP32 Metal | 7.5 GiB (peak tree RSS × 1.25, rounded up to next 0.5 GiB; observed peak: 6,434,422,784 B / 5.99 GiB) | Fail | 2026-10-16 | Exceeds the 4 GiB pass ceiling; helper RSS investigation issue filed. This blocks promotion. |
| 2 | CPU fallback | | Backend file: · Queries in top 5: /5 · Page-image budget shown: | | | |
| 3 | Cross-host navigation | | Capabilities: · Citations matched: /4 | | | |
| 4 | Upgrade and rollback | v0.5.0-2 → | Steps 3–6: · Rollback state: | | | |
| 5 | On-device Gemma media search | | Image top-5 /3: · Audio top-5 /3: · Video top-5 /3: · Correct media/source: /9 · Excluded source absent: · Coverage/provenance accurate: | | | |

Promotion decision (release owner): ______________________  Date: __________
