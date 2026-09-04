# 0184 OpenAI-compatible LLM connection profiles

Status: open
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0030, 0103

## Context

Semantic search does not require an LLM. Document summaries and RAG become available only when the
user configures a generation service. Configuration must be easy for common local servers while
handling cloud credentials and disclosures correctly.

Use reusable named profiles rather than one global endpoint or duplicated full configuration per
workspace. Workspaces/conversations select a profile by ID; secrets remain in Procyon's credential
service and never enter ordinary settings, exports, logs, or the semantic worker.

## Acceptance Criteria

- Named profiles support presets for Ollama, LM Studio, vLLM, SGLang, OMLX, generic
  OpenAI-compatible endpoints, and Azure OpenAI. Presets fill safe URL/header conventions while
  allowing explicit edits.
- Normal setup asks for preset, base URL/deployment details, model, credential reference, and Test.
  Advanced settings cover context window, maximum answer tokens, temperature, timeout, TLS policy
  constraints, and allow-listed custom headers with conservative defaults.
- The required generation baseline is streaming `POST /v1/chat/completions`. Azure maps its actual
  deployment URL, API version, and `api-key` semantics. A future Responses API is represented as a
  capability, not faked through Chat Completions.
- Test validates URL parsing, loopback/local classification, authentication, model availability,
  streaming shape, timeout/error normalization, and optional model discovery without exposing the
  secret or retaining response content.
- API keys/tokens are stored only through `fm-credentials`; settings persist credential IDs. Profile
  clone/export omits secrets and profile deletion explicitly handles orphaned credential entries.
- A cloud profile requires informed consent the first time it is activated, showing endpoint host
  and that question/history plus retrieved excerpts will leave the device. Consent is keyed to the
  normalized host and is invalidated when that host changes.
- Local/cloud status remains visible wherever a profile is selected. Per-profile filename redaction
  may further minimize cloud metadata.
- Profiles and capability/test operations have equivalent semantic client, HTTP, Tauri, and mock
  implementations. Browser/server mode applies administrator policy to allowed hosts and profiles
  and prevents SSRF to disallowed destinations.
- Diagnostics log only profile/provider IDs, normalized error categories, timing, and status. Auth
  headers, HTTP bodies, prompts, responses, filenames, and tokens are never logged.
- Tests cover every preset, Azure URL construction, secret persistence, export redaction, host-change
  consent, streaming parsing, cancellation, malformed/error responses, TLS/SSRF restrictions,
  profile migration, and HTTP/Tauri parity.

## Implementation Notes

- Retrieval counts and context budgets are RAG policy, not endpoint-profile settings.
- Reuse the existing credential and connection-validation architectural patterns without forcing LLM
  profiles into the VFS connection model; they are generation services, not filesystems.
- This task creates no Ask UI and sends no document content except a bounded synthetic Test payload.

## Agent Notes

- 2026-09-04: Split from 0176. Chat Completions is the broad compatibility baseline; profiles are
  reusable globally and cloud consent is bound to the endpoint host.