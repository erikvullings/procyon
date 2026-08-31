//! Benchmarks for location parsing performance
//!
//! Measures parsing speed across different location URI formats and sizes.

#![allow(missing_docs)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use fm_domain::location::Location;

fn location_parsing_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("location_parsing");

    // Simple local file location
    group.bench_function("parse_local_file", |b| {
        b.iter(|| {
            let uri = black_box("file:///Users/test/documents/file.txt");
            let _ = Location::parse(uri);
        })
    });

    // Deeply nested local path
    group.bench_function("parse_deeply_nested", |b| {
        let deep_path =
            black_box("file:///a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z/file.txt");
        b.iter(|| {
            let _ = Location::parse(deep_path);
        })
    });

    // Path with Unicode characters
    group.bench_function("parse_unicode_path", |b| {
        let unicode_path = black_box("file:///Users/André/Документы/文件/ファイル.txt");
        b.iter(|| {
            let _ = Location::parse(unicode_path);
        })
    });

    // SFTP location
    group.bench_function("parse_sftp_location", |b| {
        let sftp_uri = black_box("sftp://user@host.com:22/path/to/file.txt");
        b.iter(|| {
            let _ = Location::parse(sftp_uri);
        })
    });

    // Batch parsing (realistic scenario)
    group.bench_function("parse_batch_1000", |b| {
        let paths: Vec<_> = (0..1000)
            .map(|i| format!("file:///test/{:06}/file.txt", i))
            .collect();
        b.iter(|| {
            for path in &paths {
                let _ = Location::parse(path);
            }
        })
    });

    group.finish();
}

criterion_group!(benches, location_parsing_benchmark);
criterion_main!(benches);
