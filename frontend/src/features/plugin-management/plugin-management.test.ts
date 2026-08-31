import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { PluginDescriptor } from '../../models';
import { PluginManagement } from './plugin-management';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function fixturePlugin(overrides: Partial<PluginDescriptor> = {}): PluginDescriptor {
  return {
    id: 'example.copy-markdown-path',
    name: 'Copy Markdown Path',
    version: '1.0.0',
    description: 'Copies the selected entry path as a markdown link.',
    enabled: true,
    permissions: {
      selectedEntryMetadata: true,
      selectedEntryContentRead: false,
      filesystemRead: [],
      filesystemWrite: [],
      clipboardRead: false,
      clipboardWrite: true,
      network: [],
      processSpawn: false,
      notifications: false,
      settingsStorage: false,
    },
    ...overrides,
  };
}

describe('PluginManagement', () => {
  it('shows an empty state when no plugins are discovered', () => {
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [],
          onToggle: vi.fn(),
          onRequestLogs: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(root.querySelector('.fm-plugin-empty')?.textContent).toBe('No plugins discovered.');
  });

  it('lists plugin name, version, and description', () => {
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [fixturePlugin()],
          onToggle: vi.fn(),
          onRequestLogs: vi.fn(),
        }),
    });
    m.redraw.sync();

    const row = root.querySelector('.fm-plugin-row');
    expect(row?.querySelector('strong')?.textContent).toBe('Copy Markdown Path');
    expect(row?.querySelector('.fm-plugin-version')?.textContent).toBe('v1.0.0');
    expect(row?.querySelector('.fm-plugin-description')?.textContent).toBe(
      'Copies the selected entry path as a markdown link.',
    );
  });

  it('renders granted and denied permissions with a non-color-only marker', () => {
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [fixturePlugin()],
          onToggle: vi.fn(),
          onRequestLogs: vi.fn(),
        }),
    });
    m.redraw.sync();

    const items = [...root.querySelectorAll('.fm-plugin-permission')];
    const clipboardWrite = items.find((item) => item.textContent?.includes('Clipboard write'));
    const clipboardRead = items.find((item) => item.textContent?.includes('Clipboard read'));
    expect(clipboardWrite?.getAttribute('data-granted')).toBe('true');
    expect(clipboardWrite?.querySelector('.fm-plugin-permission-state')?.textContent).toBe('✓');
    expect(clipboardRead?.getAttribute('data-granted')).toBe('false');
    expect(clipboardRead?.querySelector('.fm-plugin-permission-state')?.textContent).toBe('✗');
  });

  it('shows the diagnostic for a failed or invalid plugin instead of hiding it', () => {
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [
            fixturePlugin({
              enabled: false,
              diagnostic: 'disabled after 3 consecutive failures: timed out',
            }),
          ],
          onToggle: vi.fn(),
          onRequestLogs: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(root.querySelector('.fm-plugin-diagnostic')?.textContent).toBe(
      'disabled after 3 consecutive failures: timed out',
    );
  });

  it('toggles a plugin through the mithril-materialized switch', () => {
    const onToggle = vi.fn().mockResolvedValue(undefined);
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [fixturePlugin({ enabled: false })],
          onToggle,
          onRequestLogs: vi.fn(),
        }),
    });
    m.redraw.sync();

    const toggleContainer = root.querySelector('.fm-plugin-toggle');
    toggleContainer?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));

    expect(onToggle).toHaveBeenCalledWith('example.copy-markdown-path', true);
  });

  it('surfaces a toggle failure inline without losing the row', async () => {
    const onToggle = vi.fn().mockRejectedValue(new Error('backend refused the request'));
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [fixturePlugin({ enabled: false })],
          onToggle,
          onRequestLogs: vi.fn(),
        }),
    });
    m.redraw.sync();

    const toggleContainer = root.querySelector('.fm-plugin-toggle');
    toggleContainer?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    await Promise.resolve();
    await Promise.resolve();
    m.redraw.sync();

    expect(root.querySelector('.fm-plugin-toggle-error')?.textContent).toBe(
      'backend refused the request',
    );
    expect(root.querySelector('.fm-plugin-row')).not.toBeNull();
  });

  it('loads and displays the bounded diagnostic log for a plugin', async () => {
    const onRequestLogs = vi.fn().mockResolvedValue([{ message: 'plugin timed out' }]);
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [fixturePlugin()],
          onToggle: vi.fn(),
          onRequestLogs,
        }),
    });
    m.redraw.sync();

    root
      .querySelector('.fm-plugin-view-log')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    m.redraw.sync();
    expect(document.body.textContent).toContain('Loading…');

    await Promise.resolve();
    await Promise.resolve();
    m.redraw.sync();

    expect(onRequestLogs).toHaveBeenCalledWith('example.copy-markdown-path');
    expect(document.body.textContent).toContain('plugin timed out');
  });

  it('reports a log loading failure without crashing', async () => {
    const onRequestLogs = vi.fn().mockRejectedValue(new Error('log fetch failed'));
    m.mount(root, {
      view: () =>
        m(PluginManagement, {
          plugins: [fixturePlugin()],
          onToggle: vi.fn(),
          onRequestLogs,
        }),
    });
    m.redraw.sync();

    root
      .querySelector('.fm-plugin-view-log')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
    await Promise.resolve();
    m.redraw.sync();

    expect(document.body.textContent).toContain('log fetch failed');
  });
});
