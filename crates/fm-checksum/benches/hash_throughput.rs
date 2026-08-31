//! Benchmarks for streaming checksum throughput (task 0077 acceptance
//! criterion "throughput is benchmarked", task 0065).
//!
//! Measures each algorithm over a file large enough to exercise many refills
//! of the bounded `HASH_CHUNK_BYTES` buffer, so the numbers reflect the
//! streaming path the application actually uses rather than a single-shot
//! hash of an in-memory slice.

#![allow(missing_docs)]

use std::fs::File;
use std::io::{BufWriter, Write};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fm_checksum::{ChecksumAlgorithm, HASH_CHUNK_BYTES, hash_blocking};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// 64 MiB: large enough for a stable throughput reading and for the chunked
/// loop to dominate per-call overhead, small enough to stay CI-friendly.
const PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

fn create_payload(directory: &TempDir) -> std::path::PathBuf {
    let path = directory.path().join("payload.bin");
    let pattern: Vec<u8> = (0..HASH_CHUNK_BYTES)
        .map(|index| (index % 251) as u8)
        .collect();
    let mut writer = BufWriter::new(File::create(&path).expect("payload must be created"));
    for _ in 0..(PAYLOAD_BYTES / pattern.len()) {
        writer.write_all(&pattern).expect("payload must be written");
    }
    writer.flush().expect("payload must flush");
    path
}

fn hash_throughput_benchmark(criterion: &mut Criterion) {
    let directory = TempDir::new().expect("temp directory must be created");
    let path = create_payload(&directory);

    let mut group = criterion.benchmark_group("hash_throughput");
    group.throughput(Throughput::Bytes(PAYLOAD_BYTES as u64));
    group.sample_size(10);

    for algorithm in ChecksumAlgorithm::ALL {
        group.bench_with_input(
            BenchmarkId::from_parameter(algorithm.as_str()),
            &algorithm,
            |bencher, algorithm| {
                bencher.iter(|| {
                    let file = File::open(&path).expect("payload must open");
                    hash_blocking(file, &[*algorithm], &CancellationToken::new())
                        .expect("hashing must succeed")
                });
            },
        );
    }

    // The multi-algorithm case is what the UI actually requests when a user
    // ticks more than one box: one pass over the file feeding several hashers.
    group.bench_function("sha256+blake3 (single pass)", |bencher| {
        bencher.iter(|| {
            let file = File::open(&path).expect("payload must open");
            hash_blocking(
                file,
                &[ChecksumAlgorithm::Sha256, ChecksumAlgorithm::Blake3],
                &CancellationToken::new(),
            )
            .expect("hashing must succeed")
        });
    });

    group.finish();
}

criterion_group!(benches, hash_throughput_benchmark);
criterion_main!(benches);
