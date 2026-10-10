# Experimental EmbeddingGemma 2 Metal image probe

The optional `gemma-metal` feature offloads FP32 matrix multiplications in the
native vision tower, vision-to-language projection, and soft-token language
fusion using the pinned Lattice Metal GEMM primitive. Vision attention's
query/key and probability/value products also run through that primitive;
bicubic preprocessing, attention softmax, normalization, pooling, and fusion
attention remain on CPU. BF16 checkpoint weights are decoded to FP32; there
is no FP16 inference.
Small matrices below Lattice's dispatch threshold use its CPU GEMM. In a
macOS worker built with the optional `gemma-metal` feature, a consented,
image-only Gemma library selects Metal automatically when available; an
unavailable device or mixed image/audio/video library uses CPU with an
explicit backend report. The standard production bundle recipe still builds
without `gemma-metal`, and default E5 libraries are unchanged. Audio and
video remain unqualified on this Metal path.

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
This is one PNG on one machine; dispatch counts alone do not establish a
speedup or memory limits across supported targets. Do not enable GPU in
production workers until cross-platform quality, resource, and CPU-fallback
qualification is complete.

## Installed development worker (feature-gated automatic selection)

`pnpm dev:tauri:semantic:gemma:metal` builds a separately signed development
bundle with `developer-bundle,gemma-native,gemma-metal`; no runtime Metal
opt-in flag or environment variable is needed. The worker selects the backend
from its compiled feature, device availability and immutable library media.
`pnpm dev:tauri:semantic:gemma` builds a CPU-only worker. A development-only
`--gemma-cpu-images` launch argument provides paired CPU baseline measurements;
production feature builds reject it. Both the selected backend and CPU
fallback reason are printed to the host's inherited stderr and the selected
startup backend is recorded in the owner-only worker runtime directory's
`gemma-backend` file, refreshed on each managed launch. If the device is
unavailable or images are mixed with audio/video, the worker uses CPU. A
failed checkpoint load remains an error rather than a successful-looking
fallback. If a GPU GEMM does not dispatch during inference, it computes that
shape on CPU and reports the first occurrence; the tested FP32 CPU and Metal
vectors share the same pinned embedding space. Existing cancellation checks
are shared by both backends. This mode is not exposed in the app's settings.

The ignored desktop integration test
`installed_development_gemma_resolves_verified_original_files` first
installs the signed original checkpoint through the development component
manager, then launches the **installed** optimized worker from those verified
file paths. With `PROCYON_GEMMA_METAL_INTEGRATION=1`, it ingests the same patterned PNG into
separate forced-CPU baseline and automatically selected Metal indexes at all
four widths, queries it
through worker IPC, and compares the actual persisted image vectors with each
other and the checked-in pinned Python reference. Run against a built signed
development bundle:

```sh
PROCYON_GEMMA_DEVELOPER_BUNDLE="$PWD/target/semantic-developer-bundle/darwin-arm64-gemma-metal" \
PROCYON_GEMMA_METAL_INTEGRATION=1 \
  cargo test -p fm-desktop --features semantic-gemma --lib \
  installed_development_gemma_resolves_verified_original_files -- --ignored --nocapture
```

On the local M4 Max with the large competing LLM unloaded, pinned checkpoint
`914f7f89142e33e77833254d9c9b90c3cef7303b` and 128 x 96 PNG:

| Width | CPU ingest | Metal ingest | CPU query | Metal query | CPU sampled peak RSS | Metal sampled peak RSS | Metal/CPU cosine |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 128 | 28.34 s | 5.67 s | 63 ms | 66 ms | 2.35 GB | 2.35 GB | 0.999999989 |
| 256 | 28.30 s | 5.86 s | 55 ms | 76 ms | 2.35 GB | 2.44 GB | 1.000000001 |
| 512 | 29.10 s | 5.79 s | 54 ms | 69 ms | 2.38 GB | 2.47 GB | 0.999999997 |
| 768 | 28.16 s | 5.92 s | 60 ms | 74 ms | 2.35 GB | 2.36 GB | 1.000000006 |

These are wall times from IPC submission through completed image ingestion
and from query submission through response. Worker startup took 1.36-1.67
seconds separately; the memory figures are each worker's highest RSS sampled
with `ps` every 100 ms from startup through query, not a GPU-memory high-water
mark. CPU/Python cosine was 0.999999868 or better, and Metal/Python was
0.999999878 or better at all widths. Metal's selected backend was reported
by the launched worker; the direct parity test above separately confirmed
1,004 successful GPU dispatches per image on this checkpoint. This is a
development-only single-image result, **not** a release resource, device,
fallback, or retrieval-quality qualification. CPU fallback for mixed media
and a simulated failed dispatch are covered by focused tests; an actually
unavailable Apple device cannot be exercised on this Metal-capable machine.
The simulated unavailable-device startup and failed GEMM tests exercise both
fallback paths; concurrent images and pre-cancelled Metal images have separate
pinned-checkpoint tests.

The same test passed again after rebuilding the final signed bundle. In that
run CPU ingestion was 31.08/31.28/80.00/85.11 seconds and Metal ingestion
was 9.79/8.77/9.07/15.64 seconds at 128/256/512/768. All persisted vectors
retained the same CPU/Metal/Python parity. Startup varied from 1.40 to 4.21
seconds; sampled RSS remained between 2.35 and 2.46 GB. The large wall-time
variation, especially at 512/768 CPU, means these numbers are not an
uncontended hardware throughput guarantee. Both runs favored Metal for this
one image; further device/load coverage is required before product exposure.

With automatic device selection (no Metal launch flag), a newly signed
development worker completed the same installed image ingest/query at all
four widths. The forced-CPU development baseline took
28.34/28.06/27.83/29.29 seconds to ingest at 128/256/512/768; automatic
Metal took 5.81/5.83/5.80/6.12 seconds. Startup was 1.40-1.49 seconds,
query 56-91 ms, and sampled peak worker RSS 2.33-2.41 GB (all below the
test's 4 GiB bound). The actual persisted CPU/Metal vectors each matched
the pinned Python reference (cosine >0.99999); CPU/Metal cosine was
0.999999989/1.000000001/0.999999997/1.000000006. Startup logs reported
FP32 Metal on automatic launches and CPU on forced-baseline launches.

A final signed-bundle run also asserted the installed worker's `gemma-backend`
status file at every width before ingestion. It passed: forced CPU ingest
28.53/28.16/28.80/28.13 seconds versus automatic Metal
5.90/6.05/6.22/5.92 seconds at 128/256/512/768. Persisted vectors retained
the same >0.99999 parity; query latency was 55-121 ms and sampled peak
worker RSS was 2.33-2.46 GB. Results are local single-image measurements
on an M4 Max, not cross-device production qualification.

## Initial GEMM-only measurements (2026-10-09)

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
**no reliable speedup** in the initial implementation: vision attention was
still on CPU, and each Metal GEMM synchronized before continuing. The later
vision-attention change below resolves this local bottleneck; supported-target
fallback/resource qualification remains necessary before release.

## Repeat after unloading the large MLX-Serve model

The initial measurements ran while MLX-Serve reported a loaded 68 GB Qwen
model. After the user quit that model, its server process and API were gone.
The already-built release test binary was run twice more, without rebuilding,
against the same pinned checkpoint and PNG. A separate `omp --model` process
remained on the machine (about 1% CPU when checked), so these runs do not
prove exclusive access to the GPU.

| Width | CPU run 1 | Metal run 1 | CPU run 2 | Metal run 2 |
| --- | ---: | ---: | ---: | ---: |
| 128 | 26.41 s | 28.23 s | 27.62 s | 28.22 s |
| 256 | 27.95 s | 28.94 s | 27.58 s | 28.15 s |
| 512 | 27.75 s | 28.34 s | 27.43 s | 28.31 s |
| 768 | 27.46 s | 28.29 s | 27.49 s | 28.21 s |

Every vector retained the original >0.99999 CPU/Metal/upstream cosine and
620 Metal GEMM dispatches. The two test processes peaked at 2,482,454,528
and 2,489,204,736 resident bytes. The earlier 50.70-second Metal outlier did
not repeat, but Metal still took about 0.57-1.82 seconds longer per image
in these paired measurements. The large competing model is therefore not a
sufficient explanation for the lack of speedup. At this point the remaining
gap still needed stage-level profiling; CPU vision attention and per-GEMM
synchronization were candidates.

## Profile and vision-attention offload

On the same pinned checkpoint, a temporary stage timer and macOS CPU sample
identified the 2,394-patch vision attention loop as the dominant cost. The
unmodified CPU path spent 25.96-26.60 seconds in vision attention and about
27 seconds in the vision stage; preprocessing took about 5 ms, projection
43-47 ms, and fusion about 1 second. The initial Metal path still ran that
attention loop on CPU. Its 620 synchronous GEMM calls took approximately
1.06-1.10 seconds in total, including GPU dispatch/wait. Those calls
referenced 4.72 GB of input/output buffer extents; this is **not** measured
bus traffic because Lattice uses shared Metal buffers.

The bounded change uses the existing pinned FP32 Metal GEMM for each vision
head's query/key score product and probability/value product. Softmax and the
CPU default remain unchanged. On the instrumented optimized build,
attention took 2.95-3.08 seconds on this Metal path, vision 4.48-4.63 seconds,
projection 48-56 ms, and fusion 1.18-1.21 seconds. The 1,004 Metal dispatches
per image spent about 1.60-1.64 seconds total in synchronous GEMM calls;
the summed buffer extents were 13.99 GB (again, not measured transfer bytes).

After removing the temporary profiler, the final release test binary
repeated the paired comparison without recompilation or the large MLX-Serve
model:

| Width | CPU | Metal | CPU/Python cosine | Metal/Python cosine |
| --- | ---: | ---: | ---: | ---: |
| 128 | 28.10 s | 5.83 s | 0.999999921 | 0.999999927 |
| 256 | 28.39 s | 5.75 s | 0.999999912 | 0.999999902 |
| 512 | 27.83 s | 5.67 s | 0.999999895 | 0.999999891 |
| 768 | 27.98 s | 5.64 s | 0.999999868 | 0.999999878 |

The test passed all four widths with 1,004 successful GPU dispatches each,
Metal/CPU cosine >0.99999998, and 2,523,824,128 bytes peak resident memory
for the direct test process. On this one image and M4 Max this is roughly
4.8-5.0x faster end to end, excluding model loading. It does not qualify
other image shapes, devices, installed-worker GPU fallback, memory ceilings,
or retrieval quality; the standard release remains CPU-only, while the
opt-in installed development worker is evaluated separately above.
