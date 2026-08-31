//! Benchmarks for local filesystem directory listing performance
//!
//! Measures throughput of listing directories with various entry counts
//! and metadata retrieval complexity.

#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

/// Create a test directory with specified number of entries
fn create_test_directory(count: usize) -> TempDir {
    let dir = TempDir::new().expect("Failed to create temp directory");
    let dir_path = dir.path();

    for i in 0..count {
        let file_path = dir_path.join(format!("file-{:06}.txt", i));
        let mut file = File::create(&file_path).expect("Failed to create test file");
        writeln!(file, "content {}", i).expect("Failed to write to test file");
    }

    dir
}

/// List a directory's entries using std::fs
fn list_directory_std(path: &Path) -> std::io::Result<usize> {
    let entries = fs::read_dir(path)?.filter_map(|entry| entry.ok()).count();
    Ok(entries)
}

fn directory_listing_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("directory_listing");

    for count in [1_000, 10_000, 100_000].iter() {
        let test_dir = create_test_directory(*count);
        let dir_path = test_dir.path();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_entries", count)),
            &count,
            |b, _| b.iter(|| list_directory_std(dir_path).expect("Failed to list directory")),
        );
    }

    group.finish();
}

/// Benchmark metadata retrieval performance
fn metadata_collection_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("directory_metadata");

    for count in [1_000, 10_000].iter() {
        let test_dir = create_test_directory(*count);
        let dir_path = test_dir.path();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_entries", count)),
            &count,
            |b, _| {
                b.iter(|| {
                    let entries: Vec<_> = fs::read_dir(dir_path)
                        .expect("Failed to read directory")
                        .filter_map(|entry| {
                            let entry = entry.ok()?;
                            let metadata = entry.metadata().ok()?;
                            Some((entry.file_name(), metadata.len()))
                        })
                        .collect();
                    entries.len()
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    directory_listing_benchmark,
    metadata_collection_benchmark
);
criterion_main!(benches);
