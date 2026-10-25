import type { CorpusThemeStatus, CorpusThemes, ThemeMapPoint, ThemePassage } from '../../models';

/** Index revision of the in-process fixture library. */
export const MOCK_THEME_INDEX_REVISION = 'mock-revision-1';

const passage = (
  chunkId: string,
  sourceId: string,
  title: string,
  position: number,
  excerpt: string,
): ThemePassage => ({
  chunkId,
  sourceId,
  title,
  sourcePosition: position,
  sectionPath: [],
  provenance: JSON.stringify({
    kind: 'textLines',
    start_line: position + 1,
    end_line: position + 2,
  }),
  excerpt,
});

const CLUSTER_CENTRES: readonly (readonly [number, number])[] = [
  [-0.6, 0.4],
  [0.6, 0.3],
  [0.0, -0.7],
];

/** Deterministic points around each theme centre, plus one stray "no theme" point. */
const mockMapPoints = (): ThemeMapPoint[] => {
  const counts = [10, 8, 5];
  const points: ThemeMapPoint[] = [];
  counts.forEach((count, theme) => {
    const [cx, cy] = CLUSTER_CENTRES[theme] ?? [0, 0];
    for (let index = 0; index < count; index += 1) {
      const angle = (index / count) * Math.PI * 2;
      const radius = 0.08 + 0.02 * (index % 3);
      points.push({
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius,
        theme,
        passage: passage(
          `chunk-${theme}-${index}`,
          theme === 1 ? 'mock-source-b' : 'mock-source-a',
          theme === 1 ? 'Finance notes.txt' : 'Port report.txt',
          index,
          `Passage ${index + 1} about theme ${theme + 1}.`,
        ),
      });
    }
  });
  points.push({
    x: 0.85,
    y: -0.85,
    theme: null,
    passage: passage('chunk-d', 'mock-source-a', 'Port report.txt', 9, 'An unrelated remark.'),
  });
  return points;
};

/** Deterministic fixture themes: three topics, one outlier chunk, two documents. */
export const mockCorpusThemes = (): CorpusThemes => ({
  indexRevision: MOCK_THEME_INDEX_REVISION,
  fingerprint: 'mock-fingerprint',
  algorithmVersion: 'corpus-themes-kmeans/1',
  modelId: 'mock-embedding-model',
  chunkCount: 24,
  documentCount: 2,
  sampled: false,
  themes: [
    {
      id: 0,
      chunkCount: 10,
      documentCount: 2,
      share: 10 / 24,
      keywords: [
        { term: 'harbour', score: 0.31 },
        { term: 'shipping', score: 0.22 },
      ],
      passages: [
        passage('chunk-a', 'mock-source-a', 'Port report.txt', 3, 'The harbour handled shipping.'),
      ],
      concepts: [],
    },
    {
      id: 1,
      chunkCount: 8,
      documentCount: 1,
      share: 8 / 24,
      keywords: [
        { term: 'mortgage', score: 0.28 },
        { term: 'interest', score: 0.2 },
      ],
      passages: [
        passage('chunk-b', 'mock-source-b', 'Finance notes.txt', 1, 'Mortgage interest rose.'),
      ],
      concepts: [],
    },
    {
      id: 2,
      chunkCount: 5,
      documentCount: 1,
      share: 5 / 24,
      keywords: [{ term: 'violin', score: 0.4 }],
      passages: [passage('chunk-c', 'mock-source-b', 'Finance notes.txt', 7, 'A violin concert.')],
      concepts: [],
    },
  ],
  noTheme: {
    chunkCount: 1,
    documentCount: 1,
    share: 1 / 24,
    passages: [passage('chunk-d', 'mock-source-a', 'Port report.txt', 9, 'An unrelated remark.')],
  },
  documentMixtures: [
    {
      sourceId: 'mock-source-a',
      title: 'Port report.txt',
      chunkCount: 12,
      shares: [
        { theme: 0, chunkCount: 9, share: 0.75 },
        { theme: 2, chunkCount: 2, share: 2 / 12 },
        { theme: null, chunkCount: 1, share: 1 / 12 },
      ],
    },
    {
      sourceId: 'mock-source-b',
      title: 'Finance notes.txt',
      chunkCount: 12,
      shares: [
        { theme: 1, chunkCount: 8, share: 8 / 12 },
        { theme: 2, chunkCount: 3, share: 3 / 12 },
        { theme: 0, chunkCount: 1, share: 1 / 12 },
      ],
    },
  ],
  mixturesTruncated: false,
  mapPoints: mockMapPoints(),
  mapSampled: false,
});

export const mockCorpusThemeStatus = (): CorpusThemeStatus => ({
  indexRevision: MOCK_THEME_INDEX_REVISION,
  documentCount: 2,
  chunkCount: 24,
  modelId: 'mock-embedding-model',
  profileTooWeak: false,
});
