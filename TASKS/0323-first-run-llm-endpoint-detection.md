# 0323 First-run LLM endpoint detection

Status: open
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0322

## Context
A new user gets no AI features until they find Settings and enter an endpoint. Many people already
run a local server (Ollama, LM Studio, oMLX on macOS). The first-run flow should find those
automatically and offer to use them. The list of servers to probe must be data, so that adding a
new default later needs no new detection code.

## Acceptance Criteria
- On first launch (no endpoint configured, and the first-run flag not yet set), the app probes the
  built-in endpoint presets on loopback. Presets are a declared list: name, preset kind, base URL,
  probe path, platform filter (for example oMLX: macOS only). Initial list: Ollama, LM Studio, oMLX.
- Adding a preset to the list is the only change needed for it to be probed. The probe and the
  presentation are generic and tested with a fixture preset that is not one of the built-ins.
- Probes are short (timeout well under a second each), run in parallel, and never send a prompt:
  they list models only. A preset that responds with a model list is reported as found.
- The first-run panel offers, for each found server: "Use for chat and search" with a model picker
  pre-selected from the list (or one model for both roles, per 0322 scenario A). If nothing is
  found: "Connect a cloud provider" (opens the endpoint editor with 0322's consent step) or "Later".
- Skipping leaves a dismissible banner in the AI settings. The first-run flag is set once the user
  has chosen either option, and can be reset in Settings.
- The panel is reachable again from Settings > LLM (AI) with "Detect local servers".
- Cross-host: the browser host (mock/HTTP) runs the same probe through the backend, so the probe
  result is the same as on desktop. Probing a server on the server host is what the browser sees.
- No probe reaches a non-loopback address. Cloud presets are never probed.
- Tests: preset list validation, probe result mapping, platform filter, first-run flag, fixture
  preset added without touching the detection code.

## Implementation Notes
- Preset list as a static Rust table or a JSON file bundled with `fm-application`, so the frontend
  reads it through an API and does not duplicate it. Verify the default ports and probe paths of
  each server against its current documentation when implementing (Ollama 11434, LM Studio 1234;
  oMLX as documented by its project).
- The existing preset enum (`LlmProfilePresetDto`, `list_llm_profile_presets`) is the natural place
  to attach the detection metadata.
- Detection runs through the existing HTTP stack and the loopback policy in `llm_profiles.rs`.

## Agent Notes
- 2026-11-12 claude: Created from design discussion. Not started.
