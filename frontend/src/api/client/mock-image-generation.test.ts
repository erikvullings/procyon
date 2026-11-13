import { describe, expect, it } from 'vitest';
import type { GeneratedImage, ImageBatch, LlmProfile, SaveLlmProfileRequest } from '../../models';
import { MockFileManagerClient } from './mock-file-manager-client';

function localProfile(): SaveLlmProfileRequest {
  return {
    name: 'Local images',
    preset: 'custom',
    baseUrl: 'http://127.0.0.1:8000/v1',
    deployment: null,
    apiVersion: null,
    model: 'flux-schnell',
    credential: null,
    advanced: {
      contextWindow: 4096,
      maximumAnswerTokens: 512,
      temperature: 0.7,
      timeoutSeconds: 120,
      tlsPolicy: 'requireValidCertificate',
      customHeaders: {},
    },
    // An image endpoint answers neither chat nor discovery API; the role decides what is asked of it.
    locality: 'loopback',
    consentedHost: null,
    redactFilenames: false,
  } as unknown as SaveLlmProfileRequest;
}

/** The first image of a batch; the mock always produces at least one. */
function firstImage(batch: ImageBatch): GeneratedImage {
  const image = batch.images[0];
  if (image === undefined) throw new Error('the mock produced no images');
  return image;
}

async function clientWithImageRole(
  overrides: Partial<SaveLlmProfileRequest> = {},
): Promise<MockFileManagerClient> {
  const client = new MockFileManagerClient();
  const profile = await client.createLlmProfile({ ...localProfile(), ...overrides });
  await client.setLlmRole('image', {
    profileId: profile.id,
    model: 'flux-schnell',
    allowCloud: true,
  });
  return client;
}

const REQUEST = {
  requestId: '11111111-1111-4111-8111-111111111111',
  workspaceId: 'workspace-1',
  prompt: 'a lighthouse at dusk',
  negativePrompt: null,
  width: 1_024,
  height: 1_024,
  count: 2,
  seed: 7,
} as const;

describe('MockFileManagerClient image generation', () => {
  it('returns a batch of PNGs the preview can render, holding them by id', async () => {
    const client = await clientWithImageRole();

    const batch = await client.generateImages(REQUEST);

    expect(batch.images).toHaveLength(2);
    expect(batch.model).toBe('flux-schnell');
    expect(batch.seed).toBe(7);
    const bytes = await client.generatedImageBytes(batch.generationId, firstImage(batch).id, false);
    expect(bytes.mediaType).toBe('image/png');
    // A PNG signature: the preview side can put these straight into an <img>.
    expect([...bytes.bytes.slice(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  });

  it('refuses to bind an endpoint that only speaks the images API to the chat role', async () => {
    const client = new MockFileManagerClient();
    const profile = await client.createLlmProfile({
      ...localProfile(),
      name: 'Model Studio',
      preset: 'modelStudio',
      baseUrl: 'https://dashscope.aliyuncs.com/api/v1',
      capabilities: ['dashscopeTasks'],
      redactFilenames: true,
    });

    await expect(
      client.setLlmRole('chat', { profileId: profile.id, model: 'qwen-plus', allowCloud: true }),
    ).rejects.toMatchObject({ code: 'invalidRequest', details: { reason: 'llmProfileInvalid' } });

    // The same endpoint is a fine image endpoint.
    await expect(
      client.setLlmRole('image', { profileId: profile.id, model: 'qwen-image', allowCloud: true }),
    ).resolves.toMatchObject({ image: { model: 'qwen-image' } });
  });

  it('asks for an image role and refuses a cloud endpoint without consent', async () => {
    const withoutRole = new MockFileManagerClient();
    await expect(withoutRole.generateImages(REQUEST)).rejects.toMatchObject({
      code: 'roleNotAssigned',
    });

    const cloud = await clientWithImageRole({
      name: 'Cloud images',
      baseUrl: 'https://api.example.test/v1',
    });
    await expect(cloud.generateImages(REQUEST)).rejects.toMatchObject({
      code: 'cloudImageNotAllowed',
    });
  });

  it('refuses the requests the backend refuses', async () => {
    const client = await clientWithImageRole();
    const cases = [
      [{ prompt: '   ' }, 'the prompt is empty'],
      [{ width: 1_023 }, 'multiple of 8'],
      [{ width: 4_096 }, 'outside the accepted range'],
      [{ width: 2_048, height: 2_048 }, 'too many pixels'],
      [{ count: 5 }, 'number of images'],
      [{ seed: 4_294_967_296 }, 'seed is too large'],
    ] as const;
    for (const [changes, why] of cases) {
      await expect(client.generateImages({ ...REQUEST, ...changes }), why).rejects.toMatchObject({
        code: 'invalidRequest',
      });
    }
  });

  it('writes nothing until a save, and then refuses to replace without agreement', async () => {
    const client = await clientWithImageRole();
    const batch = await client.generateImages(REQUEST);
    const image = firstImage(batch);
    const destination = {
      providerId: 'mock',
      uri: 'file:///shots/lighthouse-1024x1024-1.png',
    };

    await expect(
      client.saveGeneratedImage({
        generationId: batch.generationId,
        imageId: image.id,
        destination,
        overwrite: false,
      }),
    ).resolves.toMatchObject({ location: destination, bytesWritten: image.bytes });

    // The same name now exists: a second save has to ask before replacing it.
    await expect(
      client.saveGeneratedImage({
        generationId: batch.generationId,
        imageId: image.id,
        destination,
        overwrite: false,
      }),
    ).rejects.toMatchObject({ code: 'destinationAlreadyExists' });

    await expect(
      client.saveGeneratedImage({
        generationId: batch.generationId,
        imageId: image.id,
        destination,
        overwrite: true,
      }),
    ).resolves.toMatchObject({ location: destination });
  });

  it('refuses a name that is not a file name of the image format', async () => {
    const client = await clientWithImageRole();
    const batch = await client.generateImages(REQUEST);
    // The backend validates the name too; the mock refuses the same names.
    for (const name of ['lighthouse.jpg', '.', '..', 'a.png ', 'a\\b.png', 'a\u0000.png', '']) {
      await expect(
        client.saveGeneratedImage({
          generationId: batch.generationId,
          imageId: firstImage(batch).id,
          destination: { providerId: 'mock', uri: `file:///shots/${name}` },
          overwrite: false,
        }),
      ).rejects.toMatchObject({ code: 'invalidRequest' });
    }
  });

  it('drops a discarded batch so it can no longer be saved', async () => {
    const client = await clientWithImageRole();
    const batch = await client.generateImages(REQUEST);
    await client.discardImageBatch(batch.generationId);

    await expect(
      client.saveGeneratedImage({
        generationId: batch.generationId,
        imageId: firstImage(batch).id,
        destination: { providerId: 'mock', uri: 'file:///shots/a.png' },
        overwrite: false,
      }),
    ).rejects.toMatchObject({ code: 'imageBatchExpired' });
    await expect(
      client.generatedImageBytes(batch.generationId, firstImage(batch).id, false),
    ).rejects.toMatchObject({ code: 'imageBatchExpired' });
  });

  it('reports a stopped generation instead of a batch', async () => {
    const client = await clientWithImageRole();
    await client.cancelImageGeneration(REQUEST.requestId);
    await expect(client.generateImages(REQUEST)).rejects.toMatchObject({ code: 'cancelled' });
  });
});

describe('MockFileManagerClient image role bindings', () => {
  it('keeps a cloud image endpoint behind the cloud permission', async () => {
    const client = new MockFileManagerClient();
    const profile: LlmProfile = await client.createLlmProfile({
      ...localProfile(),
      name: 'Cloud images',
      baseUrl: 'https://api.example.test/v1',
    });

    await expect(
      client.setLlmRole('image', { profileId: profile.id, model: 'flux', allowCloud: false }),
    ).rejects.toMatchObject({ details: { reason: 'llmCloudImageNotAllowed' } });

    await expect(
      client.setLlmRole('image', { profileId: profile.id, model: 'flux', allowCloud: true }),
    ).resolves.toMatchObject({ image: { model: 'flux' } });

    const roles = await client.listLlmRoles();
    expect(roles.image?.profileId).toBe(profile.id);
  });
});
