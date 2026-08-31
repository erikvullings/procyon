# 0110 Native OneDrive provider

Status: done
Priority: high
Subsystem: backend
Depends on: 0103, 0108, 0109
Owner: Erik Vullings
Agent: GitHub Copilot CLI

## Context
Provide direct Microsoft Graph access without the installed sync client or an OS filesystem
representation. Task 0101 remains the simpler path for accounts exposed by the OS, but it does not
cover managed work accounts whose tenant policy prevents OneDrive folders on macOS while allowing
browser access.

Both Microsoft personal accounts (including Microsoft 365 Family members) and Microsoft Entra work
or school accounts are in scope. Direct access must respect tenant consent and Conditional Access;
Procyon must surface policy rejection and must never attempt to bypass it.

## Acceptance Criteria
- OAuth 2.0 Authorization Code with PKCE works through the system browser for public desktop clients
  without embedding a client secret.
- OAuth authorization/token refresh works for both personal Microsoft accounts and Microsoft Entra
  work or school accounts without exposing secrets.
- Users authorize an existing Microsoft account through the system browser; Procyon does not create
  a separate user account or collect Microsoft credentials.
- Refresh tokens use `CredentialStore` and rotated refresh tokens replace their predecessors.
- Personal OneDrive and OneDrive for Business browsing, streaming download/upload, paging,
  throttling/backoff and resumable upload work.
- Multiple saved personal and organizational accounts remain distinct and can be used concurrently.
- Each authorized account appears as a distinct navigable entry in Favourites under `CLOUD`, opening
  its virtual `onedrive://<connection-id>/` root without creating or storing a local OneDrive folder.
- OneDrive tabs use the saved account name at the drive root, retain a connection icon in
  subfolders, and never expose the opaque connection id in breadcrumbs.
- Tenant consent, insufficient-scope and Conditional Access failures are surfaced as actionable
  authorization errors.
- Change/delta tracking plugs into the generalized change-tracking abstraction.
- Locations reference saved accounts/connections and never include tokens.
- Transfers participate in the shared operation engine.
- Every new OneDrive label, status, action, error and authorization message uses the existing i18n
  system; no user-facing string is hard-coded in a component.
- Tests mock or safely fixture provider API behavior.

## Implementation Notes
- Suggested crates: `fm-auth-oauth`, `fm-vfs-onedrive`.
- Use the Microsoft identity platform `common` authority with delegated `offline_access`,
  `Files.ReadWrite`, and `User.Read` scopes so one public-client registration supports personal and
  organizational accounts.
- Procyon public-client application ID: `9b01b729-5908-492b-bcd1-32b4a36096de`. The registration is
  owned by the Procyon-controlled `ERIKVULLINGSGMAIL.ONMICROSOFT.COM` tenant and uses
  `http://localhost` as its native-client redirect URI. This identifier is public configuration;
  never add a client secret for this desktop application.
- OneDrive for Business's default `/me/drive` is in scope; browsing arbitrary SharePoint sites and
  document libraries remains follow-up scope.
- Use opaque Graph paging and delta links verbatim. Honour `Retry-After` for throttling, and never
  forward bearer tokens to pre-authenticated download or upload URLs.
- Do not duplicate 0101's OS-exposed OneDrive path.

## Agent Notes
- Before starting, verify there is a real product requirement for direct API access; otherwise keep this open.
- 2026-08-30 GitHub Copilot CLI: Refined connection presentation after live OneDrive testing. The
  root tab now shows the saved account name instead of its UUID, all OneDrive tabs carry a plug icon,
  and breadcrumbs expose only `onedrive://` plus the drive-relative path. Clicking the provider
  breadcrumb opens the canonical drive root. Newly created OneDrive profiles open automatically
  after Microsoft authorization succeeds rather than trying to browse an unauthorised root.
- 2026-08-29 GitHub Copilot CLI: Unfroze 0110 after confirming the concrete requirement: a managed
  work OneDrive is browser-accessible on macOS but prohibited from creating an OS-mounted sync
  folder. Expanded the contract to personal and Entra organizational accounts, including explicit
  tenant-policy handling. Microsoft Graph implementation research confirmed the `common` authority,
  Authorization Code + PKCE for a public desktop client, delegated `offline_access Files.ReadWrite
  User.Read`, `/me/drive` compatibility for personal and business drives, refresh-token rotation,
  opaque paging/delta links, `Retry-After`, and pre-authenticated transfer URL constraints.
- 2026-08-29 GitHub Copilot CLI: Blocked before implementation by product identity setup. Procyon
  needs a maintainer-owned Microsoft Entra public-client app registration configured for accounts
  in any organizational directory plus personal Microsoft accounts, with a localhost Mobile and
  desktop redirect URI. Record its application (client) ID in this task when available; it is
  public configuration, not a secret. The user chose not to introduce a temporary per-developer
  client-ID setting. Requested delegated scopes remain `offline_access Files.ReadWrite User.Read`;
  organizational tenants may still require administrator consent or reject access through
  Conditional Access.
- 2026-08-29 GitHub Copilot CLI: Registration blocker resolved. Created the multi-tenant `Procyon`
  public/native application in the maintainer-owned default directory, enabled personal Microsoft
  accounts plus all Entra tenants, configured `http://localhost`, and added delegated Microsoft
  Graph permissions `Files.ReadWrite`, `offline_access`, and `User.Read`. No client secret was
  created. Application (client) ID: `9b01b729-5908-492b-bcd1-32b4a36096de`.
- 2026-08-29 GitHub Copilot CLI: Implemented the complete native provider and host integration.
  Added Authorization Code + S256 PKCE, bounded loopback callbacks, refresh-token rotation through
  `CredentialStore`, Continuous Access Evaluation (`cp1`) claims replay, redacted actionable policy
  failures, serialized refresh, and cancellable/expiring authorization attempts. Added the
  `onedrive://<connection-id>/` Graph provider with personal/Business default-drive browsing,
  opaque paging and delta cursors, retry/throttling, preauthenticated downloads, ranged reads,
  recycle-bin deletion, and bounded simple/resumable uploads. The operation planner now supplies a
  fresh known size only to destinations advertising resumable upload.
- 2026-08-29 GitHub Copilot CLI: Added matching HTTP, Tauri and deterministic mock authorization
  clients. The browser and desktop hosts open only the trusted Microsoft `common` authorization
  endpoint in the system browser; the frontend polls/cancels backend-owned attempts and never sees
  an authorization code or token. The connection manager now shows localized sign-in progress,
  personal/work account identity, reauthorization, cancellation, and actionable tenant consent,
  insufficient-scope and Conditional Access errors. Authorized accounts are distinct entries under
  Favourites and the native Go menu's `CLOUD` group, opening their virtual root without creating a
  local folder. Added matching English and Dutch catalogue entries.
- 2026-08-29 GitHub Copilot CLI: Verified 1,617 Rust tests plus workspace doctests, 1,569 frontend
  tests serially, 40 script tests, frontend typecheck, deterministic OpenAPI/Orval generation,
  desktop compilation with the scoped Tauri opener capability, rustfmt, workspace clippy with
  warnings denied, and Biome (three pre-existing CSS specificity warnings only). The default
  parallel frontend run still intermittently times out one unrelated existing type-to-select test;
  that test passes alone and the complete suite passes with one worker. Microsoft Graph/OAuth
  behavior is fixture-tested for personal and Business drives; a live managed-tenant sign-in and
  tenant-specific Conditional Access policy remain environment-dependent and were not exercised in
  automated validation.
