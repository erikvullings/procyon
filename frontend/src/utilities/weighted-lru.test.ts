import { describe, expect, it } from 'vitest';

import { WeightedLru } from './weighted-lru';

const byLength = (value: string | undefined): number => value?.length ?? 0;

describe('WeightedLru', () => {
  it('evicts the least recently used entries once the weight budget is exceeded', () => {
    const cache = new WeightedLru<string | undefined>(10, byLength);
    cache.set('a', 'xxxx');
    cache.set('b', 'xxxx');
    cache.set('c', 'xxxx');
    expect(cache.has('a')).toBe(false);
    expect(cache.has('b')).toBe(true);
    expect(cache.has('c')).toBe(true);
    expect(cache.weight).toBe(8);
  });

  it('counts a read as use so visible entries survive eviction', () => {
    const cache = new WeightedLru<string | undefined>(10, byLength);
    cache.set('a', 'xxxx');
    cache.set('b', 'xxxx');
    expect(cache.get('a')).toBe('xxxx');
    cache.set('c', 'xxxx');
    expect(cache.has('a')).toBe(true);
    expect(cache.has('b')).toBe(false);
  });

  it('keeps the entry just inserted even when it alone exceeds the budget', () => {
    const cache = new WeightedLru<string | undefined>(5, byLength);
    cache.set('a', 'x');
    cache.set('big', 'xxxxxxxxxx');
    expect(cache.has('big')).toBe(true);
    expect(cache.has('a')).toBe(false);
  });

  it('replaces an entry without double counting and stores undefined markers', () => {
    const cache = new WeightedLru<string | undefined>(100, byLength);
    cache.set('a', 'xxxx');
    cache.set('a', 'xx');
    cache.set('missing', undefined);
    expect(cache.weight).toBe(3);
    expect(cache.has('missing')).toBe(true);
    expect(cache.get('missing')).toBeUndefined();
    expect(cache.size).toBe(2);
  });

  describe('idle expiry', () => {
    function clocked() {
      let now = 0;
      const cache = new WeightedLru<string | undefined>(100, byLength, {
        maxAgeMs: 1_000,
        now: () => now,
      });
      return {
        cache,
        advance: (ms: number) => {
          now += ms;
        },
      };
    }

    it('treats an entry idle for longer than the maximum age as absent', () => {
      const { cache, advance } = clocked();
      cache.set('a', 'xxxx');
      advance(1_000);
      expect(cache.has('a')).toBe(true);
      advance(1);
      expect(cache.has('a')).toBe(false);
      expect(cache.get('a')).toBeUndefined();
      expect(cache.weight).toBe(0);
    });

    it('keeps an entry alive while it is read', () => {
      const { cache, advance } = clocked();
      cache.set('a', 'xxxx');
      advance(800);
      expect(cache.get('a')).toBe('xxxx');
      advance(800);
      expect(cache.get('a')).toBe('xxxx');
    });

    it('frees idle entries on prune without touching fresh ones', () => {
      const { cache, advance } = clocked();
      cache.set('old', 'xxxx');
      advance(900);
      cache.set('new', 'xx');
      advance(200);
      cache.prune();
      expect(cache.size).toBe(1);
      expect(cache.has('new')).toBe(true);
      expect(cache.weight).toBe(2);
    });

    it('never expires entries when no maximum age is set', () => {
      const cache = new WeightedLru<string | undefined>(100, byLength, { now: () => 1e12 });
      cache.set('a', 'xxxx');
      cache.prune();
      expect(cache.has('a')).toBe(true);
    });
  });
});
