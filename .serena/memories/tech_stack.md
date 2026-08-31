# Tech stack
- Rust Cargo workspace; Axum browser server; Tauri desktop shell.
- Frontend: strict TypeScript, Mithril 2, Meiosis setup/Mergerino patch state, Vite, Vitest, Biome.
- pnpm 11 workspace; root scripts orchestrate Rust, frontend, and Node script checks.
- OpenAPI export from `fm-server`; Orval generates the Fetch client.