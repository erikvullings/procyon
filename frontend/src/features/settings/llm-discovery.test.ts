import m from 'mithril';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import { LlmDiscovery } from './llm-discovery';

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

function button(label: string): HTMLButtonElement | undefined {
  return [...root.querySelectorAll('button')].find((item) => item.textContent === label);
}

function mount(client: MockFileManagerClient, firstRun: boolean) {
  const onConnectCloud = vi.fn();
  const onConfigured = vi.fn();
  m.mount(root, {
    view: () => m(LlmDiscovery, { client, firstRun, onConnectCloud, onConfigured }),
  });
  return { onConnectCloud, onConfigured };
}

it('presents a fixture server and binds the chosen model to both roles', async () => {
  const client = new MockFileManagerClient({
    detectedLlmServers: [
      {
        name: 'Fixture AI',
        preset: 'vllm',
        baseUrl: 'http://127.0.0.1:12345',
        models: ['first-model', 'second-model'],
      },
    ],
  });
  const { onConfigured } = mount(client, true);
  await vi.waitFor(() => expect(root.textContent).toContain('Fixture AI'));
  const select = root.querySelector('select');
  expect(select?.value).toBe('first-model');
  if (select === null) throw new Error('Model picker missing');
  select.value = 'second-model';
  select.dispatchEvent(new Event('change', { bubbles: true }));
  button('Use for chat and search')?.click();
  await vi.waitFor(() => expect(onConfigured).toHaveBeenCalledOnce());
  const { chat, embeddings } = await client.listLlmRoles();
  expect(chat?.model).toBe('second-model');
  expect(embeddings?.profileId).toBe(chat?.profileId);
  expect(embeddings?.model).toBe('second-model');
  expect((await client.getLlmDiscoveryStatus()).firstRunPending).toBe(false);
});

it('remembers Later, dismisses the Settings reminder, and can reset or start cloud setup', async () => {
  const client = new MockFileManagerClient();
  mount(client, true);
  await vi.waitFor(() => expect(root.textContent).toContain('No local AI servers found.'));
  button('Later')?.click();
  await vi.waitFor(async () =>
    expect((await client.getLlmDiscoveryStatus()).showBanner).toBe(true),
  );

  m.mount(root, null);
  const { onConnectCloud } = mount(client, false);
  await vi.waitFor(() => expect(button('Dismiss')).toBeDefined());
  button('Dismiss')?.click();
  await vi.waitFor(() => expect(button('Dismiss')).toBeUndefined());
  button('Reset first-run AI setup')?.click();
  await vi.waitFor(async () =>
    expect((await client.getLlmDiscoveryStatus()).firstRunPending).toBe(true),
  );
  button('Detect local servers')?.click();
  await vi.waitFor(() => expect(button('Connect a cloud provider')).toBeDefined());
  button('Connect a cloud provider')?.click();
  await vi.waitFor(() => expect(onConnectCloud).toHaveBeenCalledOnce());
  expect(await client.listLlmProfiles()).toEqual([]);
  expect((await client.getLlmDiscoveryStatus()).firstRunPending).toBe(false);
});
