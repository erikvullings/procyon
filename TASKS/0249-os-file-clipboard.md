# 0249 OS file clipboard in the desktop app

Status: in_progress
Priority: medium
Subsystem: desktop
Depends on: 0048, 0058

## Context

Procyon's file clipboard (task 0048) is internal: Cmd/Ctrl+C in a pane cannot be pasted into
Finder or Explorer, and files copied there cannot be pasted into Procyon. User decision: "Yes,
separate PR: Cmd+C puts the files on the system clipboard in the desktop app".

## Design

- `PlatformAdapter::{write,read}_clipboard_files` (task 0058) gain real implementations and the
  `CLIPBOARD_FILE_REFERENCES` capability:
  - macOS (`fm-platform-macos::clipboard`): `NSPasteboard` file URLs; Finder `file:///.file/id=`
    reference URLs are resolved with `filePathURL`.
  - Windows (`fm-platform-windows::clipboard`): `CF_HDROP` (wide `DROPFILES`) plus
    `Preferred DropEffect` = `DROPEFFECT_COPY`.
  - Linux (`fm-desktop::linux_clipboard`, via `clipboard-rs`): `text/uri-list`,
    `x-special/gnome-copied-files` and a plain-text fallback. Best effort: X11/XWayland only; the
    clipboard context stays alive in-process so the selection remains owned.
- `fm-application::os_file_clipboard` owns policy; the Tauri commands `write_os_clipboard_files`
  and `read_os_clipboard_files` only delegate to `FileManagerService`.
- `RuntimeCapabilitiesDto.osFileClipboard` is true only in the Tauri host with the platform
  capability. `FileManagerClient.writeOsClipboardFiles`/`readOsClipboardFiles` exist on every
  adapter; the HTTP and mock adapters resolve `false`/`[]` without any request.
- Frontend `features/clipboard/os-file-clipboard.ts` mirrors every internal clipboard change to the
  OS and snapshots what it wrote. On paste, if the OS clipboard holds different file references
  (copied later in Finder/Explorer), they replace the internal clipboard for that paste as a copy.

## Decisions

- **Cut is published as copy.** The OS side never receives move semantics (no `DROPEFFECT_MOVE`,
  no `cut` in `gnome-copied-files`), so pasting in Finder/Explorer cannot delete Procyon's source.
  Move semantics remain internal to Procyon's own paste.
- **Only local entries reach the OS clipboard.** If any selected entry is non-local (SFTP, S3,
  archive members, …), nothing is written and the copy stays internal-only; nothing is downloaded.
- **Pastes from the OS are ordinary copy jobs** through the Rust operation engine with the usual
  conflict policy; a paste into the source folder becomes a duplicate. TypeScript never mutates
  files, symlinks are not followed, and nothing is overwritten silently.
- **Browser host limitation (parity note, spec rule 9).** A browser cannot read or write file
  references on the user's OS clipboard and the server's clipboard is not the user's, so the
  browser host reports `osFileClipboard: false` and keeps the internal clipboard behaviour.
- The context-menu **Paste** item's enabled state still reflects the internal clipboard only;
  Cmd/Ctrl+V (and the menu item when enabled) consult the OS clipboard at paste time.
- **The macOS Edit menu is app-dispatched, not responder-chain roles.** A menu item's key
  equivalent consumes Cmd+X/C/V/A before WKWebView delivers the keydown (verified with an
  instrumented `pnpm dev:tauri` build: Cmd+C never reached JS, even in the capture phase), so the
  role-based Copy item beeped with no text selection and the file clipboard never ran. The Edit
  items (`ui.edit.*`) now replay their chord as a keydown on the focused element
  (`replayEditCommand`), so pane, quick-filter and editor handlers see it exactly as in the
  browser. Only when no page handler claims it (text fields, plain text selections) does the
  thin `perform_native_edit_command` command send `cut:`/`copy:`/`paste:`/`selectAll:` down the
  responder chain, keeping native text editing. Windows (HTML menu) and Linux menus are unchanged.

## Acceptance Criteria

- [x] Copying local files in the desktop app writes OS file references (macOS tests against a
      scratch pasteboard; Windows payload tests; Linux encoding tests).
- [x] Non-local selections and unsupported hosts leave the OS clipboard untouched.
- [x] Cmd/Ctrl+V pastes newer Finder/Explorer file references through a copy job.
- [x] Browser and mock adapters report the capability unsupported and degrade gracefully.
- [x] macOS Cmd+C in a pane writes the file URL to the pasteboard (real `pnpm dev:tauri` build).
- [x] Manual macOS round-trip: Procyon → Finder and Finder → Procyon.
- [ ] Manual Windows (Explorer) and Linux (Nautilus/Dolphin, X11) round-trips.

## Agent Notes

- 2026-10-16 Copilot: Renumbered from 0247 to 0249 after merge; #125 had already taken 0247
  (media consent) and #128 took 0248. The user manually verified the macOS round trip in a
  `pnpm dev:tauri` build (Procyon → Finder, Finder → Procyon, Procyon pane → pane, and Cmd+C
  text copy in inputs) after the native Edit-menu fix. Windows and Linux manual round trips
  remain open.
