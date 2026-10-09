# Experimental EmbeddingGemma 2 Metal image probe

The optional `gemma-metal` feature offloads FP32 matrix multiplications in the
native vision tower, vision-to-language projection, and soft-token language
fusion using the pinned Lattice Metal GEMM primitive. Bicubic image preparation,
vision attention/normalization/pooling, and fusion attention/normalization remain
on CPU. BF16 checkpoint weights are decoded to FP32; there is no FP16 inference.
Small matrices below Lattice's dispatch threshold use its CPU GEMM. The managed
worker still constructs the CPU encoder, even when built with `gemma-metal`:
this is an explicitly selected, image-only local parity probe, not a user-facing
GPU mode. Audio and video remain unqualified on this path.

The real-checkpoint test `metal_image_matches_cpu_and_upstream_at_every_dimension`
uses the pinned revision `914f7f89142e33e77833254d9c9b90c3cef7303b`
and patterned PNG fixture. It measures decode-through-vector wall time for
each encoder (excluding checkpoint loading), verifies actual GPU dispatches,
and checks cosine > 0.99999 against both CPU and the upstream Python golden
at 128, 256, 512, and 768 dimensions. For each width the 768-dimensional
upstream vector is truncated and normalized using the same model contract.
Run on Apple Silicon after fetching the verified originals:

```sh
node scripts/fetch-embeddinggemma-probe.mjs
PROCYON_GEMMA_PROBE_MODEL_DIR="$PWD/target/semantic-model-cache/google--embeddinggemma-2/914f7f89142e33e77833254d9c9b90c3cef7303b" \
  /usr/bin/time -l cargo test --release -p fm-semantic-worker \
  --features gemma-metal --test embeddinggemma_image_parity \
  metal_image_matches_cpu_and_upstream_at_every_dimension -- --ignored --nocapture
```

`/usr/bin/time -l` reports process peak resident bytes. Run once to compile,
then execute the built test binary directly for an inference-process reading.
This is one
PNG on one machine; neither GPU speedup nor memory limits across supported
targets are established by a dispatch count. Do not enable GPU in installed
workers or reuse existing indexed vectors until cross-platform quality,
resource, and CPU-fallback qualification is complete.

## Local measurements (2026-10-09)

Apple M4 Max (40 GPU cores, Metal 4), macOS, pinned BF16 checkpoint decoded
to FP32, optimized Rust build. One patterned 128 x 96 PNG, each encoder
loaded separately for each width. Times cover preprocessing, vision tower,
projection and language fusion, but **exclude checkpoint loading**:

| Width | CPU | Metal | GPU GEMM dispatches | CPU/Python cosine | Metal/Python cosine |
| --- | ---: | ---: | ---: | ---: | ---: |
| 128 | 28.46 s | 29.32 s | 620 | 0.999999921 | 0.999999931 |
| 256 | 28.55 s | 29.10 s | 620 | 0.999999912 | 0.999999902 |
| 512 | 28.39 s | 30.16 s | 620 | 0.999999895 | 0.999999890 |
| 768 | 31.05 s | 29.33 s | 620 | 0.999999868 | 0.999999873 |

Metal/CPU cosine exceeded 0.99999999 at all four widths. The test process
itself peaked at 2,521,169,920 resident bytes (`/usr/bin/time -l`, invoking
the built test binary directly). The earlier test, which kept multiple
encoder instances alive concurrently, peaked at 7,926,235,136 bytes. The
final test releases one encoder before loading the next. An earlier repeat
also had a 50.70-second Metal outlier at width 256. These measurements show
**no reliable speedup**; vision attention remains on CPU, and each Metal GEMM
has synchronous dispatch/transfer overhead. More measurements, a
device-resident attention
path, and supported-target fallback/resource qualification are needed before
GPU can be advertised or enabled in a released worker.
