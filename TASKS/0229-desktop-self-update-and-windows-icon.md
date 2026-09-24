# 0229 Desktop self-update and Windows icon

Status: done
Priority: high
Subsystem: desktop, settings, release
Depends on: 0015, 0030, 0063

## Context

The Windows taskbar renders Procyon's icon visibly smaller than neighboring applications because
the ICO inherited generous transparent padding from the cross-platform source. Desktop releases
also require users to discover and install every new version manually.

## Acceptance Criteria

- The Windows ICO uses a tighter, dedicated source without changing the macOS/Linux icon framing,
  includes 16, 24, 32, 48, 64, and 256 pixel frames, and has an automated occupancy check.
- Tauri checks a stable HTTPS endpoint for signed updates and rejects unsigned or incorrectly
  signed artifacts through the official updater plugin.
- Automatic checks are enabled by default and can be disabled in Settings. A manual check remains
  available. Neither path downloads, installs, nor restarts without explicit user confirmation.
- Update availability, download progress, success/restart, unsupported-host, and failure states are
  visible. Browser and mock hosts fail explicitly rather than pretending to update.
- Release jobs sign and upload macOS, Windows, and Linux updater artifacts and publish one complete
  `latest.json` only after all supported platform jobs succeed.
- Pull-request desktop builds continue to work without release signing credentials.

## Agent Notes

- 2026-09-24 Copilot: Added Tauri updater/process plugins, a runtime-neutral frontend update
  boundary, startup confirmation dialog, Settings controls, durable schema migration, and
  localized copy.
- 2026-09-24 Copilot: Generated a dedicated Windows icon source with 95.3% canvas occupancy,
  regenerated the six-frame ICO, and added a packaging regression test.
- 2026-09-24 Copilot: Provisioned a permanent encrypted updater key in macOS Keychain, configured
  the private key/password as GitHub Actions secrets, and compiled only the public key into the
  application.
- 2026-09-24 Copilot: Added signed updater artifacts and a fail-closed stable manifest publication
  job. First real install and Windows taskbar appearance remain release/platform smoke checks.
