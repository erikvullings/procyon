import type { ChecksumEntryDto } from '../api/generated/models/checksumEntryDto';
import type { ChecksumPageDto } from '../api/generated/models/checksumPageDto';
import type { DuplicateGroupDto } from '../api/generated/models/duplicateGroupDto';
import type { DuplicatePageDto } from '../api/generated/models/duplicatePageDto';
import type { HardlinkClusterDto } from '../api/generated/models/hardlinkClusterDto';
import type { VerificationReportDto } from '../api/generated/models/verificationReportDto';
import type { VerificationResultDto } from '../api/generated/models/verificationResultDto';
import type { Location } from './location';

/** A checksum algorithm the backend can compute (spec §18, task 0077). */
export type ChecksumAlgorithm = 'sha256' | 'blake3' | 'crc32' | 'md5';

/** Every algorithm, in the order they are offered to the user. */
export const CHECKSUM_ALGORITHMS: readonly ChecksumAlgorithm[] = [
  'sha256',
  'blake3',
  'crc32',
  'md5',
];

/** Human-readable label for an algorithm. */
export function checksumAlgorithmLabel(algorithm: ChecksumAlgorithm): string {
  switch (algorithm) {
    case 'sha256':
      return 'SHA-256';
    case 'blake3':
      return 'BLAKE3';
    case 'crc32':
      return 'CRC-32';
    case 'md5':
      return 'MD5';
  }
}

/** One entry's computed checksums. */
export interface ChecksumEntry {
  location: Location;
  relativePath: string;
  size: number;
  /** Digests keyed by lower-case algorithm name. */
  checksums: Readonly<Record<string, string>>;
  error?: string;
}

/** Converts the wire DTO into the frontend model, normalizing `null` to omitted. */
export function checksumEntryFromDto(dto: ChecksumEntryDto): ChecksumEntry {
  return {
    location: dto.location,
    relativePath: dto.relativePath,
    size: dto.size,
    checksums: dto.checksums ?? {},
    ...(dto.error == null ? {} : { error: dto.error }),
  };
}

/** A bounded page of a checksum job's results. */
export interface ChecksumPage {
  jobId: string;
  algorithms: readonly ChecksumAlgorithm[];
  offset: number;
  limit: number;
  total: number;
  totalEntries: number;
  entries: readonly ChecksumEntry[];
  isComplete: boolean;
  isCancelled: boolean;
  hasMore: boolean;
}

export function checksumPageFromDto(dto: ChecksumPageDto): ChecksumPage {
  return {
    jobId: dto.jobId,
    algorithms: dto.algorithms,
    offset: dto.offset,
    limit: dto.limit,
    total: dto.total,
    totalEntries: dto.totalEntries,
    entries: dto.entries.map(checksumEntryFromDto),
    isComplete: dto.isComplete,
    isCancelled: dto.isCancelled,
    hasMore: dto.hasMore,
  };
}

/** Outcome of verifying one checksum-file entry. */
export type VerificationStatus = 'match' | 'mismatch' | 'missing';

/** One verified path and its outcome. */
export interface VerificationResult {
  path: string;
  status: VerificationStatus;
  expected?: string;
  actual?: string;
}

export function verificationResultFromDto(dto: VerificationResultDto): VerificationResult {
  return {
    path: dto.path,
    status: dto.status,
    ...(dto.expected == null ? {} : { expected: dto.expected }),
    ...(dto.actual == null ? {} : { actual: dto.actual }),
  };
}

/** The full verification report. */
export interface VerificationReport {
  jobId: string;
  results: readonly VerificationResult[];
  matched: number;
  mismatched: number;
  missing: number;
}

export function verificationReportFromDto(dto: VerificationReportDto): VerificationReport {
  return {
    jobId: dto.jobId,
    results: dto.results.map(verificationResultFromDto),
    matched: dto.matched,
    mismatched: dto.mismatched,
    missing: dto.missing,
  };
}

/** Two or more paths that are the same file through a hardlink. Presented
 * separately from true duplicates because deleting one reclaims nothing. */
export interface HardlinkCluster {
  device: number;
  inode: number;
  locations: readonly Location[];
}

export function hardlinkClusterFromDto(dto: HardlinkClusterDto): HardlinkCluster {
  return { device: dto.device, inode: dto.inode, locations: dto.locations };
}

/** A set of byte-identical files. */
export interface DuplicateGroup {
  fullHash: string;
  size: number;
  hardlinkClusters: readonly HardlinkCluster[];
  distinctLocations: readonly Location[];
  reclaimableBytes: number;
}

export function duplicateGroupFromDto(dto: DuplicateGroupDto): DuplicateGroup {
  return {
    fullHash: dto.fullHash,
    size: dto.size,
    hardlinkClusters: dto.hardlinkClusters.map(hardlinkClusterFromDto),
    distinctLocations: dto.distinctLocations,
    reclaimableBytes: dto.reclaimableBytes,
  };
}

/** Counters describing how much work each detection stage performed. */
export interface DuplicateStats {
  candidates: number;
  sizeSurvivors: number;
  partiallyHashed: number;
  fullyHashed: number;
  bytesHashed: number;
  failed: number;
}

/** A bounded page of a duplicate scan's grouped results. */
export interface DuplicatePage {
  scanId: string;
  roots: readonly Location[];
  offset: number;
  limit: number;
  total: number;
  groups: readonly DuplicateGroup[];
  isComplete: boolean;
  isCancelled: boolean;
  hasMore: boolean;
  stats: DuplicateStats;
  warningsCount: number;
}

export function duplicatePageFromDto(dto: DuplicatePageDto): DuplicatePage {
  return {
    scanId: dto.scanId,
    roots: dto.roots,
    offset: dto.offset,
    limit: dto.limit,
    total: dto.total,
    groups: dto.groups.map(duplicateGroupFromDto),
    isComplete: dto.isComplete,
    isCancelled: dto.isCancelled,
    hasMore: dto.hasMore,
    stats: dto.stats,
    warningsCount: dto.warningsCount,
  };
}

/** Rendered checksum-file text, for the caller to copy or save. */
export interface ChecksumFile {
  suggestedName: string;
  content: string;
}

/** Confirms a checksum file was written to disk. */
export interface SavedChecksumFile {
  location: Location;
  bytesWritten: number;
}
