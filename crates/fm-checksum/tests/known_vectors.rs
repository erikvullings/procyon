//! Known-answer tests for every supported algorithm (task 0077 acceptance
//! criteria: "known-vector checksums").
//!
//! The SHA-256 and MD5 vectors are the canonical empty-string and `"abc"`
//! answers from NIST FIPS 180-4 and RFC 1321; the CRC-32 vector is the
//! standard `"123456789"` check value; the BLAKE3 vectors are the reference
//! implementation's own published test vectors.

use fm_checksum::{ChecksumAlgorithm, HASH_CHUNK_BYTES, hash_blocking, hash_stream};
use tokio_util::sync::CancellationToken;

const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const MD5_EMPTY: &str = "d41d8cd98f00b204e9800998ecf8427e";
const MD5_ABC: &str = "900150983cd24fb0d6963f7d28e17f72";
const CRC32_EMPTY: &str = "00000000";
const CRC32_CHECK: &str = "cbf43926";
const BLAKE3_EMPTY: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";
const BLAKE3_ABC: &str = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85";

fn digest(data: &[u8], algorithm: ChecksumAlgorithm) -> String {
    hash_blocking(data, &[algorithm], &CancellationToken::new())
        .expect("hashing must succeed")
        .get(algorithm)
        .expect("the requested digest must be present")
        .to_owned()
}

#[test]
fn matches_the_published_sha256_vectors() {
    assert_eq!(digest(b"", ChecksumAlgorithm::Sha256), SHA256_EMPTY);
    assert_eq!(digest(b"abc", ChecksumAlgorithm::Sha256), SHA256_ABC);
}

#[test]
fn matches_the_published_md5_vectors() {
    assert_eq!(digest(b"", ChecksumAlgorithm::Md5), MD5_EMPTY);
    assert_eq!(digest(b"abc", ChecksumAlgorithm::Md5), MD5_ABC);
}

#[test]
fn matches_the_published_crc32_vectors() {
    assert_eq!(digest(b"", ChecksumAlgorithm::Crc32), CRC32_EMPTY);
    assert_eq!(digest(b"123456789", ChecksumAlgorithm::Crc32), CRC32_CHECK);
}

#[test]
fn matches_the_published_blake3_vectors() {
    assert_eq!(digest(b"", ChecksumAlgorithm::Blake3), BLAKE3_EMPTY);
    assert_eq!(digest(b"abc", ChecksumAlgorithm::Blake3), BLAKE3_ABC);
}

#[tokio::test]
async fn computes_every_requested_algorithm_in_one_pass() {
    let set = hash_stream(
        &b"abc"[..],
        &ChecksumAlgorithm::ALL,
        &CancellationToken::new(),
    )
    .await
    .expect("hashing must succeed");
    assert_eq!(set.get(ChecksumAlgorithm::Sha256), Some(SHA256_ABC));
    assert_eq!(set.get(ChecksumAlgorithm::Blake3), Some(BLAKE3_ABC));
    assert_eq!(set.get(ChecksumAlgorithm::Md5), Some(MD5_ABC));
    assert_eq!(set.bytes_hashed(), 3);
}

/// Chunk-boundary regression: a digest computed over many buffer refills must
/// equal the one-shot answer, which is what proves the incremental update
/// loop carries state correctly.
#[tokio::test]
async fn a_multi_chunk_stream_agrees_with_a_reference_digest() {
    // Deliberately not a multiple of the chunk size, so the final partial
    // chunk is exercised too.
    let data: Vec<u8> = (0..(HASH_CHUNK_BYTES * 5 + 977))
        .map(|index| (index % 251) as u8)
        .collect();

    let streamed = hash_stream(
        data.as_slice(),
        &[ChecksumAlgorithm::Sha256],
        &CancellationToken::new(),
    )
    .await
    .expect("hashing must succeed");

    let mut reference = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut reference, &data);
    let expected: String = sha2::Digest::finalize(reference)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    assert_eq!(
        streamed.get(ChecksumAlgorithm::Sha256),
        Some(expected.as_str())
    );
    assert_eq!(streamed.bytes_hashed() as usize, data.len());
}
