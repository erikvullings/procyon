import type { BeginOneDriveAuthorizationResponseDto } from '../api/generated/models/beginOneDriveAuthorizationResponseDto';
import type { ConnectionConfigurationDto } from '../api/generated/models/connectionConfigurationDto';
import type { ConnectionDto } from '../api/generated/models/connectionDto';
import type { ConnectionKindDto } from '../api/generated/models/connectionKindDto';
import type { ConnectionSecretInputDto } from '../api/generated/models/connectionSecretInputDto';
import type { ConnectionStatusDto } from '../api/generated/models/connectionStatusDto';
import type { CreateConnectionRequestDto } from '../api/generated/models/createConnectionRequestDto';
import type { FtpConnectionConfigurationDto } from '../api/generated/models/ftpConnectionConfigurationDto';
import type { HostKeyPolicyDto } from '../api/generated/models/hostKeyPolicyDto';
import type { HostKeyProbeDto } from '../api/generated/models/hostKeyProbeDto';
import type { OneDriveAuthorizationAttemptDto } from '../api/generated/models/oneDriveAuthorizationAttemptDto';
import type { OneDriveAuthorizationErrorCodeDto } from '../api/generated/models/oneDriveAuthorizationErrorCodeDto';
import type { OneDriveAuthorizationStatusDto } from '../api/generated/models/oneDriveAuthorizationStatusDto';
import type { OneDriveConnectionConfigurationDto } from '../api/generated/models/oneDriveConnectionConfigurationDto';
import type { S3ConnectionConfigurationDto } from '../api/generated/models/s3ConnectionConfigurationDto';
import type { SmbConnectionConfigurationDto } from '../api/generated/models/smbConnectionConfigurationDto';
import type { SshAuthenticationMethodDto } from '../api/generated/models/sshAuthenticationMethodDto';
import type { SshConnectionConfigurationDto } from '../api/generated/models/sshConnectionConfigurationDto';
import type { UpdateConnectionRequestDto } from '../api/generated/models/updateConnectionRequestDto';
import type { WebDavAuthenticationSchemeDto } from '../api/generated/models/webDavAuthenticationSchemeDto';
import type { WebDavConnectionConfigurationDto } from '../api/generated/models/webDavConnectionConfigurationDto';

// Re-exported as plain aliases (spec §5.2, task 0103): the backend DTOs are
// already the shape the frontend wants for these flat/tagged-union types, so
// no normalization or `fromDto` mapping is needed (matching how
// `WorkspaceLayout`/`WorkspaceSummary` alias their Dto counterparts directly
// in `./workspace.ts`).

/** A connection's protocol/provider family (spec §5.2). */
export type ConnectionKind = ConnectionKindDto;

/** A connection's runtime status (spec §5.4). */
export type ConnectionStatus = ConnectionStatusDto;

/** How an SSH connection authenticates (spec §6.3). */
export type SshAuthenticationMethod = SshAuthenticationMethodDto;

/** SSH host-key verification policy (spec §6.4). */
export type HostKeyPolicy = HostKeyPolicyDto;

/**
 * Result of probing an SSH connection's currently presented host key without
 * authenticating (spec §6.4, task 0104): whether it is already trusted, has
 * never been seen before, or has changed since it was last accepted.
 */
export type HostKeyProbe = HostKeyProbeDto;

/** SSH connection configuration (spec §6.3). */
export type SshConnectionConfiguration = SshConnectionConfigurationDto;

/** Minimal FTP/FTPS connection configuration. */
export type FtpConnectionConfiguration = FtpConnectionConfigurationDto;

/** Minimal native OneDrive connection configuration. */
export type OneDriveConnectionConfiguration = OneDriveConnectionConfigurationDto;

/** Starts browser authorization without exposing an authorization code or token. */
export type BeginOneDriveAuthorizationResponse = BeginOneDriveAuthorizationResponseDto;

/** Sanitized lifecycle state for one backend-owned authorization attempt. */
export type OneDriveAuthorizationAttempt = OneDriveAuthorizationAttemptDto;

/** Actionable, secret-free reason an authorization attempt failed. */
export type OneDriveAuthorizationErrorCode = OneDriveAuthorizationErrorCodeDto;

/** Current state of a backend-owned authorization attempt. */
export type OneDriveAuthorizationStatus = OneDriveAuthorizationStatusDto;

/** Minimal WebDAV connection configuration. */
export type WebDavConnectionConfiguration = WebDavConnectionConfigurationDto;

/** How a WebDAV connection authenticates (task 0147). */
export type WebDavAuthenticationScheme = WebDavAuthenticationSchemeDto;

/** Minimal S3-compatible connection configuration. */
export type S3ConnectionConfiguration = S3ConnectionConfigurationDto;

/** Minimal native SMB connection configuration. */
export type SmbConnectionConfiguration = SmbConnectionConfigurationDto;

/** Tagged, protocol-specific connection configuration (spec §5.2). */
export type ConnectionConfiguration = ConnectionConfigurationDto;

/**
 * Write-only secret input accepted when creating or updating a connection.
 * Never present on any value read back from the backend - clear it from
 * in-memory form state immediately after a successful save.
 */
export type ConnectionSecretInput = ConnectionSecretInputDto;

/**
 * A saved connection profile (spec §5.2), as returned by the backend. Never
 * carries secret content: `hasCredential` is a presence boolean only.
 */
export type Connection = ConnectionDto;

/** Input used to create a stored connection. */
export type CreateConnectionRequest = CreateConnectionRequestDto;

/** Input used to update a stored connection. */
export type UpdateConnectionRequest = UpdateConnectionRequestDto;

/** Default typed SSH configuration for a freshly opened "add connection" form. */
export function defaultSshConfiguration(): ConnectionConfiguration {
  return {
    kind: 'ssh',
    host: '',
    port: 22,
    username: '',
    startPath: null,
    authentication: 'password',
    hostKeyPolicy: 'promptOnFirstUse',
    keepaliveSeconds: null,
  };
}
