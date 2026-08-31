//! Benchmarks for operation planning performance
//!
//! Measures the cost of planning file operations (especially copy) across
//! different directory structures and sizes.

#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;

/// Create a test directory tree for copy planning benchmarks
fn create_tree_structure(base: &std::path::Path, depth: usize, width: usize) -> usize {
    let mut count = 0;

    fn recurse(
        path: &std::path::Path,
        current_depth: usize,
        max_depth: usize,
        width: usize,
        count: &mut usize,
    ) {
        if current_depth > max_depth {
            return;
        }

        for i in 0..width {
            let dir_name = format!("dir-{:02}", i);
            let dir_path = path.join(&dir_name);
            let _ = fs::create_dir(&dir_path);

            // Create a few files at each level
            for j in 0..3 {
                let file_path = dir_path.join(format!("file-{:02}.txt", j));
                if let Ok(mut file) = File::create(&file_path) {
                    let _ = writeln!(file, "content at depth {}", current_depth);
                    *count += 1;
                }
            }

            if current_depth < max_depth {
                recurse(&dir_path, current_depth + 1, max_depth, width, count);
            }
        }
    }

    recurse(base, 0, depth, width, &mut count);
    count
}

/// Simple tree traversal to simulate copy planning enumeration
fn traverse_tree(path: &std::path::Path) -> usize {
    let mut count = 0;

    fn recurse(path: &std::path::Path, count: &mut usize) {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                *count += 1;
                if let Ok(metadata) = entry.metadata()
                    && metadata.is_dir()
                {
                    recurse(&entry.path(), count);
                }
            }
        }
    }

    recurse(path, &mut count);
    count
}

fn copy_planning_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("copy_planning");

    // Shallow, wide tree (many files in one level)
    group.bench_function("shallow_wide_100files", |b| {
        let test_dir = TempDir::new().expect("Failed to create temp directory");
        let dir_path = test_dir.path();

        // Create 100 files at root level
        for i in 0..100 {
            let file_path = dir_path.join(format!("file-{:03}.txt", i));
            if let Ok(mut file) = File::create(&file_path) {
                let _ = writeln!(file, "test {}", i);
            }
        }

        b.iter(|| traverse_tree(dir_path))
    });

    // Deep, narrow tree (nested directories)
    for depth in [10, 20, 50].iter() {
        let test_dir = TempDir::new().expect("Failed to create temp directory");
        let dir_path = test_dir.path();
        let expected_files = create_tree_structure(dir_path, *depth, 1); // depth, width=1

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("deep_narrow_depth_{}", depth)),
            depth,
            |b, _| b.iter(|| traverse_tree(dir_path)),
        );

        println!("  Created {} files at depth {}", expected_files, depth);
    }

    // Moderate tree (balanced)
    for width in [5, 10].iter() {
        let test_dir = TempDir::new().expect("Failed to create temp directory");
        let dir_path = test_dir.path();
        let expected_files = create_tree_structure(dir_path, 3, *width); // depth=3, variable width

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("balanced_width_{}", width)),
            width,
            |b, _| b.iter(|| traverse_tree(dir_path)),
        );

        println!("  Created {} files with width {}", expected_files, width);
    }

    group.finish();
}

criterion_group!(benches, copy_planning_benchmark);
criterion_main!(benches);
