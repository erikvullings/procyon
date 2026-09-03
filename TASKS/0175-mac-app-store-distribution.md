# 0175 Mac App Store distribution

Status: open
Priority: medium
Subsystem: desktop, platform
Depends on: 0063

## Context

Procyon can use Developer ID signing and notarization for direct macOS distribution, but that does
not make the app eligible for the Mac App Store. App Store distribution requires a separate
Mac App Distribution identity, installer identity, provisioning profile, sandboxed build, signed
`.pkg`, App Store Connect record, and review.

The principal engineering constraint is App Sandbox. Procyon currently operates as an unrestricted
dual-pane file manager: it discovers mounted volumes, restores native paths, and lets the local VFS
read and mutate arbitrary locations. A Mac App Store build may access only its container and
explicitly entitled or user-authorized locations. Durable access to selected folders therefore
requires security-scoped bookmarks, which the repository does not currently implement. The
embedded local terminal also launches the user's shell and needs an explicit product/review decision
rather than being assumed compatible.

Implement this as a distinct Mac App Store distribution variant. Do not weaken or sandbox the
Developer ID build used for direct/Homebrew distribution.

## Acceptance Criteria

- A documented feature matrix defines differences between the Developer ID and Mac App Store
  variants, including local filesystem roots, mounted volumes, local/remote terminals, remote
  providers, plugins, external application launch, Finder integration, and self-update behavior.
- The Mac App Store variant uses the existing `nl.erikvullings.procyon` App ID, a Mac App
  Distribution certificate, a Mac Installer Distribution certificate, and a Mac App Store Connect
  provisioning profile. Credentials and profiles are injected only through the protected release
  environment and are never committed.
- The App Store build enables App Sandbox and carries only justified entitlements: application/team
  identifiers, user-selected read/write filesystem access, outbound network access, and inbound
  network access only if a retained OAuth callback listener requires it.
- A native folder-selection flow grants sandbox access. Security-scoped bookmarks are persisted,
  resolved before workspace restoration, kept active for the required access lifetime, refreshed
  when stale, and revoked or replaced explicitly. Denied, missing, moved, and stale locations
  produce actionable UI rather than empty panes or repeated unexplained prompts.
- Local listing, preview, editing, copy/move/delete, watching, search, metadata, and workspace
  restoration work inside authorized roots under a real sandboxed build. Attempts outside those
  roots fail safely and do not silently broaden access.
- The local embedded terminal is either proven compliant and functional under App Sandbox and App
  Review rules or disabled in the App Store variant with clear capability reporting. Remote
  terminals and remote providers are tested with the chosen network entitlements.
- Bundled restricted Lua plugins remain self-contained and require no JIT or unsigned executable
  memory entitlement. Runtime-downloaded executable plugins and a plugin marketplace are excluded
  unless separately reviewed against the current App Review Guidelines.
- Tauri has an App Store-specific configuration containing the category, entitlements, embedded
  provisioning profile, valid `CFBundleVersion`, and accurate
  `ITSAppUsesNonExemptEncryption` declaration. The ordinary Developer ID configuration remains
  unchanged.
- The protected workflow builds a universal sandboxed `.app`, signs it for App Store distribution,
  wraps it in a `.pkg` signed with the installer identity, validates signatures and entitlements,
  and uploads it to App Store Connect/TestFlight without exposing credentials.
- App Store Connect contains complete app privacy answers, privacy-policy and support URLs,
  category, age rating, description, screenshots, review contact/details, export-compliance
  answers for SSH/SFTP/FTPS/TLS, and agreements/tax/banking information when required.
- A TestFlight build is installed on a clean account and exercises authorized local folders,
  workspace restoration, file mutations, remote connections, and all intentionally disabled
  features before production review. Results and any App Review feedback are recorded in Agent
  Notes.
- Automated tests cover build-variant capability gating, bookmark persistence and stale-bookmark
  recovery, unauthorized-path failures, entitlement/profile configuration, and the protected
  packaging workflow. Pull-request builds remain credential-free.

## Implementation Notes

- Follow Apple's App Sandbox documentation:
  <https://developer.apple.com/documentation/security/app-sandbox> and current App Review
  Guidelines: <https://developer.apple.com/app-store/review/guidelines/>.
- Follow Tauri 2's maintained App Store path:
  <https://v2.tauri.app/distribute/app-store/>. Tauri builds the `.app`; the documented macOS path
  then uses `productbuild` to create the signed `.pkg` for App Store Connect.
- Keep security-scoped URL/bookmark handling in the macOS host/platform boundary. Do not leak native
  bookmark blobs or paths into provider-neutral DTOs, and do not add AppKit/Foundation dependencies
  to core crates.
- Grant access through explicit user selection and persist the minimum folder scopes needed by
  workspaces. Full Disk Access is not a substitute for App Sandbox authorization.
- Use `com.apple.security.network.client` for SFTP, FTP/FTPS, WebDAV, S3, OneDrive, and remote SSH.
  Add `com.apple.security.network.server` only if a tested local callback listener remains part of
  authentication in the App Store variant.
- The App Store submission does not need Developer ID notarization: App Store review performs the
  corresponding checks. Keep the existing Developer ID notarization path for direct distribution.
- Determine encryption export compliance from the actual SSH/TLS implementation and App Store
  Connect questionnaire; do not assume `ITSAppUsesNonExemptEncryption=false`.
- The current `mlua` configuration uses the interpreted `lua54` backend with `vendored`, not LuaJIT.
  Confirm the sandboxed hardened-runtime build does not request JIT-related entitlements.
- Treat arbitrary shell execution as a high-risk review item. Do not request broad temporary
  exception entitlements merely to preserve the local terminal.

## Agent Notes

- 2026-09-03: Created after comparing Procyon's desktop host and release configuration with current
  Apple and Tauri requirements. The largest gap is durable user-authorized filesystem access:
  `TASKS/0157-workspace-not-restored-and-tcc-reprompt.md` confirms the current non-sandboxed app has
  no security-scoped bookmark implementation. `apps/fm-desktop/src-tauri/src/terminal.rs` also
  spawns the user's local shell, requiring an explicit App Store variant decision. Bundled plugins
  use non-JIT Lua 5.4 and a restricted host API, which is a substantially safer starting point than
  downloadable native or JIT plugins.
