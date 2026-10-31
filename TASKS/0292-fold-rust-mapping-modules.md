# 0292 Fold Rust pass-through mapping modules into owning crates

Status: partial (8 of 15 folded; criterion 1 met, criterion 5 not)
Priority: medium
Subsystem: backend
Depends on: none

## Context

`crates/fm-application/src/` contains 15+ mapping modules (`checksum_mapping.rs`, `comparison_mapping.rs`, `connection_dto.rs`, `document_summary_mapping.rs`, `knowledge_mapping.rs`, `llm_profile_mapping.rs`, `operation_requests.rs`, `platform_mapping.rs`, `rag_mapping.rs`, `semantic_component_mapping.rs`, `semantic_library_mapping.rs`, `semantic_ocr_mapping.rs`, `semantic_vocabulary_mapping.rs`, `settings_mapping.rs`, `sync_profile_mapping.rs`) that are thin pass-throughs translating between DTOs and domain models. Most are under 200 lines and provide zero behavioral leverage. They're pass-through modules that earn no keep — the deletion test shows complexity vanishes, then reappears in callers that must know which mapping file to import.

## Acceptance Criteria

- At least 8 of the 15 mapping modules are removed (folded into their owning capability modules)
- No feature behaviour changes — the mapping functions still exist, just in different files
- `cargo test --workspace` passes with no regressions
- `cargo clippy --workspace` passes with no new warnings
- The remaining mapping modules (if any) are under 50 lines each

## Implementation Notes

- Map each mapping module to its owning capability module:
  - `checksum_mapping.rs` → `checksum_coordinator.rs`
  - `comparison_mapping.rs` → `search_comparison_coordinator.rs`
  - `connection_dto.rs` → `connection_facade.rs`
  - `document_summary_mapping.rs` → `document_summary.rs`
  - `knowledge_mapping.rs` → `knowledge_service.rs`
  - `llm_profile_mapping.rs` → `llm_profiles.rs`
  - `operation_requests.rs` → `operations_coordinator.rs`
  - `platform_mapping.rs` → `platform_adapter` area
  - `rag_mapping.rs` → `rag.rs`
  - `semantic_component_mapping.rs` → `semantic_components.rs`
  - `semantic_library_mapping.rs` → `semantic_library.rs`
  - `semantic_ocr_mapping.rs` → `semantic_ocr.rs`
  - `semantic_vocabulary_mapping.rs` → `semantic_vocabulary.rs`
  - `settings_mapping.rs` → `settings_mapping.rs` (keep — this is a DTO mapping, not a pass-through)
  - `sync_profile_mapping.rs` → `sync_profile_mapping.rs` (keep — similar to settings)
- The mapping functions should be private (`fn`) to the owning module, not public, unless they're needed externally
- `settings_mapping.rs` and `sync_profile_mapping.rs` may be kept as separate modules if they have external consumers

## Agent Notes

- Start with `operation_requests.rs` (164 lines) — it's small and clearly belongs inside `operations_coordinator.rs`
- `checksum_mapping.rs` and `comparison_mapping.rs` are also small and easy targets
- After folding, run `cargo clippy` to make sure no `pub` exports are accidentally removed
- The `fm-transport-dto` crate owns the DTO types — mapping functions that just convert DTO fields can be private within the owning module
- Update any tests that import from the removed mapping modules to import from the new location instead
- **Done:** folded `checksum_mapping`, `comparison_mapping`, `rag_mapping`, `document_summary_mapping`,
  `semantic_vocabulary_mapping`, `operation_requests`, `llm_profile_mapping`, `connection_dto` into
  their owners (`checksum_coordinator`, `search_comparison_coordinator`, `rag`, `document_summary`,
  `semantic_vocabulary`, `operations_coordinator`, `llm_profiles`, `connection_facade`). Mapping
  tests kept as `<name>_tests` modules. Visibility unchanged (`pub(crate)`) because `service.rs` and
  sibling services still consume them. `cargo clippy -p fm-application --all-targets -D warnings` and
  `cargo nextest run -p fm-application` (1185) pass.
- **Not done:** the "remaining modules < 50 lines" criterion is unrealistic: the leftovers are
  `knowledge_mapping` 701, `platform_mapping` 667, `semantic_component_mapping` 987,
  `semantic_library_mapping` 774, `semantic_ocr_mapping` 262 (`pub mod`, external consumers),
  `settings_mapping` 668, `sync_profile_mapping` 161. The Context claim "most under 200 lines" was
  wrong; folding these would bloat their owners. Revisit the criterion rather than force it.
