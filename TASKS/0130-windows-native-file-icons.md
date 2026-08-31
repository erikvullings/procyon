# 0130 Windows native file icon extraction

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: platform
Depends on: 0060, 0091

## Context
Split out of 0060 ("Windows platform integration"). That task's acceptance criteria listed shell
icons as in scope for `fm-platform-windows`, but the Agent Notes record they were **not**
delivered: `PlatformAdapter::file_icon` on Windows still delegates to the fallback adapter
(`Unsupported`), because extracting a real shell icon needs an `HICON`/
`IShellItemImageFactory` bitmap re-encoded as PNG, and the workspace had no image-encoder
dependency at the time. Task 0060's own text now flags this as still open.

This is purely additive: 0091 ("Native file icon overlay") already built the whole
backend-served-icon-overlay pipeline (`GET /api/v1/icons?uri=...`, capability gating via
`RuntimeCapabilitiesDto.nativeFileIcons`, per-extension caching) against macOS (0059), and
explicitly designed the route so an `Unsupported` result cleanly falls back to the themed glyph
(0085/0092) on hosts without native icons — which is exactly what Windows does today. Nothing in
the overlay pipeline, the frontend, or the Catppuccin icon theme needs to change; this task only
implements `file_icon` for `crates/fm-platform-windows/src/lib.rs` and flips
`nativeFileIcons` on for the Windows capability bits.

## Acceptance Criteria
- `fm-platform-windows`'s `PlatformAdapter::file_icon(&self, path: &Path) -> Result<Vec<u8>,
  PlatformError>` returns real shell icon bytes (PNG) instead of delegating to the fallback
  `Unsupported`.
- Icon extraction is cached by extension (one lookup per extension, not per file — spec §28),
  following the same shape as the existing macOS cache
  (`Mutex<HashMap<String, Vec<u8>>>` keyed by `icon_cache_key(path)` in
  `crates/fm-platform-macos/src/lib.rs`) — do not add a second cache layer elsewhere.
- Directories and extension-less files get a sensible generic icon rather than an error.
- Windows platform capability bits report `nativeFileIcons: true` once implemented.
- Tests: icon bytes are non-empty and decodable as PNG for a representative set of extensions
  (including directories and an extension-less file); cache is exercised (second call for the same
  extension does not re-invoke the shell API — inject/mock as needed, matching the macOS test
  approach).
- Manually verified on Windows; the task notes record the OS version tested (§35).

## Implementation Notes
- Use `IShellItemImageFactory` (preferred, DPI-aware) or `SHGetFileInfoW` with `SHGFI_ICON` as a
  fallback, via the `windows` crate (already a dependency of `fm-platform-windows`).
- Converting the resulting `HICON`/`HBITMAP` to PNG needs an image encoder; none is currently a
  workspace dependency — evaluate `image` (already a strong candidate given its ubiquity) versus a
  minimal hand-rolled PNG encoder to avoid pulling in a large dependency tree for one call site.
- Strip the `\\?\` extended-length prefix before calling shell APIs, matching every other native
  call in this crate (they reject that form).
- Thumbnails remain explicitly out of scope (declared unimplemented capability per 0060).

## Agent Notes
- Implemented on the Windows adapter with `SHGetFileInfoW(SHGFI_ICON | SHGFI_LARGEICON)`, GDI
  bitmap extraction, and a small dependency-free PNG encoder. Extended-length paths are stripped
  before the shell lookup, matching the other Windows native calls.
- File icons are cached by lowercased extension, with separate directory and extension-less
  sentinels. `PlatformCapabilities::FILE_ICONS` is now reported by the Windows adapter; thumbnails
  remain unsupported.
- Tests cover real non-empty PNG output for a text file, an extension-less file, and a directory,
  plus an injected cache test proving a second lookup for a case-variant extension does not invoke
  the fetch function again.
- Verified on Windows 11 with `cargo test -p fm-platform-windows` and
  `cargo clippy --workspace --all-targets -- -D warnings`. The full `cargo test --workspace`
  run reaches one unrelated existing failure in
  `fm-plugin-runtime::tests::discovers_the_real_catppuccin_icons_plugin_package`; the focused
  Windows platform suite remains green.
