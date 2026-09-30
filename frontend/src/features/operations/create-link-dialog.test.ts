import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { LinkKindOption, LinkOptions, Location } from '../../models';
import { CreateLinkDialog, linkNameStemEnd, linkRequestFor } from './create-link-dialog';

let root: HTMLElement;

const target: Location = { providerId: 'local', uri: 'file:///a/r%C3%A9sum%C3%A9.txt' };
const destination: Location = { providerId: 'local', uri: 'file:///b' };

const symlink: LinkKindOption = {
  kind: 'symbolicLink',
  supportsRelative: true,
  requirements: ['developerModeOrAdministrator'],
  suggestedName: 'résumé.txt',
};
const shortcut: LinkKindOption = {
  kind: 'shortcut',
  supportsRelative: false,
  requirements: ['shellOnly'],
  suggestedName: 'résumé.txt.lnk',
};

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

async function mountDialog(options: LinkOptions) {
  const onConfirm = vi.fn();
  const loadOptions = vi.fn().mockResolvedValue(options);
  // A stable request identity: the dialog reloads its options whenever the request changes.
  const request = { target, destination };
  m.mount(root, {
    view: () =>
      m(CreateLinkDialog, {
        request,
        loadOptions,
        onConfirm,
        onCancel: vi.fn(),
      }),
  });
  m.redraw.sync();
  await Promise.resolve();
  await Promise.resolve();
  m.redraw.sync();
  return { onConfirm, loadOptions };
}

function select(id: string, value: string): void {
  const element = document.querySelector<HTMLSelectElement>(id);
  if (!element) throw new Error(`${id} missing`);
  element.value = value;
  element.dispatchEvent(new Event('change', { bubbles: true }));
  m.redraw.sync();
}

function pressEnter(): void {
  document
    .querySelector<HTMLInputElement>('#create-link-name')
    ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
}

describe('linkRequestFor', () => {
  it('keeps the chosen style only for kinds that support relative targets', () => {
    expect(linkRequestFor(symlink, 'relative')).toEqual({
      kind: 'symbolicLink',
      targetStyle: 'relative',
    });
    expect(linkRequestFor(shortcut, 'relative')).toEqual({
      kind: 'shortcut',
      targetStyle: 'absolute',
    });
  });
});

describe('linkNameStemEnd', () => {
  it('selects the name without its extension or shortcut suffix', () => {
    expect(linkNameStemEnd('résumé.txt')).toBe(6);
    expect(linkNameStemEnd('data.json.LNK')).toBe(4);
    expect(linkNameStemEnd('archive.tar.gz')).toBe(11);
    expect(linkNameStemEnd('Makefile')).toBe(8);
    expect(linkNameStemEnd('.bashrc')).toBe(7);
  });
});

describe('CreateLinkDialog', () => {
  it('shows its choices in visible native selects and preselects the name stem', async () => {
    const frames: FrameRequestCallback[] = [];
    const frame = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      frames.push(callback);
      return frames.length;
    });
    await mountDialog({ kinds: [symlink, shortcut] });
    frame.mockRestore();
    for (const callback of frames) callback(0);
    for (const id of ['#create-link-kind', '#create-link-target-style']) {
      const element = document.querySelector<HTMLSelectElement>(id);
      expect(element?.classList.contains('browser-default')).toBe(true);
      expect(element?.selectedOptions[0]?.textContent).not.toBe('');
    }
    const input = document.querySelector<HTMLInputElement>('#create-link-name');
    expect(document.activeElement).toBe(input);
    expect([input?.selectionStart, input?.selectionEnd]).toEqual([0, 6]);
  });

  it('loads options, suggests the name and explains requirements before confirming', async () => {
    const { onConfirm, loadOptions } = await mountDialog({ kinds: [symlink, shortcut] });
    expect(loadOptions).toHaveBeenCalledWith({ target, destination });
    const input = document.querySelector<HTMLInputElement>('#create-link-name');
    expect(input?.value).toBe('résumé.txt');
    expect(root.textContent).toContain('Developer Mode');
    expect(document.querySelector<HTMLSelectElement>('#create-link-target-style')?.value).toBe(
      'absolute',
    );
    select('#create-link-target-style', 'relative');
    pressEnter();
    expect(onConfirm).toHaveBeenCalledWith('résumé.txt', {
      kind: 'symbolicLink',
      targetStyle: 'relative',
    });
  });

  it('switching to a shortcut updates the suggested name and hides the style choice', async () => {
    const { onConfirm } = await mountDialog({ kinds: [symlink, shortcut] });
    select('#create-link-kind', 'shortcut');
    expect(document.querySelector('#create-link-target-style')).toBeNull();
    expect(document.querySelector<HTMLInputElement>('#create-link-name')?.value).toBe(
      'résumé.txt.lnk',
    );
    expect(root.textContent).toContain('Windows Explorer');
    pressEnter();
    expect(onConfirm).toHaveBeenCalledWith('résumé.txt.lnk', {
      kind: 'shortcut',
      targetStyle: 'absolute',
    });
  });

  it('names the only available kind in the title instead of offering a choice', async () => {
    const { onConfirm } = await mountDialog({ kinds: [symlink] });
    expect(document.querySelector('#create-link-kind')).toBeNull();
    expect(document.body.textContent).toContain('Create symbolic link');
    pressEnter();
    expect(onConfirm).toHaveBeenCalledWith('résumé.txt', {
      kind: 'symbolicLink',
      targetStyle: 'absolute',
    });
  });

  it('explains when no link kind is available', async () => {
    const { onConfirm } = await mountDialog({ kinds: [] });
    expect(root.textContent).toContain('Links cannot be created between these locations.');
    expect(document.querySelector('#create-link-name')).toBeNull();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
