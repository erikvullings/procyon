# Suggested commands
- `pnpm dev:mock`, `pnpm dev:http`, `pnpm dev:tauri`: runtime-specific development.
- `pnpm --dir frontend test`, `pnpm --dir frontend typecheck`, `pnpm --dir frontend build`: frontend verification.
- `pnpm test`, `pnpm run lint`, `pnpm run build`: repository-wide verification.
- `pnpm run api:export`, `pnpm run api:generate`, `pnpm run api:check`: generated API workflow.
- `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`: Rust checks.