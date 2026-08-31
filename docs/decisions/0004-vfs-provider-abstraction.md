# 0004 VFS provider abstraction

Status: accepted

## Context
The file manager must eventually support more than the local filesystem (archives, and potentially
remote or virtual sources), while directory listing, metadata and operations code should not need
to know which kind of source it is talking to (spec §6, §16).

## Decision
`fm-vfs` defines a provider trait plus an explicit capabilities model (rule 10, spec §3): each
provider advertises what it supports (e.g. write, rename, watch) rather than the caller assuming
uniform behaviour across sources. `fm-vfs-local` is the first implementation, against the local
filesystem. Location parsing/normalization (task 0017) and directory listing (task 0018) are built
against the trait, not against `fm-vfs-local` directly, so a second provider can be added without
changing the application layer.

## Alternatives
- **Local-filesystem-only, no trait**: rejected — the spec requires archive browsing (§6) and the
  cost of introducing the abstraction later, once callers already assume local-path semantics,
  is much higher than defining it up front.
- **One trait method per operation with `Result<T, UnsupportedError>` returns instead of
  capabilities**: rejected — capabilities let the frontend disable/hide unsupported actions before
  the user attempts them, rather than discovering support by failing a call.
- **Trait with default methods that panic for unsupported operations**: rejected — violates "avoid
  unsafe filesystem assumptions" (spec §35) and would surface as a runtime crash instead of a
  checked capability.

## Consequences
- Every new provider must honestly declare its capability set; over-declaring causes runtime
  failures, under-declaring hides working functionality from the UI.
- Application-layer code that calls providers must check capabilities before invoking
  capability-gated operations rather than relying on try/catch.
- The archive provider (`fm-archive`) and any future remote provider slot into the same trait
  without changes to `fm-application`'s directory-listing or navigation logic.

## Revisit conditions
Revisit if a future provider needs a capability the current trait cannot express (e.g. partial
writes, streaming ranges), or if capability checks prove insufficient and providers need
richer feature negotiation.
