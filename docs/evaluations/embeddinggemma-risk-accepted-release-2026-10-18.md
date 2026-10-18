# EmbeddingGemma 2 risk-accepted release

Procyon v0.6.0 offers EmbeddingGemma 2 as an **opt-in, risk-accepted** semantic
model. This release decision is not a claim that Gemma passed the
five-gate installed-app qualification. The release owner chose to ship and
evaluate the model with users rather than wait for every manual qualification
run. E5 remains available and is still the default.

## Known limitations accepted for this release

- The last recorded Metal scanned-PDF resource run measured 6,434,422,784
  bytes (5.99 GiB) peak process-tree RSS. The documented 4 GiB pass ceiling
  was exceeded; the proposed ceiling was 7.5 GiB. This result remains **Fail**.
- The forced-CPU scanned-PDF resource result has not been recorded.
- CPU fallback behavior and performance have not been qualified across
  supported targets. CPU PDF page-image indexing is capped and may be slow;
  PDF text indexing remains available.
- Windows GPU behavior has not been qualified. Gemma is opt-in in the
  macOS Apple Silicon (arm64), Linux x86-64, and Windows x86-64 desktop
  packages; Intel Macs remain E5-only. This is not evidence that the Windows
  GPU path or CPU performance has passed qualification.
- Browser/desktop citation parity and upgrade/rollback from v0.5.0-2 have not
  been qualified.
- The nine-query local image/audio/video retrieval and consent-isolation gate
  has not been qualified. Model retrieval quality is not established as
  superior to E5.

These are disclosed release risks, not passing results. The qualification
checklist remains unchanged as a record of actual evidence. Do not describe
this release as five-gate qualified.

## User choice and reporting issues

Gemma is optional and creates a separate embedding library; it does not
convert or delete the existing E5 index. Choose Gemma when its multilingual
text and optional media indexing suit your use. Keep a backup of important
data and use E5 if Gemma does not work well on your hardware or workload.

When reporting a problem, include the Procyon version, operating system,
hardware/accelerator, selected embedding dimension and media options, and
whether the worker was using Metal or CPU. Keep private file paths, document
contents and search queries out of public reports.

The release owner’s risk acceptance must be recorded separately from the
five-gate qualification manifest and bound to the exact signed component
candidate used by the release. Missing qualification results must never be
fabricated or converted to `Pass`.
