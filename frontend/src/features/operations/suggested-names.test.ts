import { describe, expect, it } from 'vitest';
import type { EntrySummary, Location } from '../../models';
import { nameWithoutExtension, suggestedArchiveName, suggestedFolderName } from './suggested-names';

function entry(name: string, kind: EntrySummary['kind'], uri: string): EntrySummary {
  return {
    id: uri as EntrySummary['id'],
    location: { providerId: 'local', uri },
    name,
    kind,
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

const folder = entry('beelden', 'directory', 'file:///a/2025-04%20CWZ/beelden');
const file = entry('report.final.pdf', 'file', 'file:///a/2025-04%20CWZ/report.final.pdf');
const directory: Location = { providerId: 'local', uri: 'file:///a/2025-04%20CWZ' };

describe('suggested names', () => {
  it('strips only a file extension', () => {
    expect(nameWithoutExtension(file)).toBe('report.final');
    expect(nameWithoutExtension(entry('v1.2', 'directory', 'file:///v1.2'))).toBe('v1.2');
    expect(nameWithoutExtension(entry('.env', 'file', 'file:///.env'))).toBe('.env');
  });

  it('suggests the cursor entry for a new folder, nothing on the parent row', () => {
    expect(suggestedFolderName(folder)).toBe('beelden');
    expect(suggestedFolderName(file)).toBe('report.final');
    expect(suggestedFolderName(entry('..', 'directory', 'fm:parent:/a'))).toBe('');
    expect(suggestedFolderName(undefined)).toBe('');
  });

  it('names an archive after its single source, else after the folder it is packed from', () => {
    const entries = [folder, file];
    expect(suggestedArchiveName([folder.location], entries, directory)).toBe('beelden');
    expect(suggestedArchiveName([file.location], entries, directory)).toBe('report.final');
    expect(suggestedArchiveName([folder.location, file.location], entries, directory)).toBe(
      '2025-04 CWZ',
    );
    expect(suggestedArchiveName([], [], { providerId: 'local', uri: 'file:///' })).toBe('archive');
  });
});
