# 0330 Cross-platform color tags with tag virtual folders

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0136, 0162

## Context
Finder tags (0136) are macOS-only: they live in the `kMDItemFinderComment`/`com.apple.metadata:_kMDItemUserTags` xattrs, and Windows/Linux report the capability `false`. There is no cross-platform color label. Requested by the user, who especially wants tags to act as **virtual folders**: a tag view that groups files from any directory, the way smart folders (0162) and basket (0238) already do.

Scope is a Procyon-owned tag store, not file xattrs, so tags work identically on macOS, Windows, Linux and remote/archive locations where xattrs are unavailable.

## Acceptance Criteria
- Tag any file or folder with one of a fixed color palette (and optionally a name) from the context menu, Properties, and the command palette.
- Tags persist in a Procyon-owned store (SQLite or versioned JSON, per the settings/operation-history pattern) keyed by stable entry identity, and survive restarts.
- Sidebar lists tags; selecting one opens a virtual folder listing every tagged entry across directories, with source location shown.
- Tag virtual folders are workspace-restorable like smart folders (0162) and do not snapshot entry arrays.
- Removing or moving a file updates or clears its tag; missing entries show as unavailable, not errors.
- Behaves the same in browser and Tauri hosts (preserve parity).
- Interaction with 0136/0145: when macOS Finder tags exist, decide and document whether they are imported, mirrored, or kept separate. Default: kept separate, no silent writes to xattrs.

## Implementation Notes
- Reuse the virtual-folder plumbing from 0162 and 0238 rather than a new listing path.
- Stable identity: check how 0162 and 0238 key entries before choosing the key (path alone breaks on moves).
- Consider exposing tags as a multi-rename input (0267 already reads tag fields).

## Agent Notes
- Created from user request; nothing investigated yet.
