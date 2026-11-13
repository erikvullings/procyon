# 0329 Paste images into the chat

Status: implemented (uncommitted); browser check of the paste chip still open
Priority: low
Subsystem: backend, frontend
Depends on: 0324, 0325

## Context
The chat tab (0324) takes text only. Pasting an image should attach it to the next question, but only
when the selected model can read images. Today the transport can carry images only on the Ollama
native generation path: `generate_chat` returns `InvalidConfiguration` for images on OpenAI-style
endpoints (`crates/fm-application/src/llm_profiles.rs`, around lines 630 and 669). The chat service
also renders the whole conversation into one prompt string (`crates/fm-application/src/chat.rs`,
`build_prompts`), so an image has no message slot yet. The summary path (document-summary dialog,
`include images`) already resizes and bounds images, and `mithril-materialized` ships `MaterialBox`
for the viewer.

## Decisions (taken)
1. **Endpoints:** both Ollama native and OpenAI-style (`image_url` data URLs in a content array).
2. **Capability source:** one per-profile `vision` capability, set in Settings. Ollama's probed `vision`
   flag is not used, so the person's switch is the only source for chat.
3. **Persistence:** images are sent with the request only. The conversation file keeps text, so the
   transcript does not show them after sending.

## Plan
- **Backend DTO:** `ChatReplyRequestDto` gains `images: Vec<ChatImageDto>` (`media_type`, base64
  `data`), serde default empty. Limits: at most 4 images, PNG/JPEG/WebP only, each at most 1024 px on
  its long side after the client resize, total bytes bounded. Validate in `validate_message` beside the
  text limits. Reject with a typed failure, never truncate silently.
- **Capability exposure:** the profile or role response reports whether the chat endpoint accepts
  images (Ollama `vision` after a model probe, or the person's switch), so the frontend knows before the
  person pastes.
- **Service:** `chat.rs` passes the images into `LlmChatGeneration.images` on the user prompt; the
  request and the saved message carry them only as decided in point 3.
- **OpenAPI:** regenerate with `pnpm api:export` and `pnpm api:generate`; `pnpm api:check` must pass.
- **Frontend paste:** in the composer's `onpaste`, read `clipboardData.items` for `image/*` when the
  chat endpoint accepts images, resize on a canvas to 1024 px, encode as JPEG or PNG, and add a chip
  (thumbnail plus ×) to the pills row. Without the capability, leave the paste to the browser so
  nothing is silently dropped; show a short note saying the model cannot read images.
- **Viewer:** clicking a thumbnail opens `MaterialBox` with the image as `src`.
- **Limits in the UI:** the composer shows how many images are attached and blocks Send above the
  limit with the existing `contextTooLarge` style message.

## Acceptance Criteria
- Pasting an image with a vision-capable endpoint attaches a chip; Send includes the image in the
  request, and the reply uses it. A non-vision endpoint never receives image bytes.
- Oversize or unsupported images are refused with a message; nothing is truncated silently.
- Clicking a chip opens the image full size in `MaterialBox`; × removes it before sending.
- Rust tests: validation limits, Ollama body carries the images, service passes them through.
  Frontend tests: paste routing (image vs. text vs. no capability) and the resize helper.
- Both browser and Tauri hosts behave the same (rule: preserve parity).
