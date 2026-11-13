# 0325 Image generation pane

Status: done (OpenAI-style images endpoint only; local adapters such as ComfyUI/Automatic1111 remain
presets to add. Neither host's pane has been checked visually.)
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
- Implemented:
  - `crates/fm-application/src/image_generation.rs` validates the request (prompt and negative-prompt
    length, edges 64..=2048 in multiples of 8, at most 2 097 152 pixels, 1..=4 images, seed up to
    2^32-1), holds finished batches for 30 minutes and at most 128 MiB, and serves preview or original
    bytes by id. Previews are downscaled to a 1024 px edge for display only; a JPEG preview stays a
    JPEG and an unsupported output format is refused.
  - `crates/fm-application/src/image_generation_transport.rs` speaks the OpenAI-style images API
    (`b64_json` replies), reads a bounded response, honours cancellation, and never carries the key in
    its `Debug` text.
  - `LlmRole::Image` and `LlmRoles.image` in `llm_profiles.rs`: the image role resolves its own
    endpoint, model, auth header, timeout (>= 120 s) and images URL, and a cloud endpoint is refused
    unless cloud use is allowed for that role (`llmCloudImageNotAllowed`), as embeddings already are.
  - Save (`ImageGenerationService::save`) is the only path that writes: it validates the name, requires
    the stored extension, requires provider `WRITE`, and passes `overwrite` through, so an existing
    file maps to `DestinationAlreadyExists` and the 0045 conflict flow. Prompt metadata is embedded at
    save time - PNG `tEXt`/`iTXt` chunks after `IHDR`; JPEG and WebP through `fm_metadata`'s edit path -
    and a format that cannot carry it reports `prompt_embedded: false`.
  - Hosts: `POST /api/v1/images/generate|cancel|save|discard` and `GET /api/v1/images/bytes` in
    `fm-server` (bytes are served as a response body, never as JSON), and matching Tauri commands. The
    save route enforces the accessible roots.
  - Frontend: `frontend/src/features/image-generation/` renders the two sides - prompt (subject, style
    presets, size, count, seed, negative prompt, lens and aperture for photographic presets, and the
    composed prompt that will be sent) and preview (thumbnails, large view, Save, Discard, Regenerate).
    `frontend/src/app/image-wiring.ts` drives it; the mock host mirrors bounds, consent, cancellation and
    the no-overwrite rule so both hosts behave the same.
  - Settings > LLM (AI) now exposes the Image role in the advanced view, with the cloud permission and
    consent handled as they already are for embeddings.
  - Tests cover request bounds, preview downscaling, save and no-overwrite, metadata embedding for PNG
    and JPEG, cancellation, transport request shape and redaction, the role gate, and the mock host.
