# 0325 Image generation pane

Status: done (two images contracts: the OpenAI-style API and Alibaba Cloud Model Studio's async task
API behind `qwen-image`/`wan*`. Local adapters such as ComfyUI/Automatic1111 remain presets to add.
The browser host's pane has been walked through in the mock build and its layout measured; the Tauri
host has not been seen, no image has been generated against a live endpoint, and the Model Studio path
has not been run against the real service.)
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
  - `crates/fm-application/src/image_generation_transport.rs` speaks two contracts, picked by what the
    endpoint's profile advertises: the OpenAI-style images API (`b64_json` replies) and Alibaba Cloud
    Model Studio's asynchronous task API, where a submit creates a task (`X-DashScope-Async: enable`)
    that is polled at `{base}/api/v1/tasks/{task_id}` until it hands back image URLs. Both read bounded
    responses, honour cancellation, and never carry the key in their `Debug` text. The task reply's URLs
    are only fetched when they stay on the endpoint's own service (same scheme, same host or a sibling
    of its registered domain, which is how Model Studio serves results from object storage) and say
    `image/*`; the prompt is submitted with `prompt_extend: false` and `watermark: false`, because the
    person already reviewed the wording and the saved file is theirs. `LlmPreset::ModelStudio` offers it
    as a preset on both hosts.
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
  - Frontend: `frontend/src/features/image-generation/` is laid out like the chat pane rather than as two
    documents side by side: a header row (previous generations as crumbs, new prompt, new tab,
    favourites, and a gear that opens Settings at LLM (AI)), the preview above, the prompt composer
    below at full width, and a status row holding the style multi-select, size, lens, aperture and seed.
    There is no image-count choice: one image per generation, matching how the pane is used.
    `frontend/src/app/image-wiring.ts` drives it; the mock host mirrors bounds, consent, cancellation and
    the no-overwrite rule so both hosts behave the same.
  - Settings > LLM (AI) now exposes the Image role in the advanced view, with the cloud permission and
    consent handled as they already are for embeddings.
  - A profile no longer has to advertise the chat completions API to be saved - an images-only endpoint
    is legitimate - but binding one to the chat role is refused where the person can still act on it,
    both in `set_role` and again in `resolve_role` for bindings saved before the rule existed.
  - Tests cover request bounds, preview downscaling, save and no-overwrite, metadata embedding for PNG
    and JPEG, cancellation, transport request shape and redaction for both contracts (including the
    poll-to-ready path, a failed task, and the refusal to fetch a URL off the endpoint's own host), the
    role gate, the contract the capabilities select, and the mock host.
  - The style picker folds ten presets behind a count (`Style · 2`) because the status row has no room to
    name them: eight `medium` presets are mutually exclusive, `black and white` is a `treatment` that
    rides along with one, and the checkbox rows show labels alone - never the icon name beside them.
    `togglePreset`/`presetsAreCompatible` in `image-presets.ts` own that rule.
  - Generations are kept in `entry.history` (at most `MAX_GENERATION_CRUMBS`) so the crumb bar can show
    earlier ones; `selectGeneration` returns to one, `newPrompt` clears the composer, and `discard`/
    `forget` drop them from the store as well.
  - `LlmPreset::QwenImage` ("Qwen Image (Alibaba Cloud)", `qwen-image` filled in) lets the image role be
    bound from the preset list without typing a model id; `wan2.1` and the other Model Studio image
    models are reached by editing the same model field.
  - The pane is opened from the pane overflow menu next to Chat (`photoIcon`), the way the chat pane is.
  - 2026-11-13, browser host (mock build): the header icons, empty preview, composer, status row and the
    style popup were checked in Chrome at 1728x998. Two fixes came out of it - the status-row select and
    seed field were being stretched to the full pane width by Materialize's `select`/`input` rules (they
    now carry `browser-default` on the field only, with the row's own rules scoped under `.fm-image-status`
    to outweigh the shell's `select.browser-default { width: 100% }`), and no reachable command existed to
    open the pane at all.
  - 2026-11-13, second pass from the person's own look at the pane: the three status choices became
    drawn controls (a chrome-density trigger with a caret plus a `role=listbox` list that hangs above
    the row, since the row is the bottom of the pane). A native `select` is not usable here - Materialize
    hides every one it was not told about, and the field it substitutes is a form control several times
    the row's height. Lens and aperture gained readable labels (`85mm portrait`, `f/1.8 wide open`)
    because the ids are slugs.
  - The composer now says nothing under an empty prompt: the "Nothing yet - describe what you want to
    see." line and the red "Describe what you want to see first." were both removed, since a disabled
    send key already says a subject is missing and failures reach the preview's alert. The wording that
    will be sent still appears once there are words, because the presets add words the person did not
    type. The leave-out field is named on its left and its placeholder is marked as an example, so
    "text, watermark" is not read as a feature of its own. The send key is the Enter glyph, as in chat.
  - Still unverified: a live generation end to end. Driving the mock settings UI saved the Qwen profile and
    showed the Image row in Roles, but the role never stuck, and the pane stayed on its "No image model is
    set" state - which is itself covered by wiring tests, not by a real reply.
