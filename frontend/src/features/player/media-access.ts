import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location } from '../../models';
import { applicationErrorCode } from '../navigation/navigation';

/** Makes a location ready to be streamed; rejects if it cannot be read. */
export type PrepareMedia = (location: Location, signal?: AbortSignal) => Promise<void>;

export interface MediaAccessOptions {
  readonly client: Pick<FileManagerClient, 'readFileRange'>;
  /**
   * Asks the user for an archive password and caches it on the backend; resolves `false` when the
   * user cancels. Absent where no password UI exists, in which case the error simply propagates.
   */
  readonly requestArchivePassword?: (location: Location, invalid: boolean) => Promise<boolean>;
}

/**
 * Builds the pre-flight that runs before a media element is pointed at the streaming URL
 * (task 0269). Media elements cannot report why a request failed, so an encrypted archive entry
 * would just look broken; probing one byte first surfaces `credentialRequired` /
 * `invalidCredential` so the usual archive password prompt can run, exactly as when browsing the
 * archive. Locations outside archives need no preparation.
 */
export function createMediaAccess(options: MediaAccessOptions): PrepareMedia {
  return async (location, signal) => {
    if (location.providerId !== 'archive') return;
    for (;;) {
      try {
        await options.client.readFileRange({ location, offset: 0, length: 1 }, signal);
        return;
      } catch (error: unknown) {
        const code = applicationErrorCode(error);
        if (
          options.requestArchivePassword === undefined ||
          (code !== 'credentialRequired' && code !== 'invalidCredential') ||
          !(await options.requestArchivePassword(location, code === 'invalidCredential'))
        ) {
          throw error;
        }
      }
    }
  };
}
