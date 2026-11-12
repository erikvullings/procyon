# 0327 Voice push-to-talk commands

Status: open
Priority: low
Subsystem: backend, frontend, desktop
Depends on: 0324

## Context
Hands-free file management is useful but a misheard command in a file manager can do damage. Voice
should map speech to a fixed set of intents that already exist as commands, never to free-form
execution. Speech recognition runs locally.

## Acceptance Criteria
- Push-to-talk: a hotkey (default chosen so it does not conflict with existing shortcuts) records
  while held; releasing it transcribes and shows the recognized command before running it.
- Recognized text is matched against a whitelist of intents: open folder or file, go to a location,
  search for a term, select, copy, move, and toggle view options. Anything else is shown as text in
  the chat (0324) and is not executed.
- Destructive or file-changing intents (move, delete, rename) always show a confirmation with the
  affected items before running. This cannot be disabled.
- Speech-to-text is local (a bundled whisper.cpp-class model), selected in Settings, downloaded only
  with the person's consent. No audio leaves the machine.
- Microphone permission is requested only when push-to-talk is first used; if denied, the feature
  disables itself with a message.
- Desktop host only in this task; the browser host shows the feature as unavailable.
- Tests: intent matching (including near-misses), confirmation gating, unavailable-host behaviour.
  Recognition accuracy is measured separately, not in unit tests.

## Implementation Notes
- Recognizer behind a trait in the backend, with a fake for tests, following the semantic worker
  boundary pattern. Model management follows the component catalog approach (0194).
- Intent matching is deterministic (keywords and simple patterns), not an LLM call, for predictable
  behaviour.
- Open decision: text-to-speech replies. Recommendation: out of scope for this task.

## Agent Notes
- 2026-11-12 claude: Created from design discussion. Not started.
