# Signed EmbeddingGemma 2 CPU resource probe (2026-10-09)

**Decision: not release-qualified.** Read-only [run 37992680322](https://github.com/erikvullings/procyon/actions/runs/37992680322)
on harness revision `6a691a14b0d94d9a5d977502ab4f1be220cbb6e0`
measured the *installed*, signed worker and original model from candidate
[run 37974984924](https://github.com/erikvullings/procyon/actions/runs/37974984924),
artifact **11640886748**. All four runner jobs passed; payload building,
catalog signing, collection, and publication were skipped. Each target
verified its detached catalog signature and exact candidate payload hashes
before installation. The [raw reports](embeddinggemma-signed-cpu-2026-10-09/)
contain the runner OS and memory, catalog digest, all 16 dimension results,
individual ingestion timings, three text-query timings per modality, and
worker peak-memory method. The Actions report artifacts are Linux ARM
**11646975201**, Linux x86 **11646037400**, macOS ARM **11646476390**, and
Windows x86 **11646322430** (retained for 14 days).

Each dimension starts a fresh native FP32 CPU worker and empty index with
the same installed original files. It ingests one synthetic 128x96 PNG,
one 440 Hz MP3, one two-second H.264 MP4, and one short plain-text document,
then sends three text queries for each modality through authenticated IPC.
It verifies retrieval and video timestamps, kills and restarts the worker,
and retrieves all four records again. The installation harness also checks
low disk, signature and payload tampering, interrupted resume, durable
restart, locking, explicit retain/delete uninstall, and clean reinstall.
Every target report records 13 lifecycle passes and zero network **artifact
reads during installation**; the initial Actions artifact download is not
an offline operation. `items/s` below is exactly one completed ingestion
divided by its submit-to-completion wall time, **not sustained throughput**.

| Target / runner hardware | Logical CPUs | RAM | Peak worker RSS across dimensions | Observation method |
| --- | ---: | ---: | ---: | --- |
| Linux ARM64, ubuntu24-arm64; CPU model reported `unknown` | 4 | 15.57 GiB | 3.46 GiB | Kernel `VmHWM`, max of original/restarted worker |
| Linux x86_64, ubuntu22; AMD EPYC 9V45 | 4 | 15.61 GiB | 3.47 GiB | Kernel `VmHWM`, max of original/restarted worker |
| macOS ARM64, macos15; Apple M1 (Virtual) | 3 | 7.00 GiB | **at least** 3.33 GiB | Maximum of 200 ms `ps` RSS samples |
| Windows x86_64, win25-vs2026; Intel Xeon Platinum 8573C | 4 | 15.99 GiB | 3.42 GiB | `PeakWorkingSet64`, max of original/restarted worker |

Wall is the test process from worker startup through ingestion, queries,
forced restart, and recovered queries. RSS is the **worker process only**;
installation, test-driver, OS cache, and other processes are excluded.
The four throughput columns are single-item ingestions/s, not batched
throughput. All four dimensions ran the same four inputs independently:

| Target | Dimensions | Wall (s) | Worker RSS (GiB) | Image/s | Audio/s | Video/s | Text/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Linux ARM64 | 128 | 177.61 | 3.46 | .009 | .362 | .017 | 3.220 |
| Linux ARM64 | 256 | 174.81 | 3.46 | .009 | .360 | .017 | 3.213 |
| Linux ARM64 | 512 | 170.01 | 3.46 | .010 | .360 | .017 | 3.217 |
| Linux ARM64 | 768 | 175.72 | 3.46 | .009 | .359 | .017 | 3.187 |
| Linux x86_64 | 128 | 64.88 | 3.47 | .027 | .445 | .050 | 4.817 |
| Linux x86_64 | 256 | 65.07 | 3.47 | .027 | .426 | .049 | 4.796 |
| Linux x86_64 | 512 | 64.55 | 3.47 | .027 | .466 | .050 | 4.815 |
| Linux x86_64 | 768 | 63.71 | 3.47 | .028 | .445 | .051 | 4.836 |
| macOS ARM64 | 128 | 128.38 | >=3.04 | .016 | .166 | .035 | 4.511 |
| macOS ARM64 | 256 | 101.64 | >=3.06 | .020 | .164 | .043 | 9.145 |
| macOS ARM64 | 512 | 108.87 | >=3.26 | .018 | .146 | .037 | 4.549 |
| macOS ARM64 | 768 | 97.98 | >=3.33 | .019 | .168 | .040 | 9.067 |
| Windows x86_64 | 128 | 161.11 | 3.42 | .010 | .266 | .028 | .927 |
| Windows x86_64 | 256 | 151.67 | 3.42 | .010 | .264 | .027 | 1.062 |
| Windows x86_64 | 512 | 152.74 | 3.42 | .010 | .263 | .028 | 1.068 |
| Windows x86_64 | 768 | 154.91 | 3.42 | .010 | .230 | .027 | 1.001 |

Linux ARM's virtual runner did not expose a usable CPU model; its image,
logical CPU count, OS release, and RAM are in the raw report. The macOS
sampled maximum is a **lower bound on true peak** and can miss startup or
transient spikes. Linux and Windows report OS process high-water values
sampled while each worker is still alive. Runner virtualization and
uncontrolled clock/cache contention prevent treating cross-target speed
differences as model-specific CPU comparisons. A single tiny synthetic
asset cannot establish sustained throughput, full-package memory,
concurrent-job behavior, larger/photographic media performance, or a
safe per-dimension memory budget. Queries are text-to-media/text,
not image/audio/video similarity queries; the raw timings cover three
requests per modality with only four indexed items, not representative
search latency. There are no independently labelled realistic negatives
or matched packaged E5 measurements here. This Apple M1 virtual runner
is not the earlier local M4 Max probe; its RSS and speed must not be
compared as though the workloads and machines matched.

To repeat while the candidate is retained, dispatch
`.github/workflows/release-semantic-components.yml` on this branch with
`qualify_installed_gemma=true`, `measure_installed_gemma=true`,
`qualification_run_id` empty, and an unused valid `release_tag`. The
measurement mode runs only the four read-only installed jobs and leaves
the Gemma publication guard untouched. Do not enable or publish Gemma
from this measurement alone.
