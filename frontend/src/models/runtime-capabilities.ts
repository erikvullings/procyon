/**
 * `RuntimeCapabilities` already has a real backend DTO (task 0008); re-export
 * it as-is so feature code depends on `models/`, never on `api/generated/`
 * directly (spec §12).
 */
export type { RuntimeCapabilitiesDto as RuntimeCapabilities } from '../api/generated/models/runtimeCapabilitiesDto';
