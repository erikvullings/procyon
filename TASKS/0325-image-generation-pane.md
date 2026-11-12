# 0325 Image generation pane

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0322

## Context
Image generation is a separate capability from chat: a different endpoint type (OpenAI-style images
API, or a local server such as ComfyUI or Automatic1111), different inputs, and an output the person
must review before anything is written. It gets its own role in 0322 and its own pane.

## Acceptance Criteria
- An "Image" role in Settings > LLM (AI) (0322), selecting an image-capable endpoint and model.
- A generation pane with two parts, side by side in a pane pair or stacked in one pane:
  - **Prompt**: prompt text, optional negative prompt, size, count, seed. A "Generate" button.
  - **Preview**: the generated images, each with "Save", "Discard" and "Regenerate with this prompt".
- Nothing is written to disk until "Save". Save asks for a folder and file name (name validated, no
  overwrite: conflict flow from 0045), and the saved file carries the prompt in its metadata where the
  format supports it.
- Generation is cancellable. Output size and count are bounded; very large images are downscaled for
  preview only.
- Cloud endpoints follow 0322's consent rules.
- Browser and Tauri hosts behave the same; provider calls stay in the backend.
- Tests: request bounds, save/no-overwrite, cancel, metadata embedding (where supported).

## Implementation Notes
- Backend: a generation capability next to `LlmProfileService`, with its own request and reply DTOs.
  Do not overload the chat role.
- Image bytes are returned through a short-lived backend store keyed by id, not inlined as JSON.
- Start with one endpoint type (OpenAI-style images API) and add local adapters as presets.

## Agent Notes
- 2026-11-12 claude: Created from design discussion. Not started.
