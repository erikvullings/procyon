import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ImageMetadataEdit } from '../../models';
import {
  EditImageMetadataDialog,
  type EditImageMetadataRequest,
} from './edit-image-metadata-dialog';

const request: EditImageMetadataRequest = {
  files: [{ providerId: 'local', uri: 'file:///a.jpg' }],
  richFields: true,
};

function renderDialog(
  root: HTMLElement,
  current: EditImageMetadataRequest | undefined,
  onApply: (edit: ImageMetadataEdit) => void,
): void {
  m.render(
    root,
    m(EditImageMetadataDialog, { request: current, onApply, onCancel: () => undefined }),
  );
}

function mountDialog(
  current: EditImageMetadataRequest | undefined,
  onApply: (edit: ImageMetadataEdit) => void,
): HTMLElement {
  const root = document.createElement('div');
  document.body.append(root);
  renderDialog(root, current, onApply);
  return root;
}

function applyButton(root: HTMLElement): HTMLButtonElement | undefined {
  return [...root.querySelectorAll('button')].find((button) =>
    button.textContent?.includes('Apply'),
  );
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('EditImageMetadataDialog', () => {
  it('disables Apply until something would change', () => {
    const root = mountDialog(request, vi.fn());
    expect(applyButton(root)?.disabled).toBe(true);
  });

  it('applies the typed description with the backup choice', () => {
    const onApply = vi.fn();
    const root = mountDialog(request, onApply);
    const description = root.querySelectorAll<HTMLInputElement>('input[type="text"]')[2];
    if (description === undefined) throw new Error('description input missing');
    description.value = 'Sunset';
    description.dispatchEvent(new Event('input'));
    renderDialog(root, request, onApply);
    applyButton(root)?.click();
    expect(onApply).toHaveBeenCalledWith({ backupOriginal: true, description: 'Sunset' });
  });

  it('disables title, keywords and rating when not every file is a JPEG', () => {
    const root = mountDialog({ ...request, richFields: false }, vi.fn());
    expect(root.textContent).toContain('only stored in JPEG');
    expect(root.querySelectorAll('.fm-image-edit-row.is-disabled')).toHaveLength(3);
  });
});
