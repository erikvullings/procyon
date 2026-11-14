# EmbeddingGemma 2: upstream Lattice text/image path (2026-11-14)

Lattice `main` at `583e2c1c86d1722877e21ff384b7979b1a308588` now ships
EmbeddingGemma 2 text and image encoding as the `lattice-model-embeddinggemma2`
crate, with a GPU-resident Metal forward pass. Procyon's worker now routes **text and still
images** through it; **audio and video** keep the local fusion encoder and towers because upstream
has no audio tower and no entry point for externally computed token rows.

## Measurements (Apple M4 Max, release, warm, 768 dimensions)

13 documents of 8-1200 words, and one synthetic 128 x 96 image. Previous implementation is
the pinned Lattice `17915f37` plus Procyon's own encoder, FP32 GEMM offload only.

| | Previous | Upstream | Speedup |
| --- | ---: | ---: | ---: |
| Text, Metal, 13 docs | 36.0 s | 0.70 s | ~52x |
| Text, CPU, 13 docs | 36.3 s | 2.57 s | ~14x |
| Image, Metal | 5.5 s | 0.32 s | ~17x |
| Image, CPU | 28.1 s | 2.93 s | ~10x |

The previous Metal text path was no faster than CPU: text matrices fall below Lattice's GEMM
dispatch threshold, so nothing reached the GPU. Parity after the switch (ignored model-backed
tests, `--features gemma-metal`): image CPU/Metal vs the pinned Python reference cosine
>= 0.99999998 at all four widths; text CPU/Metal/reference cosine >= 0.99999998; audio and video
unchanged (>= 0.99999996).

After the switch, post-change timings in the mixed-media test (128 dimensions): image
CPU 3.16 s / Metal 0.63 s, video 7.2 s / 2.1 s, audio 0.91 s / 0.97 s (audio is CPU-bound in the
tower, not the fusion).

## Building Lattice HEAD

Lattice pins Rust 1.94.1, whose proc-macro dylibs macOS 27 refuses to load
("mis-aligned LINKEDIT string pool"); build it with Procyon's 1.98.1 toolchain
(`RUSTUP_TOOLCHAIN=1.98.1`).

## Audio/video on the upstream architecture

Upstream's `MetalEmbeddingGemma2State::forward(embeddings, seq_len)` is exactly the entry our
fusion needs: it takes the combined `[seq, hidden]` rows. It is `pub(crate)`, as are
`scaled_embeddings` and `combine_rows`. Only `<image>` placeholders are special-cased there.
A small upstream change would let us delete `gemma_fusion.rs` and the GEMM-offload
`gemma_compute.rs`: a public `encode_rows_at_widths[_metal](ids, rows_by_position, widths)`
(placeholder ids gathered as padding, rows scattered in, per-layer signal from the combined rows).
The audio tower, feature extractor, projections and the video frame path would stay local.
