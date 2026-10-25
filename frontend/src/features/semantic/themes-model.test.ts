import { describe, expect, it } from 'vitest';

import { mockCorpusThemes } from '../../api/client/mock-corpus-themes';
import {
  isThemesStale as isStaleRejection,
  NO_THEME_COLOUR,
  percentText,
  pickPoint,
  provenanceLine,
  themeColour,
  themeKeywordLabel,
  themesAreStale,
  topDocumentsForTheme,
  toScreen,
} from './themes-model';

const GEOMETRY = { width: 220, height: 120, padding: 10 } as const;

describe('themes model', () => {
  it('gives every theme a distinct stable colour and "no theme" a neutral one', () => {
    const colours = Array.from({ length: 20 }, (_, theme) => themeColour(theme));
    expect(new Set(colours).size).toBe(20);
    expect(themeColour(3)).toBe(themeColour(3));
    expect(themeColour(null)).toBe(NO_THEME_COLOUR);
    expect(colours).not.toContain(NO_THEME_COLOUR);
  });

  it('flips y so that larger y is higher on the canvas and respects padding', () => {
    expect(toScreen({ x: -1, y: 1 }, GEOMETRY)).toEqual({ px: 10, py: 10 });
    expect(toScreen({ x: 1, y: -1 }, GEOMETRY)).toEqual({ px: 210, py: 110 });
    expect(toScreen({ x: 0, y: 0 }, GEOMETRY)).toEqual({ px: 110, py: 60 });
  });

  it('picks the nearest point inside the radius and nothing outside it', () => {
    const points = mockCorpusThemes().mapPoints;
    const target = points[0];
    if (target === undefined) throw new Error('fixture has points');
    const screen = toScreen(target, GEOMETRY);
    expect(pickPoint(points, screen.px + 1, screen.py - 1, GEOMETRY, 6)).toBe(target);
    expect(pickPoint(points, -50, -50, GEOMETRY, 6)).toBeUndefined();
  });

  it('prefers the closer of two overlapping points', () => {
    const [first, second] = mockCorpusThemes().mapPoints;
    if (first === undefined || second === undefined) throw new Error('fixture has points');
    const near = { ...first, x: 0, y: 0 };
    const far = { ...second, x: 0.02, y: 0 };
    const centre = toScreen({ x: 0, y: 0 }, GEOMETRY);
    expect(pickPoint([far, near], centre.px, centre.py, GEOMETRY, 10)).toBe(near);
  });

  it('reads the line to open from text, code and JSONL provenance', () => {
    expect(provenanceLine('{"kind":"textLines","start_line":7,"end_line":9}')).toBe(7);
    expect(provenanceLine('{"kind":"exact","value":{"kind":"jsonlRecord","line":1042}}')).toBe(
      1042,
    );
    expect(
      provenanceLine(
        '{"kind":"span","first":{"kind":"jsonlRecord","line":5},"last":{"kind":"jsonlRecord","line":8}}',
      ),
    ).toBe(5);
    expect(provenanceLine('{"kind":"pdfBlock","page_number":3}')).toBeUndefined();
    expect(provenanceLine('not json')).toBeUndefined();
    expect(provenanceLine('{"kind":"jsonlRecord","line":0}')).toBeUndefined();
  });

  it('detects stale analyses only when the status revision differs', () => {
    const themes = mockCorpusThemes();
    expect(themesAreStale(themes, undefined)).toBe(false);
    expect(themesAreStale(themes, { indexRevision: themes.indexRevision })).toBe(false);
    expect(themesAreStale(themes, { indexRevision: 'other' })).toBe(true);
  });

  it('ranks the documents of a theme by contribution, including no-theme', () => {
    const { documentMixtures } = mockCorpusThemes();
    const harbour = topDocumentsForTheme(documentMixtures, 0, 5);
    expect(harbour.map((row) => row.title)).toEqual(['Port report.txt', 'Finance notes.txt']);
    expect(topDocumentsForTheme(documentMixtures, 0, 1)).toHaveLength(1);
    expect(topDocumentsForTheme(documentMixtures, null, 5).map((row) => row.title)).toEqual([
      'Port report.txt',
    ]);
    expect(topDocumentsForTheme(documentMixtures, 99, 5)).toEqual([]);
  });

  it('formats labels and percentages', () => {
    expect(themeKeywordLabel([{ term: 'a' }, { term: 'b' }, { term: 'c' }, { term: 'd' }])).toBe(
      'a, b, c',
    );
    expect(percentText(0.4167)).toBe('42%');
    expect(percentText(0.004)).toBe('<1%');
    expect(percentText(0)).toBe('0%');
  });

  it('recognises the stale-themes rejection and nothing else', () => {
    expect(isStaleRejection({ details: { reason: 'corpusThemesStale' } })).toBe(true);
    expect(isStaleRejection({ details: { reason: 'scopeEmpty' } })).toBe(false);
    expect(isStaleRejection(new Error('x'))).toBe(false);
    expect(isStaleRejection(null)).toBe(false);
  });
});
