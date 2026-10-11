# 0243 Generation profile UX and model limits

Status: done
Priority: medium
Subsystem: frontend, backend, settings
Depends on: 0184

## Context

Generation profiles are not self-explanatory. The original 8k context and 1k output defaults can
truncate substantial summaries, and the numeric steppers overlap the input border. Users also need
an mlx-serve provider preset and a simpler way to choose supported token budgets.

## Acceptance Criteria

- Explain what generation profiles do and when local or cloud content is sent, in localized settings
  copy; semantic search remains usable without one.
- Add an mlx-serve preset using its actual compatible chat-completions URL convention.
- Keep numeric controls inside their borders at the compact settings row height.
- Use model-advertised context and output limits when discoverable, with editable capped token
  presets and clear safe fallback for unknown limits; never promise unbounded generation past model
  or backend limits.
- Do not silently change saved profiles or weaken runtime generation safety.
- Preserve HTTP, Tauri, and mock behavior; cover relevant frontend and backend cases.

## Implementation Notes

- Use the existing `fm-application` LLM capability, transport DTOs and generated frontend API
  rather than a host-specific path. Regenerate generated files rather than hand-editing them.
- Task 0184 defines preset, profile, secret, consent, and discovery boundaries.

## Agent Notes

- 2026-10-11: Started as an independent follow-up to generation-profile usability feedback; not a
  release blocker. Treat reported limits as advisory until corroborated and enforce backend bounds.
- 2026-10-11: Renamed the settings surface to AI summaries and answers with explanatory EN/NL
  copy; added mlx-serve on its OpenAI-compatible port 11234. `fm-application` discovers positive
  model-advertised limits without retaining prompts, while HTTP/Tauri expose the same nullable DTO
  and the mock simulates known/unknown cases. `llm-profile-management.ts` suggests reported caps
  only for unsaved, untouched fields, retains existing profile values, resets automatic suggestions
  to safe preset defaults for unknown models, and offers bounded token choices plus custom numeric
  entry. Backend hard caps remain 1,048,576 context and 16,384 answer tokens; reported limits are
  advisory and runtime safety still applies. The compact numeric spinner in
  `mithril-materialized-procyon.css` now sits inside its input border. Updated
  `docs/semantic-operations.md`, tests in the affected Rust and frontend files, and generated HTTP
  artifacts. Focused Rust tests, frontend suite/build, full lint and browser geometry checks pass.
