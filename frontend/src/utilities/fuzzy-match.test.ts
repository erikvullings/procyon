import { describe, expect, it } from 'vitest';
import {
  bestFuzzyMatch,
  foldCase,
  fuzzyMatches,
  fuzzyScore,
  rankFuzzyMatches,
} from './fuzzy-match';

/** Narrows a `number | undefined` (from `fuzzyScore`) to `number` for assertions without `!`. */
function defined(value: number | undefined, label: string): number {
  if (value === undefined) throw new Error(label);
  return value;
}

describe('foldCase', () => {
  it('is case- and diacritic-insensitive', () => {
    expect(foldCase('Café')).toBe('cafe');
    expect(foldCase('Müller')).toBe('muller'); // NFD-fold: u-umlaut → u
    expect(foldCase('ÉCRIT')).toBe('ecrit');
    expect(foldCase('Ω')).toBe('ω'); // greek omega lowercased (not a combining mark)
  });
});

describe('fuzzyScore', () => {
  it('returns undefined when the query is not a subsequence', () => {
    expect(fuzzyScore('xyz', 'photo.png')).toBeUndefined();
    expect(fuzzyScore('pho', 'appph')).toBeUndefined(); // order matters
  });

  it('treats an empty query as matching everything with score 0', () => {
    expect(fuzzyScore('', 'anything')).toBe(0);
    expect(fuzzyScore('', '')).toBe(0);
  });

  it('ranks a leading, word-start, consecutive match highest', () => {
    // "photo" is a perfect leading/boundary/consecutive match of the query "photo"
    const perfect = fuzzyScore('photo', 'photo.png');
    // "xphoto" matches but not at the start
    const buried = fuzzyScore('photo', 'xphoto.png');
    expect(perfect).not.toBeUndefined();
    expect(buried).not.toBeUndefined();
    expect(defined(perfect, 'perfect matched')).toBeGreaterThan(defined(buried, 'buried matched'));
  });

  it('rewards word boundaries', () => {
    const atWord = fuzzyScore('fm', 'file_manager');
    const midWord = fuzzyScore('fm', 'aardvarkform');
    expect(atWord).not.toBeUndefined();
    expect(midWord).not.toBeUndefined();
    expect(defined(atWord, 'atWord matched')).toBeGreaterThan(defined(midWord, 'midWord matched'));
  });

  it('is diacritic- and case-insensitive', () => {
    expect(fuzzyMatches('cafe', 'Café.png')).toBe(true);
    expect(fuzzyMatches('MÜLLER', 'muller.txt')).toBe(true);
  });

  it('penalises large gaps (capped), word boundaries held equal', () => {
    const tight = fuzzyScore('ac', 'abc'); // small gap
    const spread = fuzzyScore('ac', 'axxxxxxc'); // large gap, no extra separators
    expect(tight).not.toBeUndefined();
    expect(spread).not.toBeUndefined();
    expect(defined(tight, 'tight matched')).toBeGreaterThan(defined(spread, 'spread matched'));
  });
});

describe('fuzzyMatches', () => {
  it('matches subsequences regardless of case/diacritics', () => {
    expect(fuzzyMatches('ph', 'photo.jpg')).toBe(true);
    expect(fuzzyMatches('ppt', 'presentation.pptx')).toBe(true);
    expect(fuzzyMatches('zzz', 'photo.jpg')).toBe(false);
  });
});

describe('bestFuzzyMatch', () => {
  const names = ['zebra.txt', 'photo.png', 'phonograph.wav', 'appliance.pdf'];
  it('returns the highest-scoring match and is stable on ties', () => {
    expect(bestFuzzyMatch('ph', names, (n) => n)).toBe('photo.png');
  });
  it('returns undefined when nothing matches', () => {
    expect(bestFuzzyMatch('qqq', names, (n) => n)).toBeUndefined();
  });
});

describe('rankFuzzyMatches', () => {
  it('orders best-first and drops non-matches', () => {
    const names = ['appliance.pdf', 'phonograph.wav', 'photo.png', 'nomatch.xyz'];
    const ranked = rankFuzzyMatches('op', names, (n) => n);
    expect(ranked).toContain('photo.png');
    expect(ranked).toContain('phonograph.wav');
    expect(ranked).not.toContain('nomatch.xyz');
    expect(ranked).not.toContain('appliance.pdf');
    // Ordered best-first (non-increasing score), ties stable.
    const scores = ranked.map((n) => defined(fuzzyScore('op', n), `op matches ${n}`));
    expect(scores).toEqual([...scores].sort((a, b) => b - a));
  });
});
