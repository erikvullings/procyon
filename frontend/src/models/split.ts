/** The options carried by a `splitFile` operation (task 0257). */
export interface SplitFile {
  /** Size of every part except the last, in bytes (at least 1). */
  partSizeBytes: number;
  /** Also write `<name>.blake3`, listing the BLAKE3 hash of every part and of the whole file. */
  writeManifest: boolean;
}

/** One part of a detected split set. */
export interface SplitPart {
  name: string;
  size: number;
}

/** The split set a selection of part files belongs to, and whether it can be joined. */
export interface InspectSplitSetResult {
  /** Name of the file the parts join into. */
  outputName: string;
  parts: SplitPart[];
  totalSize: number;
  /** A `<name>.blake3` manifest sits next to the parts and will be verified. */
  hasManifest: boolean;
  /** Why the set cannot be joined; absent when it is ready. */
  error?: string | null;
}
