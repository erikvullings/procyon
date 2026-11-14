# Draft upstream PR: `encode_rows_at_widths` for EmbeddingGemma 2

Target: `ohdearquant/lattice`, crate `lattice-model-embeddinggemma2`. Patch: [lattice-encode-rows.patch](lattice-encode-rows.patch)
(one commit on top of `583e2c1c`). Not posted; review before opening.

## Title

feat(embeddinggemma2): encode ids with externally computed rows

## Summary

Adds `EmbeddingGemma2Model::encode_rows_at_widths` and, under `metal-gpu` on macOS,
`encode_rows_at_widths_metal`. They take token ids plus `(position, row)` pairs whose rows were
computed outside the model, and run them through the same host or GPU-resident forward pass as text
and image parts.

Semantics match what `encode_parts_at_widths` already does for image rows: the id at each listed
position is read as padding when gathering hard-token embeddings and the per-layer signal, and the
row replaces it unscaled. Rows must be strictly increasing by position, inside `ids`, `hidden_size`
long and finite. The token limit is not applied.

## Why

The checkpoint also carries audio and video towers. Downstream code can compute their projected soft
tokens (Procyon does, for audio and video) but cannot reuse the text encoder: `forward` and
`combine_rows` are `pub(crate)`, and only the image placeholder id is special-cased. Callers
currently reimplement the whole encoder; on an M4 Max Procyon's own CPU-plus-GEMM-offload encoder
took 36 s for 13 documents where this crate's Metal path takes 0.70 s.

## Tests

- `external_rows_equal_the_scaled_token_rows_they_replace`: replacing positions with the scaled
  rows of the original tokens, under different placeholder ids, reproduces `encode_ids_at_widths`.
- `external_rows_reject_bad_positions_and_shapes`: out-of-range and unordered positions, short and
  non-finite rows.

Both pass on 1.98.1. The Metal entry point compiles (`cargo check --features metal-gpu
--all-targets`); it has no GPU test yet and has not been run against audio or video rows from the
real checkpoint.

## Open items before posting

- Add a Metal parity test against the host path (follow `model/tests/metal_parity.rs`).
- The repo's `CLAUDE.md` requires `make bench-compare` output for PRs touching inference crates;
  decide whether `crates/models/*` falls under it. This change adds API only and does not alter an
  existing code path, but say so explicitly.
- `cargo clippy -D warnings` on 1.98.1 fails in `lattice-grammar` (`schema_compilation.rs:931`),
  unrelated to this change; the crate itself is clean. Lattice pins 1.94.1, which cannot build proc
  macros on macOS 27, so the workspace lint was not run under the pinned toolchain.
