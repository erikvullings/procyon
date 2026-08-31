# Conventions
- Inspect repository and depended-on TASKS contracts before changing interfaces.
- Small reviewable changes; no speculative abstractions; behavior changes require tests.
- Strongly typed errors: `thiserror` in libraries, `anyhow` only in `apps/*` binaries.
- Long-running work supports cancellation; preserve Axum/Tauri parity.
- Public interfaces documented; generated API files regenerated, never edited.
- Filesystem tests use temporary roots; never silently overwrite or follow symlinks.
- Frontend: application logic in state/actions/features, not Mithril components; no generic state framework without demonstrated need; large directories virtualized.