import m from 'mithril';
import { afterEach, expect, it, vi } from 'vitest';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import type { Location } from '../../models/location';
import { WatchedFoldersSettings } from './watched-folders-settings';

const root = document.createElement('div');

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function click(text: string) {
  const button = [...root.querySelectorAll('button')].find((item) => item.textContent === text);
  expect(button).toBeDefined();
  button?.click();
  m.redraw.sync();
}

it('shows an editable 48-hour delay and persists pause through the mock client', async () => {
  const client = new MockFileManagerClient();
  const location: Location = { providerId: 'local', uri: 'file:///Downloads' };
  document.body.append(root);
  m.mount(root, { view: () => m(WatchedFoldersSettings, { client, activeLocation: location }) });
  await vi.waitFor(() => expect(root.querySelector('button')?.disabled).toBe(false));
  click('Use active folder');
  const delay = root.querySelector<HTMLInputElement>('input[type=number]');
  expect(delay?.value).toBe('48');
  if (delay) {
    delay.value = '24';
    delay.dispatchEvent(new Event('input', { bubbles: true }));
  }
  root.querySelector<HTMLInputElement>('input[type=checkbox]')?.click();
  click('Save watched folder');
  await vi.waitFor(async () => {
    const { folders } = await client.manageWatchedFolders({ action: 'list' });
    expect(folders[0]).toMatchObject({ root: location, delayHours: 24, paused: true });
  });
});

it('queues only the reviewed assignment with the edited destination', async () => {
  const client = new MockFileManagerClient();
  const location: Location = { providerId: 'mock', uri: 'mock:///Documents' };
  document.body.append(root);
  m.mount(root, { view: () => m(WatchedFoldersSettings, { client, activeLocation: location }) });
  await vi.waitFor(() => expect(root.querySelector('button')?.disabled).toBe(false));
  click('Use active folder');
  click('Suggest a folder structure');
  await vi.waitFor(() => expect(root.textContent).toContain('report.pdf'), { timeout: 3000 });
  const destination = [...root.querySelectorAll('label')]
    .find((label) => label.textContent === 'Destination folder')
    ?.querySelector('input');
  expect(destination).toBeDefined();
  if (destination) {
    destination.value = 'Reports';
    destination.dispatchEvent(new Event('input', { bubbles: true }));
  }
  expect((await client.manageWatchedFolders({ action: 'list' })).folders).toEqual([]);
  click('Approve moves and save');
  await vi.waitFor(async () => {
    const { folders } = await client.manageWatchedFolders({ action: 'list' });
    expect(folders[0]?.pending).toMatchObject([
      { assignment: { folder: 'Reports', source: { uri: 'mock:///Documents/report.pdf' } } },
    ]);
  });
});
