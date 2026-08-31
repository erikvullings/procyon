import { describe, expect, it } from 'vitest';

import type { EntryMetadataDto } from '../api/generated/models/entryMetadataDto';
import type { EntrySummaryDto } from '../api/generated/models/entrySummaryDto';
import { entryMetadataFromDto, entrySummaryFromDto } from './entry';

const summaryFixture: EntrySummaryDto = {
  id: '985d4d6e-c37b-4135-90a0-ce0afe165fd9',
  location: { providerId: 'local', uri: 'file:///Users/erik/report.pdf' },
  name: 'report.pdf',
  kind: 'file',
  size: 2048,
  modifiedAt: '2026-07-30T00:00:00Z',
  createdAt: '2026-07-30T00:00:00Z',
  hidden: false,
  readOnly: false,
  extension: 'pdf',
  mimeType: 'application/pdf',
  iconKey: 'pdf',
  metadataRevision: 0,
};

describe('entrySummaryFromDto', () => {
  it('passes through populated optional fields unchanged', () => {
    expect(entrySummaryFromDto(summaryFixture)).toEqual({
      id: summaryFixture.id,
      location: summaryFixture.location,
      name: 'report.pdf',
      kind: 'file',
      hidden: false,
      readOnly: false,
      metadataRevision: 0,
      size: 2048,
      modifiedAt: '2026-07-30T00:00:00Z',
      createdAt: '2026-07-30T00:00:00Z',
      extension: 'pdf',
      mimeType: 'application/pdf',
      iconKey: 'pdf',
    });
  });

  it('normalizes wire `null` optional fields to omitted (`undefined`) properties', () => {
    // The backend serializes `Option<T>::None` fields as JSON `null` rather than omitting the
    // key, so the generated DTO types declare these as `T | null`; the hand-written frontend
    // model only declares them as optional (`T | undefined`). Without this normalization a
    // `null` value would slip past `!== undefined` guards (e.g. in entry-icons.ts) and crash.
    const dto: EntrySummaryDto = {
      ...summaryFixture,
      size: null,
      modifiedAt: null,
      createdAt: null,
      extension: null,
      mimeType: null,
      iconKey: null,
    };

    const entry = entrySummaryFromDto(dto);

    expect(entry.size).toBeUndefined();
    expect(entry.modifiedAt).toBeUndefined();
    expect(entry.createdAt).toBeUndefined();
    expect(entry.extension).toBeUndefined();
    expect(entry.mimeType).toBeUndefined();
    expect(entry.iconKey).toBeUndefined();
    expect('size' in entry).toBe(false);
    expect('mimeType' in entry).toBe(false);
  });
});

const metadataFixture: EntryMetadataDto = {
  entryId: '985d4d6e-c37b-4135-90a0-ce0afe165fd9',
  permissions: { readable: true, writable: true, executable: false, unixMode: 420 },
  ownership: { owner: 'erik', group: 'staff' },
  extendedAttributes: {},
  checksums: { sha256: 'abc123' },
  imageDimensions: { width: 1920, height: 1080 },
  media: { durationSeconds: 12.5, codec: 'h264', bitrateBps: 4_000_000 },
  archive: { entryCount: 3, uncompressedSize: 1024 },
  pluginFields: {},
};

describe('entryMetadataFromDto', () => {
  it('passes through populated nested optional objects unchanged', () => {
    expect(entryMetadataFromDto(metadataFixture)).toEqual({
      entryId: metadataFixture.entryId,
      extendedAttributes: {},
      checksums: { sha256: 'abc123' },
      pluginFields: {},
      permissions: { readable: true, writable: true, executable: false, unixMode: 420 },
      ownership: { owner: 'erik', group: 'staff' },
      imageDimensions: { width: 1920, height: 1080 },
      media: { durationSeconds: 12.5, codec: 'h264', bitrateBps: 4_000_000 },
      archive: { entryCount: 3, uncompressedSize: 1024 },
    });
  });

  it('normalizes `null` nested optional objects and fields to omitted properties', () => {
    const dto: EntryMetadataDto = {
      ...metadataFixture,
      permissions: { readable: true, writable: true, executable: false, unixMode: null },
      ownership: null,
      imageDimensions: null,
      media: { durationSeconds: null, codec: null, bitrateBps: null },
      archive: null,
    };

    const metadata = entryMetadataFromDto(dto);

    expect(metadata.permissions).toEqual({ readable: true, writable: true, executable: false });
    expect('unixMode' in (metadata.permissions ?? {})).toBe(false);
    expect(metadata.ownership).toBeUndefined();
    expect(metadata.imageDimensions).toBeUndefined();
    expect(metadata.media).toEqual({});
    expect(metadata.archive).toBeUndefined();
  });
});
