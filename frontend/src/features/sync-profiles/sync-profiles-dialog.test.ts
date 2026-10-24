import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Location, SyncProfile } from '../../models';
import { newSyncProfile } from './sync-profile';
import type { SyncProfileController } from './sync-profile-controller';
import { SyncProfilesDialog } from './sync-profiles-dialog';

let root: HTMLElement;
beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});
afterEach(() => {
  m.mount(root, null);
  root.remove();
});

const left: Location = { providerId: 'local', uri: 'file:///a' };
const right: Location = { providerId: 'sftp', uri: 'sftp://me@nas/b' };

function button(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll('button')].find((b) =>
    b.textContent?.includes(label),
  );
  if (found === undefined) throw new Error(`no button "${label}"`);
  return found;
}

function controller(profiles: readonly SyncProfile[]): SyncProfileController & {
  run: ReturnType<typeof vi.fn>;
  duplicate: ReturnType<typeof vi.fn>;
  remove: ReturnType<typeof vi.fn>;
} {
  return {
    profiles: () => profiles,
    isRunning: () => false,
    pendingReview: () => undefined,
    save: vi.fn(() => Promise.resolve([])),
    duplicate: vi.fn(() => Promise.resolve()),
    remove: vi.fn(() => Promise.resolve()),
    run: vi.fn(() => Promise.resolve()),
    openReview: vi.fn(),
    confirmReview: vi.fn(() => Promise.resolve()),
    dismissReview: vi.fn(() => Promise.resolve()),
    tick: vi.fn(() => Promise.resolve()),
  };
}

function mount(c: SyncProfileController): void {
  m.mount(root, {
    view: () =>
      m(SyncProfilesDialog, {
        open: true,
        controller: c,
        paneLocations: () => ({ left, right }),
        onClose: vi.fn(),
      }),
  });
  m.redraw.sync();
}

describe('SyncProfilesDialog', () => {
  it('lists profiles with their roots and last run, and offers run, duplicate and delete', () => {
    const profile = newSyncProfile('1', 'Photos', left, right);
    const c = controller([profile]);
    mount(c);

    const row = document.querySelector('.fm-sync-profile-row');
    expect(row?.textContent).toContain('Photos');
    expect(row?.textContent).toContain('file:///a');
    expect(row?.textContent).toContain('sftp://me@nas/b');
    expect(row?.textContent).toContain('Never run.');

    button('Run').click();
    expect(c.run).toHaveBeenCalledWith('1');
    button('Duplicate').click();
    expect(c.duplicate).toHaveBeenCalledWith('1');
    // Deleting needs a second, explicit confirmation click.
    button('Delete').click();
    expect(c.remove).not.toHaveBeenCalled();
    m.redraw.sync();
    button('Confirm delete').click();
    expect(c.remove).toHaveBeenCalledWith('1');
  });

  it('creates a profile from the pane folders and saves the parsed filters and schedule', () => {
    const c = controller([]);
    mount(c);
    expect(document.body.textContent).toContain('No profiles yet');

    button('New profile').click();
    m.redraw.sync();
    const name = document.querySelector<HTMLInputElement>(
      '.fm-sync-profile-editor input[type=text]',
    );
    expect(name).not.toBeNull();
    if (name === null) return;
    name.value = 'Docs';
    name.dispatchEvent(new Event('input'));
    const [include, exclude] = [...document.querySelectorAll<HTMLTextAreaElement>('textarea')];
    if (include === undefined || exclude === undefined) throw new Error('missing pattern fields');
    include.value = '*.md\n*.txt';
    include.dispatchEvent(new Event('input'));
    exclude.value = 'node_modules';
    exclude.dispatchEvent(new Event('input'));
    document
      .querySelector<HTMLInputElement>('.fm-sync-profile-schedule input[type=checkbox]')
      ?.click();
    m.redraw.sync();

    button('Save').click();
    const saved = vi.mocked(c.save).mock.calls[0]?.[0];
    expect(saved).toMatchObject({
      name: 'Docs',
      left,
      right,
      include: ['*.md', '*.txt'],
      exclude: ['node_modules'],
      mode: 'twoWayUpdate',
      schedule: { enabled: true, confirmation: 'required' },
    });
  });

  it('states that schedules only run while the app is open', () => {
    mount(controller([]));
    button('New profile').click();
    m.redraw.sync();
    document
      .querySelector<HTMLInputElement>('.fm-sync-profile-schedule input[type=checkbox]')
      ?.click();
    m.redraw.sync();
    expect(document.body.textContent).toContain('only runs while Procyon is open');
  });
});
