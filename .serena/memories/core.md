# Project core
- Dual-pane file manager: Rust workspace with thin Axum/Tauri hosts and shared Mithril/TypeScript frontend.
- Application logic belongs in `fm-application`; Axum/Tauri are adapters. Frontend application logic stays out of Mithril components.
- Generated OpenAPI and Orval client are checked in and never hand-edited.
- Read `mem:tech_stack` for tooling, `mem:conventions` for coding constraints, `mem:suggested_commands` for workflows, and `mem:task_completion` before completion.