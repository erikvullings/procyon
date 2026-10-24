import { expect, it } from 'vitest';
import { searchQueryFromParams } from '../../features/search/find-files-controller';
import { MockFileManagerClient } from './mock-file-manager-client';
import { settingsFromDto, settingsToDto } from './settings-mapping';

it('retains a selected concept URI when a semantic smart folder is saved and loaded', async () => {
  const client = new MockFileManagerClient();
  const settings = await client.getSettings();
  const source = {
    vocabularyId: 'unesco-psychology',
    conceptUri: 'http://vocabularies.unesco.org/thesaurus/concept13013',
  };
  const query = searchQueryFromParams(
    { providerId: 'local', uri: 'file:///Documents' },
    {
      mode: 'semantic',
      filenameQuery: '',
      semanticQuery: 'Mental stress',
      semanticLibraryId: 'library-1',
      semanticScope: 'entireLibrary',
      selectedConcept: source,
      contentRegex: false,
      contentCaseSensitive: false,
      contentWholeWord: false,
      recurse: true,
    },
    false,
  );
  const dto = settingsToDto({
    ...settings,
    savedSearches: [
      {
        id: 'saved-1',
        name: 'Mental stress (UNESCO)',
        pinned: true,
        query,
      },
    ],
  });
  expect(dto.savedSearches[0]?.query.semantic?.selectedConcept).toEqual(source);
  if (!dto.savedSearches[0]) throw new Error('Missing saved search DTO');
  dto.savedSearches[0].query.schemaVersion = 3;
  const loaded = settingsFromDto(dto);
  expect(loaded.savedSearches[0]?.query.schemaVersion).toBe(3);
  expect(loaded.savedSearches[0]?.query.semantic?.selectedConcept).toEqual(source);
  expect(loaded.savedSearches[0]?.query.concept).toBeUndefined();
});

it('preserves existing exact-concept predicates independently of similarity provenance', async () => {
  const client = new MockFileManagerClient();
  const settings = await client.getSettings();
  const dto = settingsToDto({
    ...settings,
    savedSearches: [
      {
        id: 'exact-1',
        name: 'Tagged concepts',
        pinned: true,
        query: {
          schemaVersion: 3,
          mode: 'concept',
          scope: { locations: [], recurse: true, showHidden: false },
          entryKinds: ['file'],
          mimeTypes: [],
          concept: {
            vocabularyId: 'vocabulary-1',
            conceptUri: 'http://example.test/concept',
            hierarchy: 'exact',
            libraryId: 'library-1',
            enrolledRootIds: ['root-1'],
          },
          gitStatuses: [],
          tags: [],
          metadata: {},
        },
      },
    ],
  });
  expect(settingsFromDto(dto).savedSearches[0]?.query.concept).toEqual({
    vocabularyId: 'vocabulary-1',
    conceptUri: 'http://example.test/concept',
    hierarchy: 'exact',
    libraryId: 'library-1',
    enrolledRootIds: ['root-1'],
  });
});

it('round-trips saved filter presets and tolerates older settings without them', async () => {
  const settings = await new MockFileManagerClient().getSettings();
  const presets = [
    {
      name: 'Big recent videos',
      criteria: {
        name: '*.mkv',
        kind: 'file' as const,
        minSize: 1_048_576,
        maxSize: null,
        modifiedWithinDays: 30,
        modifiedOlderThanDays: null,
        hidden: 'exclude' as const,
        executable: 'any' as const,
      },
    },
  ];
  const dto = settingsToDto({ ...settings, filterPresets: presets });
  expect(settingsFromDto(dto).filterPresets).toEqual(presets);

  const { filterPresets: _omitted, ...older } = dto;
  expect(settingsFromDto(older).filterPresets).toEqual([]);
  // Omitted numeric bounds (serialised `null` or absent) both mean "no bound".
  const sparse: typeof dto = {
    ...dto,
    filterPresets: [
      { name: 'x', criteria: { name: '', kind: 'any', hidden: 'any', executable: 'any' } },
    ],
  };
  expect(settingsFromDto(sparse).filterPresets[0]?.criteria.minSize).toBeNull();
});

it('round-trips sync profiles with schedule and last run and tolerates older settings', async () => {
  const settings = await new MockFileManagerClient().getSettings();
  const profiles = [
    {
      id: 'p1',
      name: 'Photos',
      left: { providerId: 'local', uri: 'file:///photos' },
      right: { providerId: 'sftp', uri: 'sftp://me@nas/photos' },
      mode: 'mirrorLeftToRight' as const,
      criteria: 'contentHash' as const,
      showHidden: true,
      include: ['*.jpg'],
      exclude: ['cache'],
      conflictPolicy: 'skip' as const,
      schedule: {
        enabled: true,
        intervalMinutes: 30,
        confirmation: 'autoApplyAdditions' as const,
      },
      lastRun: {
        at: '2026-10-24T10:00:00Z',
        trigger: 'scheduled' as const,
        outcome: 'failed' as const,
        plannedItems: 2,
        appliedItems: 0,
        message: 'Connection missing',
      },
    },
    {
      id: 'p2',
      name: 'Bare',
      left: { providerId: 'local', uri: 'file:///a' },
      right: { providerId: 'local', uri: 'file:///b' },
      mode: 'twoWayUpdate' as const,
      criteria: 'nameOnly' as const,
      showHidden: false,
      include: [],
      exclude: [],
      conflictPolicy: 'ask' as const,
      schedule: null,
      lastRun: null,
    },
  ];
  const dto = settingsToDto({ ...settings, syncProfiles: profiles });
  expect(settingsFromDto(dto).syncProfiles).toEqual(profiles);

  const { syncProfiles: _omitted, ...older } = dto;
  expect(settingsFromDto(older).syncProfiles).toEqual([]);
  // Serialised nulls and omitted optional members mean the same thing.
  const bare = dto.syncProfiles?.[1];
  if (bare === undefined) throw new Error('missing fixture');
  const { schedule: _schedule, lastRun: _lastRun, ...withoutOptionals } = bare;
  const sparse: typeof dto = { ...dto, syncProfiles: [withoutOptionals] };
  expect(settingsFromDto(sparse).syncProfiles[0]).toMatchObject({ schedule: null, lastRun: null });
});
