import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../api/client/file-manager-client';
import {
  emptyImagePane,
  failureNeedsSettings,
  type ImagePaneEntry,
} from '../features/image-generation/image-pane-state';
import { t } from '../i18n';
import type {
  GeneratedImageBytes,
  ImageBatch,
  ImagePrompt,
  Location,
  SaveGeneratedImageResponse,
} from '../models';
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
    imageGenerationAvailable: false,
    imageGenerationChecked: false,
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
    listImagePrompts: vi.fn(() => Promise.resolve([])),
    saveImagePrompt: vi.fn((prompt: ImagePrompt) => Promise.resolve(prompt)),
    deleteImagePrompt: vi.fn(() => Promise.resolve()),
    ...overrides,
  };
}

let objectUrls = 0;
const originalCreate = URL.createObjectURL;
const originalRevoke = URL.revokeObjectURL;

/** Lets the fired-off byte fetches run to their end. */
async function settle(): Promise<void> {
  for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
}

/**
 * A refusal as the backend serialises it: one shared `invalidRequest` code, and the reason the
 * person is told about tucked into `details` (task 0325).
 */
function rejection(reason: string): Error {
  return Object.assign(new Error(`refused: ${reason}`), {
    code: 'invalidRequest',
    message: 'the image endpoint refused the request',
    requestId: 'req-1',
    details: { reason },
  });
}

beforeEach(() => {
  vi.stubGlobal('crypto', { randomUUID: () => 'req-1' });
  // The preview holds bytes behind an object URL; a stand-in name is enough to follow it.
  objectUrls = 0;
  URL.createObjectURL = (() => {
    objectUrls += 1;
    return `blob:${objectUrls}`;
  }) as unknown as typeof URL.createObjectURL;
  URL.revokeObjectURL = vi.fn() as unknown as typeof URL.revokeObjectURL;
});

afterEach(() => {
  vi.unstubAllGlobals();
  URL.createObjectURL = originalCreate;
  URL.revokeObjectURL = originalRevoke;
});

describe('image wiring', () => {
  it('remembers the form with every setting once a generation produced pictures', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = '  a fox  ';
    entry.negativePrompt = 'blur';
    entry.presetIds = ['watercolour'];
    entry.width = 768;
    entry.height = 512;
    entry.count = 2;
    entry.seed = 9;
    entry.qualityId = 'good';
    entry.steps = 20;
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);
    await settle();

    expect(client.saveImagePrompt).toHaveBeenCalledWith(
      expect.objectContaining({
        subject: 'a fox',
        negativePrompt: 'blur',
        presetIds: ['watercolour'],
        width: 768,
        height: 512,
        count: 2,
        seed: 9,
        quality: 'good',
        steps: 20,
      }),
    );
    expect(client.listImagePrompts).toHaveBeenCalled();
  });

  it('remembers nothing when the generation failed', async () => {
    const client = imageClient({
      generateImages: () => Promise.reject(rejection('cloudImageNotAllowed')),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);
    await settle();

    expect(client.saveImagePrompt).not.toHaveBeenCalled();
  });

  it('still shows the pictures when the history cannot be written', async () => {
    const client = imageClient({ saveImagePrompt: () => Promise.reject(new Error('disk full')) });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);
    await settle();

    expect(entry.batch).toEqual(batch);
    expect(entry.error).toBeUndefined();
  });

  it('puts a remembered prompt back with all its settings, and reloads after forgetting one', async () => {
    const stored: ImagePrompt = {
      id: 'p1',
      title: 'A fox',
      updatedAt: '2026-11-14T10:00:00.000Z',
      subject: 'a fox',
      negativePrompt: 'blur',
      presetIds: ['watercolour'],
      lensId: 'l1',
      apertureId: 'a1',
      width: 768,
      height: 512,
      count: 3,
      seed: 5,
      quality: 'quality',
      steps: 25,
    };
    const client = imageClient({ listImagePrompts: vi.fn(() => Promise.resolve([stored])) });
    const { shell, entry } = fakeShell(client);
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.applyPrompt(entry.key, stored);
    expect(entry).toMatchObject({
      subject: 'a fox',
      negativePrompt: 'blur',
      presetIds: ['watercolour'],
      lensId: 'l1',
      apertureId: 'a1',
      width: 768,
      height: 512,
      count: 3,
      seed: 5,
      qualityId: 'quality',
      steps: 25,
    });

    wiring.deletePrompt(client as unknown as FileManagerClient, 'p1');
    await settle();
    expect(client.deleteImagePrompt).toHaveBeenCalledWith('p1');
    expect(entry.prompts).toEqual([stored]);
  });

  it('sends the composed prompt and holds the batch without writing anything', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a lighthouse at dusk';
    entry.presetIds = ['watercolour'];
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

  it('writes the file the person named, not the folder it belongs to', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    entry.views = [{ imageId: 'i1', loading: false }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.save(client as unknown as FileManagerClient, entry.key, {
      imageId: 'i1',
      fileName: 'a fox.png',
      folder: { providerId: 'local', uri: 'file:///shots/' },
      overwrite: false,
    });

    // A folder as destination names no file: the backend refuses it, and every save fails.
    expect(client.saveGeneratedImage).toHaveBeenCalledWith(
      expect.objectContaining({
        generationId: 'g1',
        imageId: 'i1',
        destination: { providerId: 'local', uri: 'file:///shots/a fox.png' },
        overwrite: false,
      }),
    );
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

    expect(entry.failure).toBe('destinationExists');
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

  it('blames the destination rather than the prompt when a save is refused as invalid', async () => {
    const refusal = Object.assign(new Error('bad destination'), { code: 'invalidRequest' });
    const client = imageClient({
      saveGeneratedImage: vi
        .fn<
          (
            request: Parameters<FileManagerClient['saveGeneratedImage']>[0],
            signal?: AbortSignal,
          ) => Promise<SaveGeneratedImageResponse>
        >()
        .mockRejectedValueOnce(refusal),
    });
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    entry.views = [{ imageId: 'i1', loading: false }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.save(client as unknown as FileManagerClient, entry.key, {
      imageId: 'i1',
      fileName: 'a.png',
      folder: { providerId: 'local', uri: '/shots' },
      overwrite: false,
    });

    expect(entry.failure).toBe('destinationInvalid');
    expect(entry.error).toBe(t('image', 'destinationInvalid'));
    // The model already took the prompt, so sending the person back to it would be a lie.
    expect(entry.error).not.toBe(t('image', 'invalidRequest'));
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
    const refused = rejection('llmCloudImageNotAllowed');
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

  it('does not blame the prompt when the endpoint never answered', async () => {
    const client = imageClient({
      generateImages: vi.fn(() => Promise.reject(rejection('imageGenerationFailed'))),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    expect(entry.failure).toBe('generationFailed');
    expect(entry.error).toBe('The image model could not answer. Check the endpoint in Settings.');
    expect(entry.error).not.toContain('prompt');
  });

  it('says a slow model needs more time, and offers the setting that governs it', async () => {
    const client = imageClient({
      generateImages: vi.fn(() => Promise.reject(rejection('llmTimeout'))),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    expect(entry.failure).toBe('timedOut');
    expect(entry.error).toContain('ran out of time');
    // A timeout is worth a route out of, not just a sentence: Settings holds the timeout.
    expect(failureNeedsSettings(entry.failure)).toBe(true);
  });

  it('points at Settings when the endpoint itself is set up wrong', async () => {
    const client = imageClient({
      generateImages: vi.fn(() => Promise.reject(rejection('llmProfileInvalid'))),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    expect(entry.failure).toBe('endpointInvalid');
    expect(entry.error).toBe('That image endpoint is not set up right. Check it in Settings.');
    expect(failureNeedsSettings(entry.failure)).toBe(true);
  });

  it('names the reason a stopped generation ended on', async () => {
    const client = imageClient({
      generateImages: vi.fn(() =>
        Promise.reject(Object.assign(new Error('stopped'), { code: 'operationCancelled' })),
      ),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    expect(entry.failure).toBe('cancelled');
    expect(entry.error).toBe('The generation was stopped.');
  });
});

describe('failure mapping', () => {
  it('reads the reason the backend reported', () => {
    // Every rejection arrives under the shared `invalidRequest` code with its stable reason in
    // `details`: reading the code alone would blame the prompt for everything the endpoint refused.
    expect(imageFailureReason(rejection('imageBatchExpired'))).toBe('imageBatchExpired');
    expect(imageFailureReason(rejection('imageGenerationFailed'))).toBe('imageGenerationFailed');
    expect(imageFailureReason(rejection('llmTimeout'))).toBe('llmTimeout');
    expect(imageFailureReason(Object.assign(new Error('x'), { reason: 'imageBatchExpired' }))).toBe(
      'imageBatchExpired',
    );
    expect(imageFailureReason(Object.assign(new Error('x'), { code: 'invalidRequest' }))).toBe(
      'invalidRequest',
    );
    expect(imageFailureReason(Object.assign(new Error('x'), { code: 'operationCancelled' }))).toBe(
      'operationCancelled',
    );
    expect(imageFailureReason({ status: 409 })).toBe('destinationAlreadyExists');
    expect(imageFailureReason({ status: 403 })).toBe('cloudImageNotAllowed');
    expect(imageFailureReason('nope')).toBe('failed');
  });

  it('has a message for every reason a generation or save can fail with', () => {
    const reasons = [
      'roleNotAssigned',
      'llmRoleNotAssigned',
      'cloudImageNotAllowed',
      'llmCloudImageNotAllowed',
      'cloudConsentRequired',
      'llmProfileInvalid',
      'permissionDenied',
      'providerUnavailable',
      'operationCancelled',
      'invalidRequest',
      'notFound',
      'imageBatchExpired',
      'imageTooLarge',
      'destinationAlreadyExists',
      'cancelled',
      'imageGenerationFailed',
      'llmTimeout',
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

describe('the crumb bar of previous generations', () => {
  /** A second batch, so a pane can be holding one generation while asked for another. */
  const otherBatch: ImageBatch = { ...batch, generationId: 'g2', prompt: 'a fox in a boat' };

  it('keeps the generation it replaces as a crumb', async () => {
    const client = imageClient({
      generateImages: vi
        .fn<() => Promise<ImageBatch>>()
        .mockResolvedValueOnce(batch)
        .mockResolvedValueOnce(otherBatch),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });
    const clientAsClient = client as unknown as FileManagerClient;

    await wiring.generate(clientAsClient, entry.key);
    await settle();
    await wiring.generate(clientAsClient, entry.key);
    await settle();

    expect(entry.history.map((record) => record.generationId)).toEqual(['g1', 'g2']);
    expect(entry.batch?.generationId).toBe('g2');
    expect(entry.activeGenerationId).toBe('g2');
  });

  it('shows an earlier generation again without asking for its pictures twice', async () => {
    const client = imageClient({
      generateImages: vi
        .fn<() => Promise<ImageBatch>>()
        .mockResolvedValueOnce(batch)
        .mockResolvedValueOnce(otherBatch),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });
    const clientAsClient = client as unknown as FileManagerClient;
    await wiring.generate(clientAsClient, entry.key);
    await settle();
    await wiring.generate(clientAsClient, entry.key);
    await settle();
    const fetched = (client.generatedImageBytes as ReturnType<typeof vi.fn>).mock.calls.length;

    wiring.selectGeneration(clientAsClient, entry.key, 'g1');
    await Promise.resolve();

    expect(entry.batch?.generationId).toBe('g1');
    expect(entry.activeGenerationId).toBe('g1');
    expect(entry.views[0]?.previewUrl).toBe('blob:1');
    // Its pictures were already fetched, so going back costs nothing.
    expect(client.generatedImageBytes).toHaveBeenCalledTimes(fetched);
  });

  it('brings back the prompt that produced the generation it goes back to', async () => {
    const client = imageClient({
      generateImages: vi
        .fn<() => Promise<ImageBatch>>()
        .mockResolvedValueOnce({ ...batch, prompt: 'a lighthouse at dusk' })
        .mockResolvedValueOnce({ ...otherBatch, prompt: 'a fox in a boat' }),
    });
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });
    const clientAsClient = client as unknown as FileManagerClient;
    await wiring.generate(clientAsClient, entry.key);
    await settle();
    await wiring.generate(clientAsClient, entry.key);
    await settle();

    wiring.selectGeneration(clientAsClient, entry.key, 'g1');

    expect(entry.subject).toBe('a lighthouse at dusk');
  });

  it('fetches again for a generation whose bytes the backend has let go', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    entry.views = [{ imageId: 'i1', loading: false }];
    entry.activeGenerationId = 'g1';
    entry.history = [{ generationId: 'g1', batch, views: entry.views }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });
    const clientAsClient = client as unknown as FileManagerClient;
    // Look at something else, then come back to the generation whose preview is gone.
    entry.batch = otherBatch;
    entry.views = [{ imageId: 'i1', loading: false, previewUrl: 'blob:other' }];
    entry.activeGenerationId = 'g2';
    entry.history = [
      ...entry.history,
      { generationId: 'g2', batch: otherBatch, views: entry.views },
    ];

    wiring.selectGeneration(clientAsClient, entry.key, 'g1');
    await Promise.resolve();

    expect(client.generatedImageBytes).toHaveBeenCalledWith('g1', 'i1', false);
  });

  it('starts a new prompt and leaves the previous generations where they are', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    entry.presetIds = ['clipart'];
    entry.seed = 7;
    entry.batch = batch;
    entry.views = [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }];
    entry.activeGenerationId = 'g1';
    entry.history = [{ generationId: 'g1', batch, views: entry.views }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.newPrompt(client as unknown as FileManagerClient, entry.key);

    expect(entry.subject).toBe('');
    expect(entry.presetIds).toEqual([]);
    expect(entry.seed).toBeUndefined();
    expect(entry.batch).toBeUndefined();
    expect(entry.activeGenerationId).toBeUndefined();
    expect(entry.history.map((record) => record.generationId)).toEqual(['g1']);
    // Nothing was thrown away: the crumb still leads to the pictures the backend holds.
    expect(client.discardImageBatch).not.toHaveBeenCalled();
  });

  it('drops every generation the pane was holding when it closes', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = otherBatch;
    entry.views = [{ imageId: 'i1', loading: false }];
    entry.history = [{ generationId: 'g1', batch, views: [] }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.forget(client as unknown as FileManagerClient, entry.key);

    expect(client.discardImageBatch).toHaveBeenCalledWith('g1');
    expect(client.discardImageBatch).toHaveBeenCalledWith('g2');
  });

  it('takes a discarded generation out of the crumb bar', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = batch;
    entry.views = [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }];
    entry.activeGenerationId = 'g1';
    entry.history = [{ generationId: 'g1', batch, views: entry.views }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.discard(client as unknown as FileManagerClient, entry.key);

    expect(entry.history).toEqual([]);
    expect(entry.activeGenerationId).toBeUndefined();
  });
});

describe('several presets at once', () => {
  it('sends a treatment on top of the medium', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a fox';
    entry.presetIds = ['watercolour', 'blackAndWhite'];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    const sent = (client.generateImages as ReturnType<typeof vi.fn>).mock.calls[0]?.[0] as {
      prompt: string;
      negativePrompt: string | null;
    };
    expect(sent.prompt).toContain('loose watercolour illustration');
    expect(sent.prompt).toContain('film grain');
    // The two presets both dislike watermarks; the word is sent once.
    expect(
      sent.negativePrompt?.split(',').filter((term) => term.trim() === 'watermark'),
    ).toHaveLength(1);
  });
});

describe('how hard the model works, and what it starts from', () => {
  const catLocation: Location = { providerId: 'local', uri: 'file:///shots/cat.png' };
  const otherFolder: Location = { providerId: 'local', uri: 'file:///shots/' };

  /** A shell whose right pane is looking at a folder with one picture under its cursor. */
  function shellWithOtherPane(
    client: Partial<FileManagerClient>,
    entries: { id: string; name: string; kind: string; location: Location }[],
    cursorEntryId?: string,
  ): { shell: ShellState; entry: ImagePaneEntry } {
    const entry = emptyImagePane('left:t1', 'left', 't1', 'w1');
    const shell = {
      client: client as FileManagerClient,
      imageByTab: new Map([[entry.key, entry]]),
      imageOpening: false,
      directories: new Map<string, unknown>([['right:t2', { location: otherFolder, entries }]]),
      selections: new Map<string, unknown>([
        [
          'right:t2',
          { selectedEntryIds: cursorEntryId === undefined ? [] : [cursorEntryId], cursorEntryId },
        ],
      ]),
      workspace: {
        paneOrder: ['left', 'right'],
        panesById: { left: { activeTabId: 't1' }, right: { activeTabId: 't2' } },
      },
    } as unknown as ShellState;
    return { shell, entry };
  }

  const picture = { id: 'e1', name: 'cat.png', kind: 'file', location: catLocation };

  function sentRequest(client: Partial<FileManagerClient>): Record<string, unknown> {
    const generate = client.generateImages;
    if (generate === undefined) throw new Error('the client cannot generate images');
    const call = vi.mocked(generate).mock.calls[0];
    if (call === undefined) throw new Error('nothing was sent to the model');
    return call[0] as unknown as Record<string, unknown>;
  }

  it('asks for the level and the step count that were chosen', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a tabby cat';
    entry.qualityId = 'superQuality';
    entry.steps = 30;
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    expect(sentRequest(client)).toMatchObject({ quality: 'superQuality', steps: 30 });
  });

  it('asks nothing about either when the person left both alone', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a tabby cat';
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.generate(client as unknown as FileManagerClient, entry.key);

    const sent = sentRequest(client);
    expect(
      'quality' in sent,
      'an endpoint without those knobs sees the request it saw before',
    ).toBe(false);
    expect('steps' in sent).toBe(false);
    expect('source' in sent).toBe(false);
  });

  it('starts from the picture the other pane has under its cursor', () => {
    const client = imageClient();
    const { shell, entry } = shellWithOtherPane(client, [picture], 'e1');
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.useSource(entry.key);

    expect(entry.source).toEqual({ location: catLocation });
  });

  it('follows the cursor rather than a whole marked group', () => {
    const client = imageClient();
    const two = [
      picture,
      {
        id: 'e2',
        name: 'dog.png',
        kind: 'file',
        location: { providerId: 'local', uri: 'file:///shots/dog.png' },
      },
    ];
    const { shell, entry } = shellWithOtherPane(client, two, 'e2');
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.useSource(entry.key);

    expect(entry.source).toEqual({
      location: { providerId: 'local', uri: 'file:///shots/dog.png' },
    });
  });

  it('will not start from a folder', () => {
    const client = imageClient();
    const { shell, entry } = shellWithOtherPane(
      client,
      [{ id: 'e1', name: 'holidays', kind: 'directory', location: otherFolder }],
      'e1',
    );
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.useSource(entry.key);

    expect(entry.source, 'a folder is not a picture to start from').toBeUndefined();
  });

  it('will not guess a picture when the other pane has nothing under its cursor', () => {
    const client = imageClient();
    const { shell, entry } = shellWithOtherPane(client, [picture]);
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.useSource(entry.key);

    expect(entry.source).toBeUndefined();
  });

  it('can be talked out of the picture it was going to start from', () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.source = { location: catLocation };
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.clearSource(entry.key);

    expect(entry.source).toBeUndefined();
  });

  it('asks again of the same picture, at the same level, when it is asked to regenerate', async () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.batch = { ...batch, quality: 'good', steps: 20, source: { location: catLocation } };
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.regenerate(client as unknown as FileManagerClient, entry.key);

    expect(sentRequest(client)).toMatchObject({
      quality: 'good',
      steps: 20,
      source: { location: catLocation },
      prompt: batch.prompt,
    });
  });

  it('does not carry a picture over to a fresh idea', () => {
    const client = imageClient();
    const { shell, entry } = fakeShell(client);
    entry.subject = 'a tabby cat';
    entry.qualityId = 'fast';
    entry.steps = 10;
    entry.source = { location: catLocation };
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    wiring.newPrompt(client as unknown as FileManagerClient, entry.key);

    expect(entry.subject).toBe('');
    expect(entry.qualityId).toBe('');
    expect(entry.steps).toBeUndefined();
    expect(entry.source).toBeUndefined();
  });

  it('offers the tool only where an image model is assigned', async () => {
    const client = imageClient();
    const { shell } = fakeShell(client);
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });
    const assigned = {
      chat: null,
      embeddings: null,
      image: { profileId: 'p1', model: 'qwen-image', allowCloud: true },
    };

    expect(wiring.imageRoleAvailable()).toBe(false);

    client.listLlmRoles = async () => assigned;
    await wiring.refreshRoles(client as unknown as FileManagerClient);
    expect(wiring.imageRoleAvailable(), 'a named model is what makes the pane worth opening').toBe(
      true,
    );
    expect(shell.imageGenerationChecked).toBe(true);

    // Clearing the role takes the tool away again rather than leaving a pane that cannot work.
    client.listLlmRoles = async () => ({ chat: null, embeddings: null, image: null });
    await wiring.refreshRoles(client as unknown as FileManagerClient);
    expect(wiring.imageRoleAvailable()).toBe(false);

    // A host that cannot answer leaves it off too, so the pane is never offered on a guess.
    client.listLlmRoles = async () => {
      throw new Error('offline');
    };
    await wiring.refreshRoles(client as unknown as FileManagerClient);
    expect(wiring.imageRoleAvailable()).toBe(false);
  });
});

describe('choosing where a picture lands', () => {
  const downloads: Location = { providerId: 'local', uri: 'file:///Users/erik/Downloads/' };
  const shots: Location = { providerId: 'local', uri: 'file:///shots/' };

  /**
   * A left pane holding the picture tab (`t1`) beside its own folder tab (`t0`), and a right pane
   * showing `rightActive` - a folder, or a tool with no folder of its own.
   */
  function shellLookingAt(options: {
    views?: Record<string, Location>;
    rightActive?: string;
    rightTabs?: string[];
    homeDirectory?: string;
  }): { shell: ShellState; entry: ImagePaneEntry } {
    const entry = emptyImagePane('left:t1', 'left', 't1', 'w1');
    const shell = {
      client: imageClient(),
      imageByTab: new Map([[entry.key, entry]]),
      imageOpening: false,
      directories: new Map(
        Object.entries(options.views ?? {}).map(([key, location]) => [key, { location }]),
      ),
      workspace: {
        paneOrder: ['left', 'right'],
        panesById: {
          left: { activeTabId: 't1', tabOrder: ['t0', 't1'] },
          right: {
            activeTabId: options.rightActive ?? 'r1',
            tabOrder: options.rightTabs ?? ['r1'],
          },
        },
      },
      homeDirectory: options.homeDirectory,
    } as unknown as ShellState;
    return { shell, entry };
  }

  function folderOf(shell: ShellState, entry: ImagePaneEntry): Location | undefined {
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });
    return wiring.saveFolder(entry.key)?.location;
  }

  it('lands in the folder the other pane is looking at', () => {
    const { shell, entry } = shellLookingAt({
      views: { 'right:r1': downloads, 'left:t0': shots },
    });

    expect(folderOf(shell, entry)).toEqual(downloads);
  });

  it('takes the folder the other pane has open when it is showing something else', () => {
    const { shell, entry } = shellLookingAt({
      views: { 'right:r2': downloads, 'left:t0': shots },
      rightActive: 'r9',
      rightTabs: ['r9', 'r2'],
    });

    expect(folderOf(shell, entry)).toEqual(downloads);
  });

  it('takes a folder of its own pane when the other pane has none', () => {
    const { shell, entry } = shellLookingAt({ views: { 'left:t0': shots }, rightActive: 'r9' });

    expect(folderOf(shell, entry)).toEqual(shots);
  });

  it('lands in the pictures folder when no pane shows a folder', () => {
    const { shell, entry } = shellLookingAt({ rightActive: 'r9', homeDirectory: '/Users/erik' });

    expect(folderOf(shell, entry)).toEqual({
      providerId: 'local',
      uri: 'file:///Users/erik/Pictures/',
    });
  });

  it('offers a folder from every pane that is open, the one looked at first', () => {
    const entry = emptyImagePane('left:t1', 'left', 't1', 'w1');
    const elsewhere: Location = { providerId: 'local', uri: 'file:///tmp/third/' };
    const shell = {
      client: imageClient(),
      imageByTab: new Map([[entry.key, entry]]),
      directories: new Map(
        Object.entries({
          'right:r1': downloads,
          'left:t0': shots,
          'third:x1': elsewhere,
          'third:x2': downloads, // the same folder in two tabs is offered once
        }).map(([key, location]) => [key, { location }]),
      ),
      workspace: {
        paneOrder: ['left', 'right', 'third'],
        panesById: {
          left: { activeTabId: 't1', tabOrder: ['t0', 't1'] },
          right: { activeTabId: 'r1', tabOrder: ['r1'] },
          third: { activeTabId: 'x1', tabOrder: ['x1', 'x2'] },
        },
      },
    } as unknown as ShellState;
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    expect(wiring.saveFolders(entry.key).map((folder) => folder.location.uri)).toEqual([
      'file:///Users/erik/Downloads/',
      'file:///shots/',
      'file:///tmp/third/',
    ]);
    expect(wiring.saveFolders(entry.key)[0]?.name).toBe('Downloads');
  });

  it('keeps saving to the folder a picture was just saved to', async () => {
    const client = imageClient();
    const { shell, entry } = shellLookingAt({ rightActive: 'r9' });
    entry.batch = batch;
    entry.views = [{ imageId: 'i1', loading: false }];
    const wiring = createImageWiring(shell, { replaceWorkspace: vi.fn() });

    await wiring.save(client as unknown as FileManagerClient, entry.key, {
      imageId: 'i1',
      fileName: 'a.png',
      folder: downloads,
      overwrite: false,
    });

    expect(wiring.saveFolder(entry.key)).toEqual({ location: downloads, name: 'Downloads' });
  });
});
