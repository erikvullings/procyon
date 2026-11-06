/** The "verify copied files" choice, remembered per viewer between operations (task 0302). */
const STORAGE_KEY = 'fm.verifyCopiedFiles';

/** Whether copies should be verified; off unless the user ticked the box before. */
export function readVerifyTransfers(): boolean {
  try {
    return globalThis.localStorage?.getItem(STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

export function writeVerifyTransfers(verify: boolean): void {
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, String(verify));
  } catch {
    // Verification still works for this operation when storage is unavailable.
  }
}
