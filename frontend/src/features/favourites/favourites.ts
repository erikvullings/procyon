import type { Location } from '../../models';

/** A named location displayed in the favourites menu. */
export interface FavouriteLocation {
  readonly label: string;
  readonly location: Location;
}

/** Maximum number of recent locations retained for one workspace. */
export const MAX_RECENT_LOCATIONS = 12;

function sameLocation(left: Location, right: Location): boolean {
  return left.providerId === right.providerId && left.uri === right.uri;
}

/** Whether a location can be reopened after the in-memory provider state changes. */
export function isRestorableRecentLocation(location: Location): boolean {
  return !location.uri.startsWith('search://') && !location.uri.startsWith('archive://');
}

/** Filters legacy favourites that persisted session-only provider resources. */
export function isRestorableFavouriteLocation(favourite: FavouriteLocation): boolean {
  return isRestorableRecentLocation(favourite.location);
}

/**
 * Records a visit without mutating the existing list. The newest visit is first,
 * duplicate locations are collapsed, and locations removed from favourites do
 * not reappear in the recent list.
 */
export function recordRecentLocation(
  existing: readonly Location[],
  location: Location,
  removedFavourites: readonly Location[] = [],
): readonly Location[] {
  const restorableExisting = existing.filter(isRestorableRecentLocation);
  if (
    !isRestorableRecentLocation(location) ||
    removedFavourites.some((removed) => sameLocation(removed, location))
  ) {
    return restorableExisting;
  }
  return [
    location,
    ...restorableExisting.filter((candidate) => !sameLocation(candidate, location)),
  ].slice(0, MAX_RECENT_LOCATIONS);
}

const LOCATION_SCHEME_PATTERN = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//;

/** Characters kept when a recent location's URI is too long for the favourites menu. */
export const RECENT_LOCATION_DISPLAY_MAX_LENGTH = 56;

/**
 * Shortens a location URI for display by dropping characters from the middle rather than the
 * end, so the last (most identifying) path segment stays visible instead of the scheme. The
 * scheme (`file://`, `archive://`, ...) is always preserved ahead of the ellipsis.
 */
export function truncateLocationForDisplay(
  uri: string,
  maxLength: number = RECENT_LOCATION_DISPLAY_MAX_LENGTH,
): string {
  if (uri.length <= maxLength) return uri;
  const ellipsis = '…';
  const scheme = uri.match(LOCATION_SCHEME_PATTERN)?.[0] ?? '';
  const rest = uri.slice(scheme.length);
  const tailLength = Math.max(maxLength - scheme.length - ellipsis.length, 0);
  return `${scheme}${ellipsis}${rest.slice(rest.length - tailLength)}`;
}

/** Moves an existing favourite to a requested position without changing its identity. */
export function reorderFavourites(
  favourites: readonly FavouriteLocation[],
  from: number,
  to: number,
): readonly FavouriteLocation[] {
  if (from < 0 || from >= favourites.length || to < 0 || to >= favourites.length) return favourites;
  const reordered = [...favourites];
  const [favourite] = reordered.splice(from, 1);
  if (favourite === undefined) return favourites;
  reordered.splice(to, 0, favourite);
  return reordered;
}
