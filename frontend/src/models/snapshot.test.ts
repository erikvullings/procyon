import { describe, expect, it } from 'vitest';

import type { DirectorySnapshotDto } from '../api/generated/models/directorySnapshotDto';
import { directorySnapshotFromDto } from './snapshot';

const fixture: DirectorySnapshotDto = {
  paneId: 'pane-left',
  requestId: 'req-1',
  revision: 3,
  location: { providerId: 'local', uri: 'file:///Users/erik' },
  writable: true,
  entries: [
    {
      id: '985d4d6e-c37b-4135-90a0-ce0afe165fd9',
      location: { providerId: 'local', uri: 'file:///Users/erik/report.pdf' },
      name: 'report.pdf',
      kind: 'file',
      size: 2048,
      modifiedAt: null,
      createdAt: null,
      hidden: false,
      readOnly: false,
      extension: 'pdf',
      mimeType: null,
      iconKey: null,
      metadataRevision: 0,
    },
  ],
  totalKnownEntries: null,
  hasMore: false,
  continuationToken: null,
  loadingState: { type: 'loaded' },
};

describe('directorySnapshotFromDto', () => {
  it('normalizes `null` optional fields, including within each mapped entry', () => {
    const snapshot = directorySnapshotFromDto(fixture);

    expect(snapshot.totalKnownEntries).toBeUndefined();
    expect(snapshot.continuationToken).toBeUndefined();
    expect('totalKnownEntries' in snapshot).toBe(false);
    expect('continuationToken' in snapshot).toBe(false);
    expect(snapshot.entries).toHaveLength(1);
    expect(snapshot.entries[0]?.mimeType).toBeUndefined();
    expect('mimeType' in (snapshot.entries[0] ?? {})).toBe(false);
  });
});
