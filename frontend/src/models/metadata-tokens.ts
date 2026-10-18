import type { Location } from './location';

/** Request to resolve metadata rename tokens (task 0267). */
export interface ResolveMetadataTokensRequest {
  readonly locations: readonly Location[];
  /** Token names without braces, for example `artist` or `exif:date-taken:yyyy-MM-dd`. */
  readonly tokens: readonly string[];
}

/** Resolved token values for one file; a token without a value is absent. */
export interface MetadataTokenValues {
  readonly location: Location;
  readonly values: Readonly<Record<string, string>>;
}

export interface ResolveMetadataTokensResult {
  readonly entries: readonly MetadataTokenValues[];
  readonly unknownTokens: readonly string[];
}
