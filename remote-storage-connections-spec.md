# Architecture Extension: Remote Filesystems, Cloud Locations, and Remote Connections

## 1. Purpose

Extend the existing Rust file-manager architecture with:

- OS-exposed cloud-backed locations such as OneDrive, iCloud Drive, Dropbox and Google Drive;
- mounted network filesystems such as SMB shares;
- SSH/SFTP remote filesystems;
- SSH terminal/remote-command actions;
- FTP and FTPS;
- launching external RDP/VNC clients;
- future native OneDrive/cloud providers;
- future native SMB support.

The existing architecture remains authoritative:

- `FileSystemProvider` abstracts browsable filesystem-like locations;
- the Rust operation engine owns copy/move/delete semantics;
- `Location` identifies provider-neutral locations;
- the frontend invokes semantic application services rather than filesystem primitives;
- browser/Axum and Tauri remain alternative hosts/transports around the same Rust services.

This extension adds a deliberate distinction between:

1. **OS-exposed filesystem locations** — local disks, removable media, mounted SMB/NFS/WebDAV shares and cloud folders already exposed by the OS.
2. **Application-managed remote connections** — SSH/SFTP, FTP/FTPS, and later WebDAV/S3/native SMB/native cloud APIs.
3. **Non-filesystem services associated with connections** — SSH terminal, remote commands and remote-desktop launch.

The implementation should begin with the features that require no connection manager.

---

# 2. Recommended implementation order

1. Recognize and present OS-exposed cloud-backed locations.
2. Discover and present mounted network volumes, including SMB shares on macOS.
3. Add reusable connection-profile and secure-credential infrastructure.
4. Add SFTP over SSH as the primary SSH file-transfer provider.
5. Add SSH terminal/command actions associated with SSH connections.
6. Add FTP and FTPS.
7. Add external remote-desktop launch actions.
8. Harden cross-provider transfers and remote change tracking.
9. Add native OneDrive only when direct cloud API access is actually required.
10. Add native SMB only when OS-mounted SMB is insufficient.

Items 1 and 2 are intentionally independent of `ConnectionManager`.

---

# 3. Easy win: OS-exposed cloud-backed locations

## 3.1 Goal

Present cloud storage already exposed by macOS or Windows as first-class locations without implementing vendor APIs.

Examples:

- OneDrive / OneDrive for Business;
- iCloud Drive;
- Dropbox;
- Google Drive;
- other providers using OS file-provider/sync mechanisms.

These locations must continue to use the existing local filesystem provider. Do not create OneDrive/Dropbox/Google connection profiles for this feature.

## 3.2 Architecture

Add a platform-facing location discovery abstraction:

```rust
#[async_trait]
pub trait SystemLocationProvider: Send + Sync {
    async fn discover_locations(
        &self,
    ) -> Result<Vec<SystemLocation>, SystemLocationError>;
}
```

Use a model such as:

```rust
pub struct SystemLocation {
    pub id: SystemLocationId,
    pub display_name: String,
    pub location: Location,
    pub kind: SystemLocationKind,
    pub icon_key: Option<String>,
    pub availability: LocationAvailability,
    pub provider_hint: Option<String>,
}

pub enum SystemLocationKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    LocalVolume,
    RemovableVolume,
    NetworkVolume,
    CloudStorage,
}
```

All cloud locations discovered here resolve to the existing `local` provider and a `file://` location.

`provider_hint` is advisory only, for example `onedrive`, `icloud`, `dropbox`, `google-drive`, or `unknown-cloud`. File-operation semantics must never depend on it.

## 3.3 Frontend

Add a Locations/sidebar surface such as:

```text
FAVOURITES
  Home
  Downloads
  Development

CLOUD
  iCloud Drive
  OneDrive — Personal
  OneDrive — Work
  Google Drive

LOCAL
  Macintosh HD
  External SSD
```

Opening these entries behaves exactly like opening an ordinary local directory.

Optional later enhancements may show states such as unavailable, offline, placeholder/not downloaded, or syncing.

## 3.4 Constraints

- Do not hard-code user-specific cloud paths.
- Prefer platform APIs/conventions for discovery.
- Fall back gracefully if a provider is installed but unavailable.
- Do not block directory listing while obtaining rich cloud metadata.
- Treat cloud-provider state as advisory; actual I/O remains owned by the local provider.
- Tolerate latency when the OS downloads on-demand placeholder files.

## 3.5 Effort

- Basic discovery/sidebar integration: a few days.
- Polished icons/state/errors/cross-platform behavior: roughly 1–3 weeks.

This is the highest-value, lowest-risk addition in this extension.

---

# 4. Easy win: mounted network volumes and SMB on macOS

## 4.1 Goal

Support network filesystems already mounted by the operating system, especially SMB/Samba on macOS.

A user may mount:

```text
smb://nas.local/media
```

and macOS exposes it as a normal mounted filesystem. Browse it with the existing local provider; no native SMB protocol implementation is needed here.

## 4.2 Architecture

Reuse `SystemLocationProvider` and represent mounted network volumes as `SystemLocationKind::NetworkVolume` with a normal local `file://` location.

Optional metadata:

```rust
pub struct NetworkVolumeMetadata {
    pub protocol_hint: Option<NetworkProtocolHint>,
    pub server_name: Option<String>,
    pub share_name: Option<String>,
    pub mounted: bool,
    pub read_only: Option<bool>,
}

pub enum NetworkProtocolHint {
    Smb,
    Nfs,
    WebDav,
    Other,
}
```

Hints must not alter basic local-provider semantics.

## 4.3 Behaviour

Handle:

- share mounted at startup;
- share mounted while the app is running;
- share unmounted while a pane/tab is open;
- temporary network loss;
- read-only mounts;
- slow enumeration.

If a mounted share disappears, preserve the tab and show a recoverable unavailable-location state.

An optional later action may ask the OS to mount an SMB URL; discovery-only support is sufficient initially.

## 4.4 Effort

- Mounted-volume discovery/display: a few days.
- Polished reconnect/disconnect behavior and metadata: about 1–3 weeks.

---

# 5. Connection subsystem

## 5.1 Purpose

Application-managed remote protocols need reusable saved endpoint/account configuration and secure credentials.

Add:

```text
fm-connections
fm-credentials
```

Used by:

- SSH/SFTP;
- FTP/FTPS;
- remote-desktop launch;
- future native OneDrive/WebDAV/S3/SMB.

Not used by OS-exposed cloud folders or already-mounted network volumes.

## 5.2 Connection profile

```rust
pub struct ConnectionProfile {
    pub id: ConnectionId,
    pub name: String,
    pub kind: ConnectionKind,
    pub configuration: ConnectionConfiguration,
    pub credential_ref: Option<CredentialRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum ConnectionKind {
    Ssh,
    Ftp,
    Ftps,
    OneDrive,
    WebDav,
    S3,
    Smb,
}
```

Protocol-specific configuration must be typed/tagged, not an unstructured map.

Never persist passwords, key passphrases, OAuth refresh tokens or other secrets in a connection profile.

## 5.3 Credential store

```rust
#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn store(
        &self,
        request: StoreCredentialRequest,
    ) -> Result<CredentialRef, CredentialError>;

    async fn resolve(
        &self,
        reference: &CredentialRef,
    ) -> Result<ResolvedCredential, CredentialError>;

    async fn delete(
        &self,
        reference: &CredentialRef,
    ) -> Result<(), CredentialError>;
}
```

Preferred platform stores:

- macOS: Keychain;
- Windows: Windows Credential Manager or equivalent OS-protected store.

Secrets must never appear in workspace JSON, settings JSON, URLs, logs, OpenAPI examples or plugin-visible DTOs.

## 5.4 Status

```rust
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    AuthenticationRequired,
    Failed,
}
```

A connection is a logical saved endpoint/account. Providers may pool physical sessions behind it.

## 5.5 Frontend

Add a section such as:

```text
SERVERS
  Home Server       ●
  NAS               ●
  Web Hosting       ○
```

Context actions are capability-driven:

```text
Browse Files
Open Terminal
Open Remote Desktop
Reconnect
Disconnect
Edit Connection…
Remove
```

---

# 6. SSH/SFTP support

## 6.1 Use SFTP for file management

Implement SSH-based browsing and transfer with SFTP. Do not make legacy SCP the primary transfer implementation.

The user-facing feature can be called SSH/SFTP. SCP compatibility can be considered later only for targets that lack SFTP and when there is a concrete need.

## 6.2 Structure

```text
crates/
  fm-connections/
  fm-credentials/
  fm-ssh/
  fm-vfs-sftp/
```

`fm-ssh` owns reusable SSH session/authentication logic. `fm-vfs-sftp` implements `FileSystemProvider`.

```rust
pub struct SftpProvider {
    connections: Arc<SshConnectionManager>,
}
```

## 6.3 SSH configuration

```rust
pub struct SshConnectionConfiguration {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub authentication: SshAuthenticationMethod,
    pub host_key_policy: HostKeyPolicy,
    pub keepalive: Option<Duration>,
}
```

Eventually support password, private key, encrypted key, SSH agent, SSH config import and jump hosts.

## 6.4 Host keys

Mandatory:

- first-connect confirmation;
- persist accepted fingerprint/known host;
- clear changed-host-key warning;
- explicit confirmation before replacing a known key.

Never silently accept a changed SSH host key.

## 6.5 Locations

Use saved connection references rather than credentials in URIs:

```text
sftp://<connection-id>/home/erik
```

## 6.6 Capabilities

Typical initial SFTP capabilities:

```text
list                 yes
read                 yes
write                yes
create_directory     yes
rename               yes
move                  yes, where supported
server_side_copy      usually limited/unsupported
trash                 generally no
watch                 no native watch
checksum              server-dependent
```

## 6.7 Transfers

Support through the operation engine:

```text
local → SFTP
SFTP → local
SFTP → SFTP
```

If a same-provider/same-connection server-side operation is safe, use it. Otherwise stream directly:

```text
provider.open_read()
        ↓
operation engine
        ↓
provider.open_write()
```

Do not require temporary local files.

## 6.8 Resilience

Implement timeouts, cancellation, keepalive, reconnect for browsing, clear auth errors, partial-file cleanup and safe retry behavior. Resume can follow after correctness.

## 6.9 Effort

- Basic provider: roughly 2–4 weeks.
- Polished auth/host-key/reconnect/pooling/resume/UX: roughly 6–10 weeks.

---

# 7. SSH terminal and command support

SSH terminal access is not a filesystem concern.

Reuse `fm-ssh` for:

```text
fm-ssh
  ├── session/authentication
  ├── SFTP consumers
  ├── terminal/PTY consumers
  └── remote-command consumers
```

Define a remote-shell service:

```rust
#[async_trait]
pub trait RemoteShellService: Send + Sync {
    async fn open_terminal(
        &self,
        connection_id: ConnectionId,
        working_directory: Option<ProviderPath>,
    ) -> Result<RemoteTerminalSession, RemoteShellError>;

    async fn execute(
        &self,
        connection_id: ConnectionId,
        request: RemoteCommandRequest,
    ) -> Result<RemoteCommandResult, RemoteShellError>;
}
```

Expose actions:

```text
Open Terminal Here
Run Command…
Copy SSH URI
Reconnect
Connection Properties…
```

Initial implementation may launch the preferred external terminal with SSH. Embedded terminal emulation can be later.

Security:

- avoid unsafe shell interpolation;
- prefer structured SSH APIs;
- do not give untrusted plugins arbitrary remote-command access by default.

---

# 8. FTP and FTPS

## 8.1 Scope

Implement FTP and explicit FTPS. Add implicit FTPS only if real target servers require it.

Plain FTP must be visibly marked insecure.

## 8.2 Structure

```text
fm-vfs-ftp
```

Reuse `fm-connections` and `fm-credentials`.

```rust
pub struct FtpConnectionConfiguration {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub security: FtpSecurity,
    pub passive_mode: bool,
}
```

## 8.3 Protocol differences

Account for:

- passive/active mode;
- separate control/data channels;
- inconsistent listing formats;
- uncertain timestamps;
- variable permission support;
- NAT/firewall failures;
- connection drops;
- limited checksums/server-side copies;
- no native filesystem watching.

Represent limitations via provider capabilities. Do not fake unsupported semantics.

## 8.4 Transfers

Use the same stream-based operation engine for FTP↔local, FTP↔SFTP and future provider combinations.

## 8.5 Effort

- Basic FTP: around 1–2 weeks.
- FTP + FTPS + polished connection UX: around 3–5 weeks.
- Broad compatibility hardening can add several weeks.

SFTP should normally be higher priority.

---

# 9. Remote desktop integration

Remote desktop is not a filesystem provider.

Associate RDP/VNC settings/actions with saved connections and initially launch external clients.

```rust
pub struct RemoteDesktopConfiguration {
    pub protocol: RemoteDesktopProtocol,
    pub host: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub client_preference: Option<RemoteDesktopClientPreference>,
}

pub enum RemoteDesktopProtocol {
    Rdp,
    Vnc,
}
```

Actions:

```text
Open Remote Desktop
Copy Remote Desktop Address
Edit Remote Desktop Settings…
```

Use platform adapters to launch available/configured clients on macOS and Windows. Avoid a hard dependency on one third-party application.

Embedded RDP/VNC is explicitly out of initial scope; it requires framebuffer/codecs, keyboard mapping, pointer capture, resizing, clipboard, certificates, auth/NLA and possibly audio/multimonitor support.

Expected effort:

- external launch: days to 1–2 weeks;
- embedded remote desktop: many weeks/months and should be a separate feature.

---

# 10. Cross-provider operation planning

Remote providers make transfer planning more important.

Add:

```rust
pub struct TransferCapabilities {
    pub server_side_copy: bool,
    pub server_side_move: bool,
    pub resumable_upload: bool,
    pub resumable_download: bool,
    pub random_read: bool,
    pub random_write: bool,
}
```

The planner chooses among:

1. provider-native rename/move;
2. provider-native server-side copy;
3. direct source-stream → destination-stream copy;
4. resumable transfer where supported.

Example same-server SFTP moves may be server-native. SFTP→FTP should stream without a local temp copy.

Progress, conflicts and cancellation remain provider-neutral.

---

# 11. Remote change tracking

Generalize local-style `watch()` semantics:

```rust
pub enum ChangeTracking {
    NativeWatch,
    DeltaApi,
    Poll {
        recommended_interval: Duration,
    },
    Unsupported,
}
```

Typical mapping:

```text
local filesystem       NativeWatch
mounted SMB            NativeWatch where OS permits
native OneDrive        DeltaApi
SFTP                    Poll
FTP                     Poll
```

Polling must be cancellable, back off on failures, slow down for inactive tabs where appropriate and avoid emitting new revisions when nothing changed.

---

# 12. Future native OneDrive provider

## 12.1 When needed

Do not implement native OneDrive merely to browse locally installed OneDrive. Task/order item 1 already handles that through the local filesystem.

Native OneDrive is useful when the app must access OneDrive directly without the sync client/OS filesystem representation.

## 12.2 Architecture

```text
fm-auth-oauth
fm-vfs-onedrive
```

Needs:

- OAuth authorization/token refresh;
- Microsoft Graph integration;
- paging;
- streaming downloads;
- resumable uploads;
- delta synchronization;
- throttling/backoff;
- drive/account selection.

Refresh tokens must use `CredentialStore` only.

Conceptual locations:

```text
onedrive://<connection-id>/root/Documents
onedrive://<connection-id>/<drive-id>/Projects
```

Expected effort:

- usable native provider: roughly 4–8 weeks;
- polished personal/business/SharePoint support: roughly 8–16 weeks.

---

# 13. Future native SMB provider

Only add native SMB if OS-mounted SMB proves insufficient.

Native SMB must handle authentication, share enumeration, dialects, NTLM/Kerberos as required, DFS, ACLs, locks/leases, reconnect and SMB-specific errors.

Expected effort:

- MVP: several weeks;
- robust corporate-grade support: potentially 8–16+ weeks.

Mounted SMB and native SMB should coexist; native SMB must not replace the easy OS-mounted path.

---

# 14. Provider and connection separation

Keep these concepts distinct:

```text
ConnectionProfile
    describes/authenticates an app-managed endpoint/account

FileSystemProvider
    exposes filesystem-like operations

SystemLocationProvider
    discovers OS-visible locations

RemoteShellService
    provides SSH terminal/commands

RemoteDesktopService
    launches remote-desktop sessions
```

Examples:

```text
OS OneDrive folder
  SystemLocationProvider → local FileSystemProvider

Mounted SMB share
  SystemLocationProvider → local FileSystemProvider

SSH connection
  ConnectionProfile
    ├── SFTP FileSystemProvider
    ├── RemoteShellService
    └── optional RemoteDesktop configuration

FTP connection
  ConnectionProfile
    └── FTP FileSystemProvider

Native OneDrive
  ConnectionProfile
    └── OneDrive FileSystemProvider
```

Do not turn every visible location into a connection profile.

---

# 15. Suggested repository additions

```text
crates/
├── fm-system-locations/
├── fm-connections/
├── fm-credentials/
├── fm-ssh/
├── fm-vfs-sftp/
├── fm-vfs-ftp/
├── fm-remote-desktop/
├── fm-auth-oauth/             # later
├── fm-vfs-onedrive/           # later
└── fm-vfs-smb/                # later

frontend/src/features/
├── locations/
├── connections/
├── connection-editor/
└── remote-actions/
```

Use existing `fm-platform-macos` / `fm-platform-windows` for:

- system/cloud location discovery;
- mounted-volume discovery;
- secure credential integration;
- remote-desktop client launch;
- platform-specific icons and labels.

---

# 16. API additions

Suggested services:

```rust
pub struct LocationService;
pub struct ConnectionService;
pub struct CredentialService;
pub struct RemoteShellService;
pub struct RemoteDesktopService;
```

Potential REST endpoints:

```text
GET    /api/v1/system-locations

GET    /api/v1/connections
POST   /api/v1/connections
GET    /api/v1/connections/{connectionId}
PUT    /api/v1/connections/{connectionId}
DELETE /api/v1/connections/{connectionId}
POST   /api/v1/connections/{connectionId}/connect
POST   /api/v1/connections/{connectionId}/disconnect
POST   /api/v1/connections/{connectionId}/test

POST   /api/v1/connections/{connectionId}/terminal
POST   /api/v1/connections/{connectionId}/remote-desktop
```

Do not expose secrets in response DTOs. Credential write requests must never echo secret values back.

Tauri commands call the same application-service methods.

---

# 17. Events

Add events such as:

```text
systemLocations.changed
connection.created
connection.updated
connection.statusChanged
connection.deleted
remoteTerminal.opened
remoteTerminal.closed
remoteDesktop.launchFailed
```

Remote filesystem directory updates continue through normal directory snapshot/delta events.

---

# 18. Testing requirements

## System locations

Test classification of cloud-backed locations, unknown-provider fallback, unavailable providers and disappearing mounted network volumes.

## Connections

Test profile serialization without secrets, credential resolution, lifecycle cleanup and structured validation failures.

## SFTP

Use an isolated SSH/SFTP fixture to test password/key auth, host-key first use/mismatch, list, upload, download, rename, mkdir, delete, cancellation, reconnect and Unicode paths.

## FTP/FTPS

Use isolated fixtures to test passive mode, TLS, listing, upload/download, rename/delete, connection loss and unsupported capabilities.

## Cross-provider transfer

Cover:

```text
local → SFTP
SFTP → local
local → FTP
FTP → local
SFTP → FTP
FTP → SFTP
```

Verify byte correctness, cancellation, conflicts, cleanup and progress.

## Remote desktop

Mock platform launch behavior; automated tests must not open real remote sessions.

---

# 19. Security requirements

Mandatory:

- no credentials in URLs;
- no plaintext secret persistence;
- no secret logging;
- SSH host-key verification;
- FTPS certificate validation;
- explicit insecure warning for plain FTP;
- least-privilege plugin access;
- connection/credential APIs never echo secrets;
- provider-safe path handling;
- browser/server mode applies authorization to remote connections.

For multi-user browser/server deployments, explicitly define which users may see/use which configured connections.

---

# 20. UI proposal

```text
FAVOURITES
  Home
  Downloads
  Development

CLOUD
  iCloud Drive
  OneDrive — Personal
  OneDrive — Work

LOCAL
  Macintosh HD
  External SSD

NETWORK
  NAS — Media
  Office Share

SERVERS
  Home Server              ●
  Web Hosting              ○
```

`CLOUD`, `LOCAL`, and mounted `NETWORK` entries are system locations. `SERVERS` contains saved application-managed connections.

SSH context actions:

```text
Browse Files
Open Terminal
Open Remote Desktop
Reconnect
Disconnect
Edit…
Remove
```

FTP context actions omit terminal/remote desktop unless separately configured.

---

# 21. Acceptance criteria for the architecture extension

1. Existing local-provider behavior is unchanged.
2. OS-exposed OneDrive/iCloud/etc. appear without connection profiles.
3. Mounted SMB/network volumes open through the local provider.
4. Remote connection secrets never appear in workspace/settings files or URLs.
5. SFTP browses/transfers through the same pane and operation interfaces as local files.
6. FTP/FTPS browses/transfers without frontend-specific copy logic.
7. Cross-provider transfers use the shared operation engine.
8. SSH terminal actions reuse SSH configuration without contaminating VFS.
9. External RDP/VNC launch is isolated behind a remote-desktop service.
10. Provider capabilities accurately expose unsupported remote semantics.
11. Remote change tracking does not assume native watching.
12. Browser/Axum and Tauri hosts retain the same application-service semantics.
13. Native OneDrive/native SMB remain optional future providers, not prerequisites for useful cloud/network access.
