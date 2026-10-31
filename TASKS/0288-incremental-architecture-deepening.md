# 0288 Incremental architecture deepening

Status: open
Priority: medium
Subsystem: architecture
Depends on: none

## Context

The `improve-codebase-architecture` skill identified six deepening opportunities across the Procyon codebase. This parent task tracks the work to address them. Each suggestion has its own subtask with a concrete acceptance criterion.

The six candidates are:

1. **Giant `FileManagerClient` interface** — decompose the 100+ method god interface into focused sub-interfaces
2. **`app-shell.ts` wiring god-file** — extract feature wiring into dedicated modules
3. **`knowledge-search-dialog.ts` monolith** — split the 3234-line semantic dialog into focused sub-modules
4. **Rust pass-through mapping modules** — fold 15+ thin mapping files into their owning capability modules
5. **`FileManagerService` composition root** — reduce the 8231-line service from 95 public methods
6. **Operations controller leaky seam** — move confirmation/guard helpers inside the controller

## Acceptance Criteria

- Each subtask has a concrete, verifiable success condition
- Subtasks can be done independently of each other (no cross-dependencies)
- Progress is visible in both task files and TASKS/README.md

## Implementation Notes

- The suggestions come from the `improve-codebase-architecture` skill process (skills/improve-codebase-architecture/SKILL.md)
- Each subtask should follow the project's coding conventions in AGENTS.md
- Subtasks should produce tests where applicable (spec §35)
- Report incomplete or platform-untested behaviour explicitly in Agent Notes

## Agent Notes

- Parent task created from `improve-codebase-architecture` skill analysis
- Subtasks are independent and can be done in any order
- Subtasks 0289 and 0290 (frontend) are good starting points as they have the clearest friction
