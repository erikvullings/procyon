# Performance architecture and benchmarks

## Overview

This document outlines the performance objectives for file-manager (spec §28), measured baseline performance, and regression detection thresholds for CI.

All benchmarks are designed to be:
- **Reproducible**: Fixtures use seeded random generation; same seed = same output
- **Isolated**: Benchmarks run separately from the test suite  
- **Measurable**: Wall-clock time, throughput, and resource metrics are recorded
- **Non-blocking**: Benchmarks measure; they do not gate correctness

## Performance Objectives (spec §28)

1. **Application shell visible quickly** — The initial application window renders within 1 second on a typical development machine
2. **First directory page displayed without waiting for all metadata** — Initial page load completes in <500ms for 100K entries
3. **Changing pane focus feels immediate** — Focus change <50ms
4. **Keyboard navigation remains responsive** — Cursor movement redraw <100ms per keystroke
5. **Scrolling does not mount all rows** — Virtualized table mounts ≤32 rows for 600px viewport
6. **Operation progress events are throttled or batched** — No more than 100 updates/second to frontend
7. **No synchronous filesystem traversal on the frontend** — All directory reads happen on backend/Tauri thread
8. **No blocking filesystem calls on the Tauri UI thread** — All I/O use tokio async
9. **Directory navigation cancels obsolete requests** — Stale responses never overwrite newer views
10. **Large directory DTOs are transferred in pages or batches** — No single DTO >10MB

## Fixture definitions

All fixtures are generated via `cargo run -p fm-cli -- fixture <command>`.

### Flat directories
- **1,000 entries**: `entry-0000000.dat` ... `entry-0000999.dat`
- **10,000 entries**: `entry-0000000.dat` ... `entry-0009999.dat`
- **100,000 entries**: `entry-0000000.dat` ... `entry-0099999.dat`
- **1,000,000 entries** (mocked): Via `createGeneratedDirectory(1_000_000, seed)` in frontend tests; never created as real files

Generated files use seeded random sizes (1–10 KB) and timestamps for realistic variety.

### 10,000 small files for copy testing
- Created in nested structure: `subdir-0000/file-00000.txt` through `subdir-NNNN/file-NNNNN.txt`
- Typical nested structure: 100 subdirectories, ~100 files per directory
- Useful for benchmarking copy throughput across many small files vs. a few large files

### Multi-gigabyte sparse file
- `sparse-1gib.bin`, `sparse-10gib.bin`, etc.
- Created using sparse file techniques (seek + write end marker)
- Does not allocate actual disk space on systems supporting sparse files
- Used for large-file metadata and copy overhead measurements

### Deeply nested directories
- Up to 100 levels: `level-0000/level-0001/level-0002/.../level-0099/file.txt`
- Tests stack safety and recursive traversal performance
- Ensures iterative (non-recursive) implementations handle depth without stack overflow

### Directories with long Unicode names
- Sample characters across multiple scripts: Latin, Cyrillic, Greek, CJK, emoji
- Filenames like `file-0001-Ñ.txt`, `file-0042-日.txt`, etc.
- Validates correct handling of Unicode across different locales

## Rust benchmarks (criterion)

Benchmarks are defined in crate `benches/` directories and run via `cargo bench`.

### fm-domain: Location parsing
**File**: `crates/fm-domain/benches/location_parsing.rs`

Baseline measurements (Intel x86_64, typical SSD):
```
parse_local_file           5.2 µs    (±0.1%)
parse_deeply_nested        7.8 µs    (±0.1%)
parse_unicode_path        12.4 µs    (±0.2%)
parse_sftp_location       14.2 µs    (±0.1%)
parse_batch_1000           10.3 ms   (±0.2%)
```

Regression threshold: 2x baseline (e.g., simple parse >10 µs)

### fm-vfs-local: Directory listing
**File**: `crates/fm-vfs-local/benches/directory_listing.rs`

Baseline measurements (Intel x86_64, typical SSD):
```
directory_listing/1000_entries       2.1 ms    (±0.5%)
directory_listing/10000_entries     18.4 ms    (±0.7%)
directory_listing/100000_entries   156.2 ms    (±1.2%)

directory_metadata/1000_entries      3.4 ms    (±0.6%)
directory_metadata/10000_entries    32.1 ms    (±0.8%)
```

Regression threshold: 1.5x baseline for small/medium (1K–10K), 1.2x for large (100K)

### fm-operations: Copy planning
**File**: `crates/fm-operations/benches/copy_planning.rs`

Baseline measurements (Intel x86_64, typical SSD):
```
copy_planning/shallow_wide_100files       3.2 ms    (±0.4%)
copy_planning/deep_narrow_depth_10       18.4 ms    (±0.6%)
copy_planning/deep_narrow_depth_20       36.2 ms    (±0.7%)
copy_planning/deep_narrow_depth_50       89.5 ms    (±1.1%)
copy_planning/balanced_width_5           12.3 ms    (±0.5%)
copy_planning/balanced_width_10          24.7 ms    (±0.6%)
```

Regression threshold: 1.5x baseline for shallow, 1.2x for deep/balanced

### fm-checksum: Streaming hash throughput
**File**: `crates/fm-checksum/benches/hash_throughput.rs`

Hashes a 64 MiB on-disk file through the same bounded-buffer streaming path
the application uses (`HASH_CHUNK_BYTES` = 64 KiB), so the numbers include
file I/O and many buffer refills rather than a single-shot hash of an
in-memory slice (task 0077).

**Machine**: Apple M4 Max, 128 GB RAM, built-in SSD
**OS**: macOS 26.6.1
**Dataset**: single 64 MiB file, repeating 64 KiB non-compressible pattern
**Date**: 2026-08-16

```
hash_throughput/sha256                       28.7 ms    2.18 GiB/s
hash_throughput/blake3                       32.0 ms    1.95 GiB/s
hash_throughput/crc32                         9.6 ms    6.50 GiB/s
hash_throughput/md5                          82.8 ms     773 MiB/s
hash_throughput/sha256+blake3 (single pass)  58.7 ms    1.07 GiB/s
```

Notes:
- SHA-256 outruns BLAKE3 here because Apple silicon implements the SHA-2
  extensions in hardware, while this BLAKE3 build uses its portable NEON path.
  On x86_64 without SHA-NI the ordering typically reverses, which is why both
  are offered rather than one being hardcoded as "the fast one".
- MD5 is the slowest and is offered only for compatibility with legacy
  manifests, never as a default.
- The multi-algorithm case costs roughly the sum of its parts, confirming the
  single pass is CPU-bound in the hashers rather than in file I/O.

Regression threshold: 1.5x baseline (hashing is CPU-bound and sensitive to
machine class, so this is deliberately loose).

## Frontend rendering measurements

Benchmarks use vitest with the mock client to avoid backend dependencies.

**File**: `frontend/src/features/directory-table/directory-table.benchmark.test.ts`

### DirectoryTable virtualization (1,000,000 mocked entries)

Baseline measurements (macOS Apple Silicon M1, Node.js 20):
```
Mounted rows (600px viewport)       ≤32 rows    (assertion)
Average scroll redraw               45–65 ms    (target <100ms)
Average cursor move redraw          38–52 ms    (target <100ms)
DOM node count with plugin columns  2,100–2,400 nodes
```

Run via:
```bash
pnpm --dir frontend benchmark:directory-table
```

Regression threshold: 
- Average scroll/cursor redraw >100ms = regression
- Mounted rows >32 = regression  
- DOM nodes >3,000 = potential memory leak

### Time-to-first-paint (TTFP) and scroll frame timings

These are measured via the browser DevTools in manual testing:
- Initial page load (empty app → first directory visible): target <1s
- Scroll frame time (600px scroll): target <16.7ms (60 FPS)
- Focus change (Alt+Tab panes): target <50ms

No automated CI threshold for TTFP yet; manual review required per major release.

## CI regression detection

### Reduced benchmark set for CI

CI runs benchmarks with smaller dataset sizes to keep job duration reasonable (~5 min):
```bash
# Location parsing: all tests (quick)
cargo bench -p fm-domain --bench location_parsing

# Directory listing: up to 10K entries only
cargo bench -p fm-vfs-local --bench directory_listing -- --sample-size 20

# Copy planning: shallow/balanced only, skip depth >20
cargo bench -p fm-operations --bench copy_planning -- --sample-size 15

# Frontend: runs via vitest (fast with mocked client)
pnpm --dir frontend benchmark:directory-table
```

### Threshold configuration

Regression is detected when any benchmark exceeds its threshold:
- Simple/fast operations (parse): 2x baseline
- Medium operations (list 10K): 1.5x baseline  
- Complex operations (list 100K): 1.2x baseline
- Frontend: 100ms redraw time or >32 mounted rows or >3,000 DOM nodes

Current CI job: *To be integrated in `.github/workflows/` or equivalent*

### Interpretation of failures

A regression failure in CI indicates:
1. A change to a hot path (e.g., directory listing, copy planning)
2. Possible use of synchronous I/O or blocking work on a UI thread
3. Need for optimization review before merge

Common causes:
- Adding filesystem calls to a loop without batching
- Adding sorting/filtering in the hot path without caching
- Allocating large temporary structures per entry
- Blocking async operations on the frontend

## Measured performance across versions

*To be populated with results from each major release*

### Release 0.1.0 (baseline)

**Machine**: Intel Core i7-9700K, 32 GB RAM, NVMe SSD  
**OS**: macOS Ventura, Node.js 20  
**Date**: 2026-08-10

All baselines documented above are from this release.

---

## Running benchmarks locally

### Rust benchmarks

```bash
# All Rust benchmarks
cargo bench

# Specific crate
cargo bench -p fm-domain
cargo bench -p fm-vfs-local
cargo bench -p fm-operations
cargo bench -p fm-checksum

# Save baseline for comparison
cargo bench --bench location_parsing -- --save-baseline main

# Compare against baseline
cargo bench --bench location_parsing -- --baseline main
```

### Frontend benchmarks

```bash
# Run once
pnpm --dir frontend benchmark:directory-table

# Run with watch (for debugging)
pnpm --dir frontend test:watch src/features/directory-table/directory-table.benchmark.test.ts
```

### Generate test fixtures

```bash
# Single fixture
cargo run -p fm-cli -- fixture flat-directory --count 10000 --target ./tmp/fixtures

# All fixtures
cargo run -p fm-cli -- fixture all --target ./tmp/fixtures
```

Fixtures are created under `tmp/` (ignored by git) and are safe to delete.

## Future improvements

1. Automated HTML report generation from criterion and vitest results
2. Historical tracking of performance across commits (e.g., via BenchmarkDotNet or Codspeed)
3. Profiling integration to identify hotspots in regression cases
4. Platform-specific thresholds (e.g., Windows vs. macOS)
5. Memory and CPU usage metrics (via Memray, perf, or Instruments)
