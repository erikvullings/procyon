//! `fm-checksum` domain types <-> transport DTO conversions for checksum
//! calculation, checksum-file verification and duplicate detection
//! (task 0077).
//!
//! Mirrors `comparison_mapping`: a self-contained cluster of pure conversion
//! functions with no dependency on the rest of the `FileManagerService`
//! facade.

use fm_checksum::{
    ChecksumAlgorithm, ChecksumEntryResult, DuplicateGroup, DuplicateStats, VerificationResult,
    VerificationStatus,
};
use fm_transport_dto::{
    ChecksumAlgorithmDto, ChecksumEntryDto, DuplicateGroupDto, DuplicateStatsDto,
    HardlinkClusterDto, VerificationResultDto, VerificationStatusDto,
};

pub(crate) const fn checksum_algorithm(dto: ChecksumAlgorithmDto) -> ChecksumAlgorithm {
    match dto {
        ChecksumAlgorithmDto::Sha256 => ChecksumAlgorithm::Sha256,
        ChecksumAlgorithmDto::Blake3 => ChecksumAlgorithm::Blake3,
        ChecksumAlgorithmDto::Crc32 => ChecksumAlgorithm::Crc32,
        ChecksumAlgorithmDto::Md5 => ChecksumAlgorithm::Md5,
    }
}

pub(crate) const fn checksum_algorithm_dto(algorithm: ChecksumAlgorithm) -> ChecksumAlgorithmDto {
    match algorithm {
        ChecksumAlgorithm::Sha256 => ChecksumAlgorithmDto::Sha256,
        ChecksumAlgorithm::Blake3 => ChecksumAlgorithmDto::Blake3,
        ChecksumAlgorithm::Crc32 => ChecksumAlgorithmDto::Crc32,
        ChecksumAlgorithm::Md5 => ChecksumAlgorithmDto::Md5,
    }
}

pub(crate) fn checksum_entry_dto(entry: &ChecksumEntryResult) -> ChecksumEntryDto {
    ChecksumEntryDto {
        location: entry.location.clone().into(),
        relative_path: entry.relative_path.clone(),
        size: entry.size,
        checksums: entry
            .checksums
            .iter()
            .map(|(algorithm, digest)| (algorithm.to_string(), digest.to_owned()))
            .collect(),
        error: entry.error.clone(),
    }
}

pub(crate) fn verification_result_dto(result: &VerificationResult) -> VerificationResultDto {
    let (status, expected, actual) = match &result.status {
        VerificationStatus::Match => (VerificationStatusDto::Match, None, None),
        VerificationStatus::Missing => (VerificationStatusDto::Missing, None, None),
        VerificationStatus::Mismatch { expected, actual } => (
            VerificationStatusDto::Mismatch,
            Some(expected.clone()),
            Some(actual.clone()),
        ),
    };
    VerificationResultDto {
        path: result.path.clone(),
        status,
        expected,
        actual,
    }
}

pub(crate) fn duplicate_group_dto(group: &DuplicateGroup) -> DuplicateGroupDto {
    DuplicateGroupDto {
        full_hash: group.full_hash.clone(),
        size: group.size,
        hardlink_clusters: group
            .hardlink_clusters
            .iter()
            .map(|cluster| HardlinkClusterDto {
                device: cluster.identity.device,
                inode: cluster.identity.inode,
                locations: cluster
                    .files
                    .iter()
                    .map(|file| file.entry.location.clone().into())
                    .collect(),
            })
            .collect(),
        distinct_locations: group
            .distinct_files
            .iter()
            .map(|file| file.entry.location.clone().into())
            .collect(),
        reclaimable_bytes: group.reclaimable_bytes(),
    }
}

pub(crate) const fn duplicate_stats_dto(stats: DuplicateStats) -> DuplicateStatsDto {
    DuplicateStatsDto {
        candidates: stats.candidates as u64,
        size_survivors: stats.size_survivors as u64,
        partially_hashed: stats.partially_hashed as u64,
        fully_hashed: stats.fully_hashed as u64,
        bytes_hashed: stats.bytes_hashed,
        failed: stats.failed as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_algorithm() {
        for algorithm in ChecksumAlgorithm::ALL {
            assert_eq!(
                checksum_algorithm(checksum_algorithm_dto(algorithm)),
                algorithm
            );
        }
    }

    #[test]
    fn carries_expected_and_actual_only_for_a_mismatch() {
        let matched = verification_result_dto(&VerificationResult {
            path: "a.txt".to_owned(),
            status: VerificationStatus::Match,
        });
        assert_eq!(matched.status, VerificationStatusDto::Match);
        assert!(matched.expected.is_none());
        assert!(matched.actual.is_none());

        let mismatched = verification_result_dto(&VerificationResult {
            path: "b.txt".to_owned(),
            status: VerificationStatus::Mismatch {
                expected: "aa".to_owned(),
                actual: "bb".to_owned(),
            },
        });
        assert_eq!(mismatched.status, VerificationStatusDto::Mismatch);
        assert_eq!(mismatched.expected.as_deref(), Some("aa"));
        assert_eq!(mismatched.actual.as_deref(), Some("bb"));
    }
}
