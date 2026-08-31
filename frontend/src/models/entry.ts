import type { ArchiveInfoDto } from '../api/generated/models/archiveInfoDto';
import type { EntryMetadataDto } from '../api/generated/models/entryMetadataDto';
import type { EntrySummaryDto } from '../api/generated/models/entrySummaryDto';
import type { GitFileStatusDto } from '../api/generated/models/gitFileStatusDto';
import type { MediaMetadataDto } from '../api/generated/models/mediaMetadataDto';
import type { OwnershipInfoDto } from '../api/generated/models/ownershipInfoDto';
import type { PermissionsInfoDto } from '../api/generated/models/permissionsInfoDto';
import type { EntryId } from './ids';
import type { Location } from './location';

/** The kind of a directory entry, mirroring `fm_transport_dto::EntryKindDto`. */
export type EntryKind = 'file' | 'directory' | 'symlink';

/** An entry's git working-tree status (task 0135), mirroring `fm_domain::GitFileStatus`. */
export type GitFileStatus = GitFileStatusDto;

/**
 * A compact summary of a directory entry, suitable for directory listings
 * (spec §5.2), mirroring `fm_transport_dto::EntrySummaryDto`.
 */
/** Single content match within a file from a recursive content search (task 0089). */
export interface ContentMatchSummary {
  /** 1-based line number of the match. */
  lineNumber: number;
  /** Byte offset of the match within the file. */
  offset: number;
  /** Length of the matched text in bytes. */
  length: number;
}

export interface EntrySummary {
  id: EntryId;
  location: Location;
  name: string;
  kind: EntryKind;
  size?: number;
  modifiedAt?: string;
  createdAt?: string;
  hidden: boolean;
  readOnly: boolean;
  extension?: string;
  mimeType?: string;
  iconKey?: string;
  metadataRevision: number;
  /** Content matches from recursive search, when this entry came from a content query (task 0089). */
  contentMatches?: ContentMatchSummary[];
  /** Git working-tree status, when this entry sits inside a local git working tree (task 0135). */
  gitStatus?: GitFileStatus;
}

/** Filesystem permission information for an entry. */
export interface PermissionsInfo {
  readable: boolean;
  writable: boolean;
  executable: boolean;
  unixMode?: number;
}

/** Ownership information for an entry. */
export interface OwnershipInfo {
  owner?: string;
  group?: string;
}

/** Pixel dimensions of an image entry. */
export interface ImageDimensions {
  width: number;
  height: number;
}

/** Media (audio/video) metadata for an entry. */
export interface MediaMetadata {
  durationSeconds?: number;
  codec?: string;
  bitrateBps?: number;
}

/** Archive-specific metadata for an entry (for example a `.zip` file). */
export interface ArchiveInfo {
  entryCount?: number;
  uncompressedSize?: number;
  compressedSize?: number;
  compressionMethod?: string;
}

/**
 * Detailed, non-eagerly-fetched metadata for a single entry (spec §5.2),
 * mirroring `fm_transport_dto::EntryMetadataDto`.
 */
export interface EntryMetadata {
  entryId: EntryId;
  permissions?: PermissionsInfo;
  ownership?: OwnershipInfo;
  extendedAttributes: Record<string, string>;
  checksums: Record<string, string>;
  imageDimensions?: ImageDimensions;
  media?: MediaMetadata;
  archive?: ArchiveInfo;
  pluginFields: Record<string, unknown>;
}

function permissionsInfoFromDto(dto: PermissionsInfoDto): PermissionsInfo {
  return {
    readable: dto.readable,
    writable: dto.writable,
    executable: dto.executable,
    ...(dto.unixMode == null ? {} : { unixMode: dto.unixMode }),
  };
}

function ownershipInfoFromDto(dto: OwnershipInfoDto): OwnershipInfo {
  return {
    ...(dto.owner == null ? {} : { owner: dto.owner }),
    ...(dto.group == null ? {} : { group: dto.group }),
  };
}

function mediaMetadataFromDto(dto: MediaMetadataDto): MediaMetadata {
  return {
    ...(dto.durationSeconds == null ? {} : { durationSeconds: dto.durationSeconds }),
    ...(dto.codec == null ? {} : { codec: dto.codec }),
    ...(dto.bitrateBps == null ? {} : { bitrateBps: dto.bitrateBps }),
  };
}

function archiveInfoFromDto(dto: ArchiveInfoDto): ArchiveInfo {
  return {
    ...(dto.entryCount == null ? {} : { entryCount: dto.entryCount }),
    ...(dto.uncompressedSize == null ? {} : { uncompressedSize: dto.uncompressedSize }),
    ...(dto.compressedSize == null ? {} : { compressedSize: dto.compressedSize }),
    ...(dto.compressionMethod == null ? {} : { compressionMethod: dto.compressionMethod }),
  };
}

/**
 * Converts the wire DTO into the frontend model, normalizing the wire's `null` (used for
 * absent optional fields) to `undefined` so consumers can rely on plain optional-property
 * checks (`!== undefined`) rather than also handling `null`.
 */
export function entrySummaryFromDto(dto: EntrySummaryDto): EntrySummary {
  return {
    id: dto.id,
    location: dto.location,
    name: dto.name,
    kind: dto.kind,
    hidden: dto.hidden,
    readOnly: dto.readOnly,
    metadataRevision: dto.metadataRevision,
    ...(dto.size == null ? {} : { size: dto.size }),
    ...(dto.modifiedAt == null ? {} : { modifiedAt: dto.modifiedAt }),
    ...(dto.createdAt == null ? {} : { createdAt: dto.createdAt }),
    ...(dto.extension == null ? {} : { extension: dto.extension }),
    ...(dto.mimeType == null ? {} : { mimeType: dto.mimeType }),
    ...(dto.iconKey == null ? {} : { iconKey: dto.iconKey }),
    ...(dto.gitStatus == null ? {} : { gitStatus: dto.gitStatus }),
  };
}

/** Converts the wire DTO into the frontend model, normalizing `null` to `undefined`. */
export function entryMetadataFromDto(dto: EntryMetadataDto): EntryMetadata {
  return {
    entryId: dto.entryId,
    extendedAttributes: dto.extendedAttributes,
    checksums: dto.checksums,
    pluginFields: dto.pluginFields,
    ...(dto.permissions == null ? {} : { permissions: permissionsInfoFromDto(dto.permissions) }),
    ...(dto.ownership == null ? {} : { ownership: ownershipInfoFromDto(dto.ownership) }),
    ...(dto.imageDimensions == null ? {} : { imageDimensions: dto.imageDimensions }),
    ...(dto.media == null ? {} : { media: mediaMetadataFromDto(dto.media) }),
    ...(dto.archive == null ? {} : { archive: archiveInfoFromDto(dto.archive) }),
  };
}
