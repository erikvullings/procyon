import { describe, expect, it } from 'vitest';
import type { ImageBatch, Location } from '../../models';
import { isValidImageFileName } from '../../models';
import {
  canGenerate,
  destinationFor,
  emptyImagePane,
  failureNeedsSettings,
  firstImageId,
  hasPreview,
  imagePlacementPaneId,
  imageRequestProblem,
  imageSaveFolderPaneId,
  isSaveConflict,
  suggestedFileName,
} from './image-pane-state';

function pane(overrides: Partial<ReturnType<typeof emptyImagePane>> = {}) {
  return { ...emptyImagePane('pane-1:tab-1', 'pane-1', 'tab-1', 'workspace-1'), ...overrides };
}

const batch: ImageBatch = {
  generationId: 'generation-1',
  model: 'flux-schnell',
  prompt: 'a lighthouse at dusk',
  negativePrompt: null,
  seed: 42,
  width: 1_024,
  height: 1_024,
  images: [
    {
      id: 'image-1',
      mediaType: 'image/png',
      width: 1_024,
      height: 1_024,
      bytes: 136,
      downscaled: false,
      extension: 'png',
    },
    {
      id: 'image-2',
      mediaType: 'image/png',
      width: 1_024,
      height: 1_024,
      bytes: 136,
      downscaled: false,
      extension: 'png',
    },
  ],
};

describe('imageRequestProblem', () => {
  it('asks for a subject before anything else', () => {
    expect(imageRequestProblem(pane())).toBe('needSubject');
  });

  it('accepts a size and count the pane offers', () => {
    expect(
      imageRequestProblem(pane({ subject: 'a lighthouse', width: 832, height: 1_216, count: 4 })),
    ).toBeUndefined();
  });

  it('refuses a size the pane never offers', () => {
    expect(imageRequestProblem(pane({ subject: 'a lighthouse', width: 777, height: 777 }))).toBe(
      'sizeNotAllowed',
    );
  });

  it('refuses a count outside the bound', () => {
    expect(imageRequestProblem(pane({ subject: 'a lighthouse', count: 5 }))).toBe(
      'countNotAllowed',
    );
  });

  it('refuses a seed above the bound', () => {
    expect(imageRequestProblem(pane({ subject: 'a lighthouse', seed: 4_294_967_296 }))).toBe(
      'seedTooLarge',
    );
  });

  it('refuses a prompt longer than the backend accepts', () => {
    expect(imageRequestProblem(pane({ subject: 'a'.repeat(4_001) }))).toBe('promptTooLong');
  });

  it('refuses a negative prompt longer than the backend accepts', () => {
    expect(
      imageRequestProblem(pane({ subject: 'a lighthouse', negativePrompt: 'b'.repeat(2_001) })),
    ).toBe('negativeTooLong');
  });

  it('says nothing while a generation runs, so Stop stays available', () => {
    expect(
      imageRequestProblem(pane({ subject: '', pendingRequestId: 'request-1' })),
    ).toBeUndefined();
  });
});

describe('canGenerate', () => {
  it('is false without a subject', () => {
    expect(canGenerate(pane())).toBe(false);
  });

  it('is false while a generation is running', () => {
    expect(canGenerate(pane({ subject: 'a lighthouse', pendingRequestId: 'request-1' }))).toBe(
      false,
    );
  });

  it('is true for a complete request', () => {
    expect(canGenerate(pane({ subject: 'a lighthouse' }))).toBe(true);
  });
});

describe('suggestedFileName', () => {
  it('turns the prompt into a readable stem with the size, number and format', () => {
    expect(suggestedFileName('A Lighthouse at Dusk!', 0, 1_024, 1_024, 'png')).toBe(
      'a-lighthouse-at-dusk-1024x1024-1.png',
    );
  });

  it('numbers the images of a batch', () => {
    expect(suggestedFileName('a cat', 2, 832, 1_216, 'jpg')).toBe('a-cat-832x1216-3.jpg');
  });

  it('keeps a name when the prompt carries nothing usable', () => {
    expect(suggestedFileName('???', 0, 1_024, 1_024, 'png')).toBe('image-1024x1024-1.png');
  });

  it('keeps a long prompt inside a file name', () => {
    const name = suggestedFileName('very long prompt '.repeat(10), 0, 1_024, 1_024, 'png');
    expect(name.length).toBeLessThanOrEqual('1024x1024-1.png'.length + 41);
    expect(name.endsWith('-1024x1024-1.png')).toBe(true);
  });
});

describe('isValidImageFileName', () => {
  it('needs the format the image is stored in', () => {
    expect(isValidImageFileName('lighthouse.png', 'png')).toBe(true);
    expect(isValidImageFileName('lighthouse.PNG', 'png')).toBe(true);
    expect(isValidImageFileName('lighthouse.jpg', 'png')).toBe(false);
    expect(isValidImageFileName('lighthouse', 'png')).toBe(false);
  });

  it('refuses names that are not a single file name', () => {
    for (const name of ['', '   ', '.', '..', 'a/b.png', 'a\\b.png', 'a\u0000.png', 'a.png ']) {
      expect(isValidImageFileName(name, 'png'), JSON.stringify(name)).toBe(false);
    }
  });

  it('refuses a name no file system would take', () => {
    expect(isValidImageFileName(`${'a'.repeat(200)}.png`, 'png')).toBe(false);
  });
});

describe('destinationFor', () => {
  const folder: Location = { providerId: 'local', uri: 'file:///tmp/shots' };

  it('adds the file name to the chosen folder', () => {
    expect(destinationFor(folder, 'cat.png')).toEqual({
      providerId: 'local',
      uri: 'file:///tmp/shots/cat.png',
    });
  });

  it('does not double a separator', () => {
    expect(destinationFor({ ...folder, uri: 'file:///tmp/shots/' }, 'cat.png').uri).toBe(
      'file:///tmp/shots/cat.png',
    );
  });

  it('trims the name', () => {
    expect(destinationFor(folder, '  cat.png  ').uri).toBe('file:///tmp/shots/cat.png');
  });
});

describe('failures', () => {
  it('recognises a conflict as something to confirm, not to report', () => {
    expect(isSaveConflict('destinationAlreadyExists')).toBe(true);
    expect(isSaveConflict('invalidRequest')).toBe(false);
    expect(isSaveConflict(undefined)).toBe(false);
  });

  it('points role and consent failures at Settings', () => {
    expect(failureNeedsSettings('roleNotAssigned')).toBe(true);
    expect(failureNeedsSettings('cloudImageNotAllowed')).toBe(true);
    expect(failureNeedsSettings('imageGenerationFailed')).toBe(false);
  });
});

describe('preview state', () => {
  it('is empty before a batch arrives', () => {
    expect(hasPreview(pane())).toBe(false);
    expect(firstImageId(undefined)).toBeUndefined();
  });

  it('shows the batch and selects its first image', () => {
    const entry = pane({ batch, views: [] });
    expect(hasPreview(entry)).toBe(true);
    expect(firstImageId(batch)).toBe('image-1');
  });
});

describe('placement', () => {
  it('opens in the active pane when it shows no editor', () => {
    expect(imagePlacementPaneId(['pane-1', 'pane-2'], 'pane-2', false)).toBe('pane-2');
  });

  it('opens beside the editor when the active pane shows one', () => {
    expect(imagePlacementPaneId(['pane-1', 'pane-2'], 'pane-1', true)).toBe('pane-2');
  });

  it('takes the other pane as the save folder', () => {
    expect(imageSaveFolderPaneId(['pane-1', 'pane-2'], 'pane-1')).toBe('pane-2');
    expect(imageSaveFolderPaneId(['pane-1'], 'pane-1')).toBeUndefined();
  });
});
