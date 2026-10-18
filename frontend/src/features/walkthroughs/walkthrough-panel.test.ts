import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { WalkthroughPanel } from './walkthrough-panel';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('WalkthroughPanel', () => {
  it('reads the bundled guide without loading remote content', () => {
    m.mount(root, {
      view: () =>
        m(WalkthroughPanel, {
          selected: 'basics',
          onSelect: vi.fn(),
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();
    expect(root.querySelector('.fm-walkthrough-article')?.textContent).toContain('README.md');
    expect(root.querySelectorAll('.fm-walkthrough-nav button')).toHaveLength(7);
    const video = root.querySelector<HTMLVideoElement>('video');
    expect(video?.getAttribute('src')).toMatch(/basics-macos\.mp4/);
    expect(video?.getAttribute('preload')).toBe('none');
    expect(video?.hasAttribute('controls')).toBe(true);
    expect(video?.hasAttribute('autoplay')).toBe(false);
    expect(
      video?.querySelector('track[kind="captions"][srclang="en"]')?.getAttribute('src'),
    ).toMatch(/basics-en\.vtt/);
  });

  it('only offers the clip in basics and retains written instructions if media fails', () => {
    let selected: 'basics' | 'finding-files' = 'basics';
    m.mount(root, {
      view: () =>
        m(WalkthroughPanel, {
          selected,
          onSelect: (id) => {
            if (id === 'basics' || id === 'finding-files') selected = id;
          },
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();
    root.querySelector('video')?.dispatchEvent(new Event('error'));
    m.redraw.sync();
    expect(root.querySelector('video')).toBeNull();
    expect(root.querySelector('.fm-walkthrough-media-fallback')?.textContent).toContain(
      'written steps',
    );
    expect(root.querySelector('.fm-walkthrough-article')?.textContent).toContain('README.md');
    const reading = root.querySelector<HTMLElement>('.fm-walkthrough-reading');
    if (reading === null) throw new Error('guide reading area missing');
    reading.scrollTo = vi.fn();
    root.querySelectorAll<HTMLButtonElement>('.fm-walkthrough-nav button')[1]?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-walkthrough-media')).toBeNull();
  });

  it('unmounts playback on guide change and makes the clip replayable on return', () => {
    let selected: 'basics' | 'finding-files' = 'basics';
    m.mount(root, {
      view: () =>
        m(WalkthroughPanel, {
          selected,
          onSelect: (id) => {
            if (id === 'basics' || id === 'finding-files') selected = id;
          },
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();
    const firstVideo = root.querySelector('video');
    const reading = root.querySelector<HTMLElement>('.fm-walkthrough-reading');
    if (reading === null) throw new Error('guide reading area missing');
    reading.scrollTo = vi.fn();
    root.querySelectorAll<HTMLButtonElement>('.fm-walkthrough-nav button')[1]?.click();
    m.redraw.sync();
    expect(root.querySelector('video')).toBeNull();
    root.querySelectorAll<HTMLButtonElement>('.fm-walkthrough-nav button')[0]?.click();
    m.redraw.sync();
    const replay = root.querySelector('video');
    expect(replay).not.toBe(firstVideo);
    expect(replay?.hasAttribute('autoplay')).toBe(false);
    expect(replay?.hasAttribute('controls')).toBe(true);
  });

  it('opens a linked guide and closes with Escape', () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    m.mount(root, {
      view: () => m(WalkthroughPanel, { selected: 'basics', onSelect, onClose }),
    });
    m.redraw.sync();
    const panel = root.querySelector<HTMLElement>('.fm-walkthrough-panel');
    expect(document.activeElement).toBe(panel);
    const link = root.querySelector<HTMLAnchorElement>('a[href="workspaces.md"]');
    const reading = root.querySelector<HTMLElement>('.fm-walkthrough-reading');
    if (reading === null) throw new Error('guide reading area missing');
    reading.scrollTo = vi.fn();
    link?.click();
    expect(onSelect).toHaveBeenCalledWith('workspaces');
    expect(reading.scrollTo).toHaveBeenCalledWith(0, 0);
    panel?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
