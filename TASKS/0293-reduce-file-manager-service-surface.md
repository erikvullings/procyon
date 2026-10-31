# 0293 Replace the FileManagerService constructor chain with ServiceConfig

Status: done
Priority: medium
Subsystem: backend
Depends on: none

## Context

`FileManagerService` was built through a four-deep chain of positional-argument constructors
(`with_event_bus` → `with_platform_adapter` → `with_platform_adapter_and_credential_store` →
`..._and_search_accelerator`, up to seven positional arguments). Each new host adapter would have
added another link.

## Outcome

- `ServiceConfig` (`crates/fm-application/src/service_config.rs`, re-exported from the crate root)
  holds runtime, directories, event bus, platform adapter, credential store and search accelerator.
  Defaults are the inert ones; hosts override via `.events()`, `.platform()`, `.credential_store()`,
  `.search_accelerator()`.
- `FileManagerService::new(..)` (defaults) and `FileManagerService::from_config(config)` are the only
  constructors. The three intermediate constructors and `with_event_bus` are gone; all hosts and
  tests were migrated.

## Deliberately not done

- Post-construction capability injectors (`with_semantic_capability`, `with_rag_retrieval_capability`,
  `with_knowledge_*`, `with_document_summary_capability`, ...) stay as fluent overrides. They are
  used by ~130 integration-test call sites, and `with_semantic_capability` resets the RAG and
  knowledge retrievers, so call order is significant; a config struct would need that order encoded
  and every test migrated, for no behavioural gain.
- No public-method-count target. `impl FileManagerService` has ~260 public fns, which are
  host-facing intentions and mostly thin delegations by design (spec: the service is the
  composition root). Removing them would just move the delegation into every Axum handler and Tauri
  command.
