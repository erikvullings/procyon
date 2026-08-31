# 0167 Declarative automation recipes

Status: open
Priority: medium
Subsystem: backend, frontend, actions
Depends on: 0035, 0049, 0051

## Context

Procyon has a typed action registry and operation engine but cannot compose repeated workflows such
as rename then move then checksum, extract then organize, or synchronize then verify. Recipes should
orchestrate existing capabilities without becoming an unrestricted scripting escape hatch.

## Acceptance Criteria

- Users can create, validate, preview, run, duplicate, export, import, and delete named recipes made
  from an allowlisted set of typed action/operation steps.
- Recipe inputs can reference the active selection, cursor entry, active/opposite pane locations, and
  explicit prompted parameters through a versioned schema.
- A preflight view resolves every step, capability, destination, and required confirmation before the
  first mutation starts.
- Mutating steps run through the operation engine and preserve each job's conflict, cancellation,
  progress, and audit behavior.
- Failure policy is explicit (stop, continue for independent items, or request intervention); there
  is no claimed transactional rollback unless 0160 can safely provide it.
- Imported recipes cannot execute arbitrary shell commands, access credentials, or bypass plugin and
  provider permissions.
- Tests cover schema migration, parameter resolution, unavailable actions, cancellation, partial
  failure, confirmation boundaries, malicious imports, and HTTP/Tauri parity.

## Implementation Notes

- Store declarative action identifiers and typed parameters, never code strings or serialized
  closures.
- Reuse command-palette parameter metadata where it is sufficiently typed; deepen the registry rather
  than creating recipe-only action descriptions.
- Scheduling recipes is out of scope until execution and recovery semantics are proven.

## Agent Notes

- 2026-08-28: Created from the product feature review. Safety and inspectability are intentional
  constraints; this is not a general shell macro facility.
