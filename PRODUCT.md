# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Procyon's primary users are technical power users who manage substantial collections of local,
remote, cloud, and archived files. They work for long periods in a file manager, value keyboard
control and information density, and need to move between locations and providers without changing
tools or workflows.

## Product Purpose

Procyon is a fast, keyboard-first dual-pane file manager for local, remote, cloud, and archive
storage. It brings navigation, transfer, search, comparison, synchronization, preview, editing,
terminal access, and automation into one consistent workspace.

Success means users can understand and manipulate large or distributed file collections quickly,
with predictable behavior and clear control over consequential operations.

## Positioning

Procyon is a modern, cross-platform successor to Total Commander that applies one keyboard-first
dual-pane workflow across local files, remote servers, cloud storage, and archives. Its provider
abstraction and shared operation engine make those locations behave as parts of one file-management
environment rather than separate integrations.

## Operating Context

- The primary experience is a dual-pane workspace with tabs, breadcrumbs, history, favourites,
  directory trees, filtering, search, and a command palette.
- Desktop users run the shared interface in Tauri on macOS or Windows. Browser/server mode uses the
  same interface through an Axum API and is suitable for managing files on a server or NAS.
- Users may work with very large directories and long-running copy, move, archive, delete,
  comparison, and synchronization jobs.
- Local, SFTP, FTP/FTPS, WebDAV, S3-compatible, OneDrive, mounted-volume, and archive locations are
  represented through provider abstractions.
- File inspection includes common text, image, media, document, spreadsheet, structured-data, and
  archive formats. An integrated editor and terminal support workflows that would otherwise require
  switching applications.

## Capabilities and Constraints

- The frontend is a shared Mithril and TypeScript application. It depends on a transport-neutral
  `FileManagerClient` and must behave consistently through mock, HTTP, and Tauri adapters.
- All filesystem mutations run through the typed Rust operation engine as cancellable, resumable
  jobs with explicit conflict handling. The frontend must never implement file mutations itself.
- Large directories require virtualization and responsive keyboard navigation.
- Filesystem access belongs behind VFS providers; new location types are providers rather than
  special cases in the application or interface.
- Shared state follows the existing Meiosis-style state tree. A second state-management system is
  not part of the product architecture.
- Plugins run in a restricted, resource-limited Lua sandbox and must not receive unrestricted
  filesystem or native access.
- Semantic search and document-understanding capabilities are optional and local to the managed
  semantic subsystem. They must not weaken filesystem authority, consent boundaries, ordinary
  startup, or the usefulness of the base file manager when unavailable.
- Browser/server deployments require authenticated, scoped access and deployment behind TLS outside
  loopback development.
- The project is open source under the MIT License.

## Brand Commitments

- The product name is **Procyon**.
- Product language is direct, compact, and operational rather than promotional inside the
  application.
- Marta establishes the bar for polish, fast keyboard navigation, workspaces, and quiet desktop
  density.
- Total Commander establishes the functional lineage: dual-pane navigation, broad file operations,
  comparison, synchronization, multi-rename, archives, and extensibility.
- muCommander demonstrates the cross-platform and remote-filesystem category, but Procyon must
  deliver substantially stronger performance and user experience.
- Existing raccoon identity assets are stored at `site/raccoon---face---bg.png` and
  `site/raccoon---body.png`.

## Evidence on Hand

- `README.md` contains the current public product description, capability matrix, installation
  guidance, and keyboard essentials.
- `file-manager-coding-agent-spec.md` is the authoritative product and engineering specification.
- `site/image-1.png` through `site/image-4.png` show the dual-pane browser, embedded terminal,
  document preview, and integrated editor.
- `site/index.html` contains the current public-facing product site and product claims.
- Architecture decisions and enforced boundaries are documented under `docs/architecture/` and
  `docs/decisions/`.
- The repository does not contain customer testimonials, case studies, adoption metrics, or
  performance comparisons that future work may present as external proof.

## Product Principles

1. **Optimize for expert flow.** Frequent navigation and file-management work should be fast,
   keyboard-efficient, information-dense, and suitable for long sessions.
2. **Make consequential operations safe and explicit.** Preserve user control through confirmation,
   conflict handling, cancellation, resumability, and truthful error states.
3. **Treat every provider as part of one file system experience.** Local, remote, cloud, and archive
   locations should share interaction models rather than becoming isolated feature silos.
4. **Keep hosts behaviorally equivalent.** Desktop and browser/server integrations may use
   different transports and native affordances, but shared capabilities must remain consistent.
5. **Add intelligence without taking authority.** Search, semantic processing, plugins, and
   automation may assist users, but authoritative state, access, consent, and mutations remain in
   the core application and filesystem boundaries.

## Accessibility & Inclusion

Procyon must support keyboard-only operation, visible focus, semantic roles, accessible labels,
screen-reader-friendly dialogs, correct modal focus trapping, reduced-motion preferences, adequate
contrast, scalable text, and status communication that does not rely on color alone. Virtualized
file tables must retain understandable focus and row semantics. Platform shortcuts use Command on
macOS and Control on Windows and Linux where the operating system permits.
