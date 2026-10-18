import { describe, expect, it, vi } from 'vitest';
import { createMediaAccess } from './media-access';

const archiveLocation = { providerId: 'archive', uri: 'archive:///a.zip!/song.mp3' };

describe('createMediaAccess', () => {
  it('does nothing for locations outside archives', async () => {
    const readFileRange = vi.fn();
    await createMediaAccess({ client: { readFileRange } })({
      providerId: 'local',
      uri: 'file:///a.mp3',
    });
    expect(readFileRange).not.toHaveBeenCalled();
  });

  it('probes a single byte of an archive entry', async () => {
    const readFileRange = vi
      .fn()
      .mockResolvedValue({ data: [1], offset: 0, length: 1, eof: false });
    await createMediaAccess({ client: { readFileRange } })(archiveLocation);
    expect(readFileRange).toHaveBeenCalledWith(
      { location: archiveLocation, offset: 0, length: 1 },
      undefined,
    );
  });

  it('prompts for the password and retries until the entry is readable', async () => {
    const readFileRange = vi
      .fn()
      .mockRejectedValueOnce({ code: 'credentialRequired' })
      .mockRejectedValueOnce({ code: 'invalidCredential' })
      .mockResolvedValue({ data: [1], offset: 0, length: 1, eof: false });
    const requestArchivePassword = vi.fn().mockResolvedValue(true);

    await createMediaAccess({ client: { readFileRange }, requestArchivePassword })(archiveLocation);

    expect(requestArchivePassword.mock.calls).toEqual([
      [archiveLocation, false],
      [archiveLocation, true],
    ]);
    expect(readFileRange).toHaveBeenCalledTimes(3);
  });

  it('rethrows when the user cancels the prompt or the error is something else', async () => {
    const credentialError = { code: 'credentialRequired' };
    await expect(
      createMediaAccess({
        client: { readFileRange: vi.fn().mockRejectedValue(credentialError) },
        requestArchivePassword: vi.fn().mockResolvedValue(false),
      })(archiveLocation),
    ).rejects.toBe(credentialError);

    const other = new Error('disk gone');
    await expect(
      createMediaAccess({ client: { readFileRange: vi.fn().mockRejectedValue(other) } })(
        archiveLocation,
      ),
    ).rejects.toBe(other);
  });
});
