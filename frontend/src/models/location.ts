import type { EntryId, ProviderId } from './ids';

/**
 * A provider-neutral pointer to a location (spec §5.1), mirroring
 * `fm_transport_dto::LocationDto`.
 */
export interface Location {
  providerId: ProviderId;
  uri: string;
}

/** A navigable local-provider location discovered by the host operating system. */
export interface SystemLocation {
  readonly name: string;
  readonly kind: 'cloud' | 'network';
  readonly location: Location;
  readonly providerHint?: string;
  readonly protocol?: string;
  readonly server?: string;
  readonly share?: string;
  readonly readOnly?: boolean;
}

/** A currently mounted local/removable/disk-image volume (task 0144). */
export interface Volume {
  readonly name: string;
  readonly location: Location;
}

/** Identifies an entry without its full summary, e.g. for metadata requests (spec §6). */
export interface EntryRef {
  id: EntryId;
  location: Location;
}
