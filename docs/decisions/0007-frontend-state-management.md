# 0007 Frontend state management

Status: accepted

## Context
The Mithril frontend needs shared, predictable state (workspaces, panes, selection, settings,
event-stream status) without adopting a general-purpose state framework the spec doesn't otherwise
require (spec §13 frontend architecture, §35 "avoid speculative abstractions").

## Decision
The frontend uses a small explicit state model in the Meiosis style: a single state tree, actions
that return patches, and `m.stream`-based composition — not a generic third-party state framework
(Redux, MobX, Pinia-style stores, etc.). State updates are patch objects (Mergerino-style) applied
to the tree, kept intentionally minimal and specific to this app's needs (workspaces, navigation,
selection, settings, connection status).

## Alternatives
- **A general-purpose state management library**: rejected — spec §35 warns against speculative
  abstractions with no demonstrated need, and AGENTS.md explicitly forbids "a generic state
  framework without a demonstrated need"; Meiosis-style patches are enough for this app's state
  shape.
- **Component-local state only, prop-drilled**: rejected — workspaces/panes/selection are shared
  across sibling components (two-pane layout, status bar, command palette) in ways local state
  can't cleanly express.
- **Framework migration (React/Vue/Svelte/Angular) purely for its state story**: rejected —
  AGENTS.md forbids adding a different UI framework, and Mithril plus an explicit state model
  already covers the requirement.

## Consequences
- All shared UI state flows through one state tree and one patch-producing action layer, making it
  straightforward to log, time-travel-debug, or unit test actions independently of components.
- Components must not hold shared state locally; anything more than one component needs belongs in
  the state tree.
- New features that seem to need a "store" should first be expressed as new state-tree slices and
  actions, not as a reason to add a second state mechanism.

## Revisit conditions
Revisit if the state tree grows large enough that patch-based updates become a performance problem
(e.g. very large directory listings held directly in state rather than referenced), or if a real
requirement (e.g. cross-tab sync) needs machinery this minimal model doesn't provide.
