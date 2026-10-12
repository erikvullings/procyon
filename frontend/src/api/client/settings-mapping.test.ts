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
            hierarchyScope: 'exact',
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
    hierarchyScope: 'exact',
    enrolledRootIds: ['root-1'],
  });
});
