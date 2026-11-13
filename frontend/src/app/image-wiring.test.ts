import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../api/client/file-manager-client';
import { emptyImagePane, type ImagePaneEntry } from '../features/image-generation/image-pane-state';
import { t } from '../i18n';
import type { GeneratedImageBytes, ImageBatch, SaveGeneratedImageResponse } from '../models';
import { createImageWiring, imageFailureKey, imageFailureReason } from './image-wiring';
import type { ShellState } from './shell-state';

const batch: ImageBatch = {
  generationId: 'g1',
  model: 'flux-schnell',
  prompt: 'a lighthouse at dusk, loose watercolour illustration',
  negativePrompt: 'text, watermark',
  seed: 42,
  width: 1_024,
  height: 1_024,
  images: [
    {
      id: 'i1',
      mediaType: 'image/png',
      width: 1_024,
      height: 1_024,
      bytes: 136,
      downscaled: false,
      extension: 'png',
    },
    {
      id: 'i2',
      mediaType: 'image/png',
      width: 1_024,
      height: 1_024,
      bytes: 136,
      downscaled: false,
      extension: 'png',
    },
  ],
};

function fakeShell(client: Partial<FileManagerClient>): {
  shell: ShellState;
  entry: ImagePaneEntry;
} {
  const entry = emptyImagePane('left:t1', 'left', 't1', 'w1');
  const shell = {
    client: client as FileManagerClient,
    imageByTab: new Map([[entry.key, entry]]),
    imageOpening: false,
    directories: new Map(),
  } as unknown as ShellState;
  return { shell, entry };
}

function imageClient(overrides: Partial<FileManagerClient> = {}): Partial<FileManagerClient> {
  return {
    generateImages: vi.fn(() => Promise.resolve(batch)),
    generatedImageBytes: vi.fn(
      (): Promise<GeneratedImageBytes> =>
        Promise.resolve({ bytes: new Uint8Array([1, 2, 3]), mediaType: 'image/png' }),
    ),
    cancelImageGeneration: vi.fn(() => Promise.resolve()),
    saveGeneratedImage: vi.fn(() =>
      Promise.resolve({
        location: { providerId: 'local', uri: 'file:///shots/lighthouse-1024x1024-1.png' },
        bytesWritten: 136,
        promptEmbedded: true,
      }),
    ),
    discardImageBatch: vi.fn(() => Promise.resolve()),
    ...overrides,
  };
}

beforeEach(() => {
  vi.stubGlobal('crypto', { randomUUID: () => 'req-1' });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('image wiring', () => {
  it('sends the composed prompt and holds the batch without writing anything', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a lighthouse at dusk';
    entry.presetId = 'watercolour';
    entry.negativePrompt = 'blur';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    expect(client.generateImages).toHaveBeenCalledWith({
      requestId: 'req-1',
      workspaceId: 'w1',
      prompt:
        'a lighthouse at dusk, loose watercolour illustration, soft washes, visible paper texture, gentle palette',
      negativePrompt: 'blur, photo, heavy outlines, text, watermark',
      width: 1_024,
      height: 1_024,
      count: 1,
      seed: null,
    });
    expect(entry.batch).toEqual(batch);
    expect(entry.pendingRequestId).toBeUndefined();
    // Nothing was saved: a save is the only path that writes, and it was never called.
    expect(client.saveGeneratedImage).not.toHaveBeenCalled();
  });

  it('asks for each stored image once and selects the first', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);
    await Promise.resolve();

    expect(client.generatedImageBytes).toHaveBeenCalledTimes(2);
    expect(client.generatedImageBytes).toHaveBeenCalledWith('g1', 'i1', false);
    expect(entry.selectedImageId).toBe('i1');
    expect(entry.views.map((view) => view.loading)).toEqual([false, false]);
  });

  it('stops the request in flight and keeps the prompt', async () => {
    let release: (() => void) | undefined;
    const client = imageClient({
      generateImages: () =>
        new Promise<ImageBatch>((resolve) => {
          release = () => resolve(batch);
        }),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    const running = wiring.generate(client as unknown as FileManagerClient, entry.key);
    expect(entry.pendingRequestId).toBe('req-1');

    wiring.stop(client as unknown as FileManagerClient, entry.key);

    expect(entry.pendingRequestId).toBeUndefined();
    expect(client.cancelImageGeneration).toHaveBeenCalledWith('req-1');
    expect(entry.subject).toBe('a fox');
    release?.();
    await running;
  });

  it('refuses to write over an existing file until the person asks for it', async () => {
    const conflict = Object.assign(new Error('exists'), { code: 'destinationAlreadyExists' });
    const client = imageClient({
      saveGeneratedImage: vi
        .fn<
          (
            request: Parameters<FileManagerClient['saveGeneratedImage']>[0],
            signal?: AbortSignal,
          ) => Promise<SaveGeneratedImageResponse>
        >()
        .mockRejectedValueOnce(conflict)
        .mockResolvedValue({
          location: { providerId: 'local', uri: 'file:///shots/a.png' },
          bytesWritten: 136,
          promptEmbedded: true,
        }),
    });
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    entry.views = [
      { imageId: 'i1', loading: false },
      { imageId: 'i2', loading: false },
    ];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });
    const folder = { providerId: 'local', uri: 'file:///shots' };

    await wiring.save(client as unknown as FileManagerClient, entry.key, {
      imageId: 'i1',
      fileName: 'a.png',
      folder,
      overwrite: false,
    });

    expect(entry.failure).toBe('destinationAlreadyExists');
    expect(entry.views[0]?.savedLocation).toBeUndefined();

    await wiring.save(client as unknown as FileManagerClient, entry.key, {
      imageId: 'i1',
      fileName: 'a.png',
      folder,
      overwrite: true,
    });

    expect(client.saveGeneratedImage).toHaveBeenLastCalledWith(
      expect.objectContaining({ overwrite: true }),
    );
    expect(entry.failure).toBeUndefined();
    expect(entry.views[0]?.savedLocation).toBe('file:///shots/a.png');
  });

  it('drops a batch without saving and tells the backend to forget it', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    entry.views = [{ imageId: 'i1', loading: false, previewUrl: 'blob:x' }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.discard(client as unknown as FileManagerClient, entry.key);

    expect(entry.batch).toBeUndefined();
    expect(entry.views).toEqual([]);
    expect(client.discardImageBatch).toHaveBeenCalledWith('g1');
    expect(client.saveGeneratedImage).not.toHaveBeenCalled();
  });

  it('regenerates from the prompt that produced the batch', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.regenerate(client as unknown as FileManagerClient, entry.key);

    const sent = (client.generateImages as ReturnType<typeof vi.fn>).mock.calls[0]?.[0] as {
      prompt: string;
      negativePrompt: string | null;
    };
    expect(sent.prompt).toBe(batch.prompt);
    expect(sent.negativePrompt).toBe(batch.negativePrompt);
  });

  it('stops a running generation and drops its batch when the pane closes', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    entry.pendingRequestId = 'req-9';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.forget(client as unknown as FileManagerClient, entry.key);

    expect(shell.imageByTab.has(entry.key)).toBe(false);
    expect(client.cancelImageGeneration).toHaveBeenCalledWith('req-9');
    expect(client.discardImageBatch).toHaveBeenCalledWith('g1');
  });

  it('explains a refusal without repeating the endpoint', async () => {
    const refused = Object.assign(new Error('cloud'), { code: 'cloudImageNotAllowed' });
    const client = imageClient({
      generateImages: vi.fn(() => Promise.reject(refused)),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    expect(entry.failure).toBe('cloudImageNotAllowed');
    expect(entry.error).toBe(
      'This image endpoint is in the cloud. Allow cloud use in Settings first.',
    );
  });
});

describe('failure mapping', () => {
  it('reads the reason the backend reported', () => {
    expect(imageFailureReason(Object.assign(new Error('x'), { reason: 'imageBatchExpired' }))).toBe(
      'imageBatchExpired',
    );
    expect(imageFailureReason(Object.assign(new Error('x'), { code: 'invalidRequest' }))).toBe(
      'invalidRequest',
    );
    expect(imageFailureReason({ status: 409 })).toBe('destinationAlreadyExists');
    expect(imageFailureReason({ status: 403 })).toBe('cloudImageNotAllowed');
    expect(imageFailureReason('nope')).toBe('failed');
  });

  it('has a message for every reason a generation or save can fail with', () => {
    const reasons = [
      'roleNotAssigned',
      'cloudImageNotAllowed',
      'llmCloudImageNotAllowed',
      'invalidRequest',
      'notFound',
      'imageBatchExpired',
      'imageTooLarge',
      'destinationAlreadyExists',
      'cancelled',
      'imageGenerationFailed',
      'anything-else',
    ];
    for (const reason of reasons) {
      const key = imageFailureKey(reason);
      const text = t('image', key);
      // Every reason ends in a sentence the person can act on, never a raw code.
      expect(text, reason).toBeTruthy();
      expect(text, reason).not.toContain(reason);
      expect(text, reason).not.toBe(key);
    }
  });
});
