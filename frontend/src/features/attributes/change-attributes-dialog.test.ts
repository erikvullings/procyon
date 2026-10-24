import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AttributeChangePreview,
  ChangeAttributes,
  InspectAttributesResult,
  Location,
} from '../../models';
import { ChangeAttributesDialog, type ChangeAttributesRequest } from './change-attributes-dialog';

const FILE: Location = { providerId: 'local', uri: 'file:///tmp/a.txt' };
const FOLDER: Location = { providerId: 'local', uri: 'file:///tmp/dir' };

const unix = (kind: 'file' | 'directory' = 'file'): InspectAttributesResult => ({
  support: { mode: true, ownership: true, windowsAttributes: false },
  entries: [
    {
      location: kind === 'file' ? FILE : FOLDER,
      kind,
      attributes: { mode: 0o644, uid: 501, gid: 20 },
    },
  ],
});

const PREVIEW: AttributeChangePreview = {
  total: 7,
  files: 4,
  folders: 3,
  skippedSymlinks: 2,
  truncated: false,
};

function client(inspect: InspectAttributesResult, preview: AttributeChangePreview = PREVIEW) {
  return {
    inspectAttributes: vi.fn().mockResolvedValue(inspect),
    previewAttributeChange: vi.fn().mockResolvedValue(preview),
  };
}

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function mount(
  request: ChangeAttributesRequest | undefined,
  fake: ReturnType<typeof client>,
  onApply: (change: ChangeAttributes, count: number) => void = vi.fn(),
): { root: HTMLElement; render: () => void } {
  const root = document.createElement('div');
  document.body.append(root);
  const render = () =>
    m.render(
      root,
      m(ChangeAttributesDialog, { request, client: fake, onApply, onCancel: vi.fn() }),
    );
  render();
  return { root, render };
}

async function opened(
  request: ChangeAttributesRequest,
  fake: ReturnType<typeof client>,
  onApply?: (change: ChangeAttributes, count: number) => void,
) {
  const view = mount(undefined, fake, onApply);
  const root = view.root;
  const render = () =>
    m.render(
      root,
      m(ChangeAttributesDialog, {
        request,
        client: fake,
        onApply: onApply ?? vi.fn(),
        onCancel: vi.fn(),
      }),
    );
  render();
  render();
  await settle();
  render();
  await settle();
  render();
  return { root, render };
}

const applyButton = (root: HTMLElement) =>
  [...root.querySelectorAll('button')].find((button) => button.textContent?.includes('Apply'));

const checkbox = (root: HTMLElement, label: string) =>
  root.querySelector<HTMLInputElement>(`label.fm-attr-check input[aria-label="${label}"]`);

afterEach(() => {
  document.body.replaceChildren();
});

describe('ChangeAttributesDialog', () => {
  it('loads the current state and keeps Apply disabled until something changes', async () => {
    const fake = client(unix());
    const { root } = await opened({ files: [FILE] }, fake);
    expect(fake.inspectAttributes).toHaveBeenCalledWith([FILE]);
    expect(root.textContent).toContain('1 item selected.');
    expect(root.querySelector<HTMLInputElement>('input[maxlength="4"]')?.value).toBe('0644');
    expect(applyButton(root)?.disabled).toBe(true);
  });

  it('sends only the touched bits with their mask', async () => {
    const onApply = vi.fn();
    const { root, render } = await opened({ files: [FILE] }, client(unix()), onApply);
    const groupWrite = checkbox(root, 'Group Write');
    if (groupWrite === null) throw new Error('group write checkbox missing');
    groupWrite.click();
    render();
    expect(applyButton(root)?.disabled).toBe(false);
    applyButton(root)?.click();
    expect(onApply).toHaveBeenCalledWith(
      { change: { mode: 0o020, modeMask: 0o020 }, recursive: false, scope: 'both' },
      expect.any(Number),
    );
  });

  it('offers recursion only for folders and previews the affected count', async () => {
    const fake = client(unix('directory'));
    const { root, render } = await opened({ files: [FOLDER] }, fake);
    expect(fake.previewAttributeChange).toHaveBeenCalledWith([FOLDER], false, 'both');
    const recursive = [...root.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')].find(
      (input) => input.closest('label')?.textContent?.includes('recursively'),
    );
    if (recursive === undefined) throw new Error('recursive checkbox missing');
    recursive.click();
    render();
    await settle();
    render();
    expect(fake.previewAttributeChange).toHaveBeenLastCalledWith([FOLDER], true, 'both');
    expect(root.textContent).toContain('7 items will be changed (4 files, 3 folders).');
    expect(root.textContent).toContain('2 symbolic links are left unchanged.');
  });

  it('does not offer recursion for plain files', async () => {
    const { root } = await opened({ files: [FILE] }, client(unix()));
    expect(root.textContent).not.toContain('recursively');
  });

  it('shows the reason and no editor when the provider cannot change permissions', async () => {
    const fake = client({
      support: { mode: false, ownership: false, windowsAttributes: false },
      unsupportedReason: 'providerUnsupported',
      entries: [{ location: FILE, kind: 'file' }],
    });
    const { root } = await opened({ files: [FILE] }, fake);
    expect(root.textContent).toContain('does not support changing permissions');
    expect(root.querySelector('table')).toBeNull();
    expect(applyButton(root)?.disabled).toBe(true);
  });

  it('shows only Windows attributes on a platform that has just those', async () => {
    const fake = client({
      support: { mode: false, ownership: false, windowsAttributes: true },
      entries: [
        {
          location: FILE,
          kind: 'file',
          attributes: { readOnly: false, hidden: false, archive: true },
        },
      ],
    });
    const onApply = vi.fn();
    const { root, render } = await opened({ files: [FILE] }, fake, onApply);
    expect(root.querySelector('table')).toBeNull();
    checkbox(root, 'Hidden')?.click();
    render();
    applyButton(root)?.click();
    expect(onApply).toHaveBeenCalledWith(
      { change: { hidden: true }, recursive: false, scope: 'both' },
      expect.any(Number),
    );
  });

  it('reports a failure to read the current state', async () => {
    const fake = client(unix());
    fake.inspectAttributes.mockRejectedValue(new Error('boom'));
    const { root } = await opened({ files: [FILE] }, fake);
    expect(root.textContent).toContain('Could not read the current permissions: boom');
  });

  it('blocks Apply while the owner is not a whole number', async () => {
    const { root, render } = await opened({ files: [FILE] }, client(unix()));
    const owner = [...root.querySelectorAll<HTMLInputElement>('input[type="text"]')].find(
      (input) => input.placeholder === 'Now 501',
    );
    if (owner === undefined) throw new Error('owner input missing');
    owner.value = 'root';
    owner.dispatchEvent(new Event('input'));
    render();
    expect(root.textContent).toContain('The owner must be a whole number.');
    expect(applyButton(root)?.disabled).toBe(true);
  });
});
