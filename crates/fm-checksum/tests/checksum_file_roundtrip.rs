//! Checksum-file writing, parsing and verification (task 0077 acceptance
//! criterion: "verification of a checksum file", reporting per-entry
//! match/mismatch/missing).

use std::io::Cursor;

use fm_checksum::{
    ChecksumAlgorithm, ChecksumFileError, VerificationStatus, read_checksum_file, verify,
    write_checksum_file,
};

const DIGEST_A: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const DIGEST_B: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn sample_file() -> String {
    let mut buffer = Vec::new();
    write_checksum_file(
        &[
            ("alpha.txt", DIGEST_A.to_owned()),
            ("nested/beta bin.dat", DIGEST_B.to_owned()),
        ],
        ChecksumAlgorithm::Sha256,
        &mut buffer,
    )
    .expect("writing must succeed");
    String::from_utf8(buffer).expect("output must be UTF-8")
}

#[test]
fn writes_the_coreutils_two_space_format() {
    let text = sample_file();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "# sha256");
    assert_eq!(lines[1], format!("{DIGEST_A}  alpha.txt"));
    assert_eq!(lines[2], format!("{DIGEST_B}  nested/beta bin.dat"));
}

#[test]
fn round_trips_through_the_reader() {
    let text = sample_file();
    let entries = read_checksum_file(Cursor::new(text)).expect("parsing must succeed");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "alpha.txt");
    assert_eq!(entries[0].digest, DIGEST_A);
    assert!(!entries[0].binary);
    assert_eq!(entries[1].path, "nested/beta bin.dat");
}

#[test]
fn parses_binary_mode_and_upper_case_digests_produced_by_other_tools() {
    let text = format!(
        "{}  text.txt\n{} *binary.bin\n",
        DIGEST_A.to_uppercase(),
        DIGEST_B
    );
    let entries = read_checksum_file(Cursor::new(text)).expect("parsing must succeed");
    assert_eq!(
        entries[0].digest, DIGEST_A,
        "digests are normalised to lower case"
    );
    assert!(!entries[0].binary);
    assert_eq!(entries[1].path, "binary.bin");
    assert!(entries[1].binary);
}

#[test]
fn infers_md5_and_crc32_from_the_digest_width() {
    let text = "d41d8cd98f00b204e9800998ecf8427e  a.txt\ncbf43926  b.txt\n";
    let entries = read_checksum_file(Cursor::new(text)).expect("parsing must succeed");
    assert_eq!(entries[0].algorithm, Some(ChecksumAlgorithm::Md5));
    assert_eq!(entries[1].algorithm, Some(ChecksumAlgorithm::Crc32));
    // 64 hex characters are ambiguous between SHA-256 and BLAKE3.
    let ambiguous = read_checksum_file(Cursor::new(format!("{DIGEST_A}  c.txt\n")))
        .expect("parsing must succeed");
    assert_eq!(ambiguous[0].algorithm, None);
}

#[test]
fn rejects_a_malformed_line_with_its_line_number() {
    let text = format!("# sha256\n{DIGEST_A}  ok.txt\nnot-a-checksum-line\n");
    let error = read_checksum_file(Cursor::new(text)).expect_err("parsing must fail");
    assert!(
        matches!(error, ChecksumFileError::MalformedLine { line: 3 }),
        "unexpected error: {error}"
    );
}

#[test]
fn reports_match_mismatch_and_missing_per_entry() {
    let text = format!("{DIGEST_A}  matching.txt\n{DIGEST_A}  wrong.txt\n{DIGEST_B}  gone.txt\n");
    let recorded = read_checksum_file(Cursor::new(text)).expect("parsing must succeed");

    let results = verify(
        [
            ("matching.txt", DIGEST_A),
            ("wrong.txt", DIGEST_B),
            // "gone.txt" is deliberately absent: a file the caller could not
            // find or could not open.
            ("extra.txt", DIGEST_A),
        ],
        &recorded,
    );

    assert_eq!(results.len(), 3, "only recorded entries are reported");
    assert_eq!(results[0].path, "matching.txt");
    assert_eq!(results[0].status, VerificationStatus::Match);
    assert!(results[0].is_match());
    assert_eq!(
        results[1].status,
        VerificationStatus::Mismatch {
            expected: DIGEST_A.to_owned(),
            actual: DIGEST_B.to_owned(),
        }
    );
    assert_eq!(results[2].status, VerificationStatus::Missing);
}

#[test]
fn verification_ignores_digest_case() {
    let recorded = read_checksum_file(Cursor::new(format!("{DIGEST_A}  a.txt\n")))
        .expect("parsing must succeed");
    let upper = DIGEST_A.to_uppercase();
    let results = verify([("a.txt", upper.as_str())], &recorded);
    assert_eq!(results[0].status, VerificationStatus::Match);
}

#[test]
fn skips_blank_and_comment_lines() {
    let text = format!("# sha256\n\n   \n{DIGEST_A}  a.txt\n");
    let entries = read_checksum_file(Cursor::new(text)).expect("parsing must succeed");
    assert_eq!(entries.len(), 1);
}
