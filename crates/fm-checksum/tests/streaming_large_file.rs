//! Streaming behaviour on a file far larger than the read buffer (task 0077
//! acceptance criterion: "hashing streams files and does not load them into
//! memory").
//!
//! # What this test does and does not prove
//!
//! It proves the chunked loop is *correct* over hundreds of buffer refills
//! and that hashing a 64 MiB file neither fails nor exhausts memory. It does
//! **not** measure the process's resident memory: Rust offers no portable way
//! to observe that from a test, and a fabricated "memory" assertion would be
//! worse than none. The bounded-memory guarantee comes from the
//! implementation itself — `hash_stream`/`hash_blocking` allocate exactly one
//! `HASH_CHUNK_BYTES` buffer and never accumulate the input — which is
//! verifiable by inspection of `src/hash.rs`.

use std::fs::File;
use std::io::{BufWriter, Write};

use fm_checksum::{ChecksumAlgorithm, HASH_CHUNK_BYTES, hash_blocking};
use tokio_util::sync::CancellationToken;

/// 64 MiB: a thousand-fold more than the 64 KiB read buffer, large enough
/// that a naive read-it-all implementation would be obvious, yet still quick
/// to write and hash in CI.
const LARGE_FILE_BYTES: usize = 64 * 1024 * 1024;

#[test]
fn hashes_a_file_far_larger_than_the_read_buffer() {
    let directory = tempfile::tempdir().expect("temp dir must be created");
    let path = directory.path().join("large.bin");

    let pattern: Vec<u8> = (0..HASH_CHUNK_BYTES)
        .map(|index| (index % 253) as u8)
        .collect();
    let repeats = LARGE_FILE_BYTES / pattern.len();
    {
        let mut writer = BufWriter::new(File::create(&path).expect("file must be created"));
        for _ in 0..repeats {
            writer.write_all(&pattern).expect("write must succeed");
        }
        writer.flush().expect("flush must succeed");
    }

    let file = File::open(&path).expect("file must open");
    let set = hash_blocking(
        file,
        &[ChecksumAlgorithm::Sha256, ChecksumAlgorithm::Blake3],
        &CancellationToken::new(),
    )
    .expect("hashing a large file must succeed");

    assert_eq!(set.bytes_hashed() as usize, pattern.len() * repeats);

    // The same content hashed from memory must agree, confirming the
    // many-chunk path loses nothing.
    let whole: Vec<u8> = pattern.repeat(repeats);
    let reference = hash_blocking(
        whole.as_slice(),
        &[ChecksumAlgorithm::Sha256, ChecksumAlgorithm::Blake3],
        &CancellationToken::new(),
    )
    .expect("reference hashing must succeed");
    assert_eq!(set, reference);
}
