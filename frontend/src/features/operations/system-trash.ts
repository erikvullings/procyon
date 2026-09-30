import type { Location } from '../../models';

/** Whether every location can go to the OS trash; other providers need a confirmed permanent delete. */
export function canUseSystemTrash(locations: readonly Location[]): boolean {
  return locations.every(
    (location) => location.providerId === 'file' || location.providerId === 'local',
  );
}
