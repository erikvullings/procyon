import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import { LlmRoleManagement } from './llm-role-management';

let root: HTMLElement;

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

async function mountLoaded(client: MockFileManagerClient): Promise<void> {
  m.mount(root, {
    view: () => m(LlmRoleManagement, { client, revision: 0 }),
  });
  await vi.waitFor(() => expect(root.textContent).not.toContain('Loading generation profiles'));
  m.redraw.sync();
}

describe('LlmRoleManagement', () => {
  it('shows the chat role with one leading secondary and one trailing primary action', async () => {
    await mountLoaded(new MockFileManagerClient());

    const chat = root.querySelector<HTMLElement>('[aria-label="Chat and summaries"]');
    expect(chat).not.toBeNull();
    const actions = [
      ...(chat?.querySelectorAll<HTMLButtonElement>('.fm-form-actions button') ?? []),
    ];
    expect(actions.map((button) => button.textContent?.trim())).toEqual(['Clear', 'Save role']);
    expect(actions[0]?.classList.contains('fm-form-actions-leading')).toBe(true);
    expect(actions[1]?.classList.contains('fm-primary-action')).toBe(true);
  });

  it('hides the embeddings role until advanced is on', async () => {
    await mountLoaded(new MockFileManagerClient());

    expect(root.querySelector('[aria-label="Semantic search (embeddings)"]')).toBeNull();
  });

  it('shows no status line for an unassigned or ready role', async () => {
    await mountLoaded(new MockFileManagerClient());

    expect(root.textContent).not.toContain('No endpoint assigned.');
    expect(root.textContent).not.toContain('Ready.');
  });
});
