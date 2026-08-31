# 0002 Frontend Vite + Mithril + TypeScript skeleton

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: frontend
Depends on: none

## Context
Create the `frontend/` project described in `file-manager-coding-agent-spec.md` §2.1 and §4.
This is the shell only — no file-manager UI yet (that starts at 0024).

## Acceptance Criteria
- `frontend/` contains `index.html`, `package.json`, `tsconfig.json`, `vite.config.ts`,
  `vitest.config.ts` and `src/main.ts`, managed with pnpm.
- Vite 8, TypeScript 7 with `strict` plus `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
  `noImplicitOverride`; ESM only.
- Mithril.js and `mithril-materialized` installed and rendering a placeholder app shell using a
  factory component and idiomatic lifecycle methods.
- Vitest runs and one smoke test passes.
- Source directories created per §4: `api/`, `app/`, `components/`, `features/`, `models/`,
  `state/`, `commands/`, `keybindings/`, `themes/`, `utilities/`.
- Vite dev server proxies `/api` to `http://127.0.0.1:<server port>`, and the proxy is configured so
  SSE responses stream without buffering (§32).
- `pnpm --dir frontend build` (i.e. `tsc --noEmit && vite build`) succeeds with no errors.
- No React/Vue/Svelte/Angular dependencies (§2.1).

## Implementation Notes
- `VITE_RUNTIME` env var (`http` | `tauri` | `mock`) is read here but only consumed in 0011.
- Keep `mithril-materialized` for dialogs/forms/menus; the file table is custom (§14).
- Reference: local `mithril`, `mithril-materialized` and `meiosis` skills.

## Agent Notes
- 2026-07-29 claude: Implemented. `frontend/` holds `index.html`, `package.json`, `tsconfig.json`,
  `vite.config.ts`, `vitest.config.ts`, `src/main.ts` and the ten §4 source directories (empty ones
  carry a `.gitkeep`). Installed versions: Vite 8.1.5, TypeScript 7.0.2, Mithril 2.3.8,
  mithril-materialized 3.17.6, Vitest 4.1.10, jsdom 29.1.1. ESM only (`"type": "module"`).
  Root `.gitignore` extended with `node_modules/`, `dist/`, `*.tsbuildinfo`.
- 2026-07-29 claude: Three units, each test-first:
  - `src/utilities/runtime.ts` — validates `VITE_RUNTIME`. Blank/unset falls back to `http`;
    whitespace and casing are tolerated; an unrecognised value throws `RuntimeConfigurationError`
    naming the value and the valid options, because silently running against the wrong transport is
    much harder to diagnose than a startup failure. Consumed by the client factory in task 0011.
  - `src/app/app-shell.ts` — factory component holding theme state in its closure, `oninit`
    initialising `ThemeManager` and rendering mithril-materialized's `ThemeSwitcher` plus a runtime
    badge. It calls `ThemeManager.setUseLocalStorage(false)` deliberately: §26 keeps settings on the
    backend, so task 0030 restores the theme from the settings service rather than browser storage.
  - `config/api-proxy.ts` — the `/api` dev proxy, kept out of `vite.config.ts` so it is importable
    and testable.
- 2026-07-29 claude: The SSE proxy behaviour is covered by a real integration test, not a config
  assertion. The fake backend opens an event stream, emits one event and then *never* ends the
  response; a buffering proxy would therefore deliver nothing, so a chunk arriving at all proves the
  stream is not buffered. That test caught a genuine bug: calling `response.flushHeaders()`
  synchronously in the `proxyRes` handler dropped `content-type` entirely, because Vite's proxy
  copies the upstream headers afterwards under `if (!res.headersSent)`. Setting the headers on
  `response` instead was also wrong — the copy pass overwrites them. The working approach is to
  mutate `proxyRes.headers` before the copy runs and defer the flush to `setImmediate`. Anyone
  touching that file should keep the test.
- 2026-07-29 claude: Verified — `pnpm test` → **14 tests across 3 files, all passing**
  (5 in `src/utilities/runtime.test.ts`, 6 in `src/app/app-shell.test.ts`, 3 in
  `config/api-proxy.test.ts`). `pnpm typecheck` (`tsc --noEmit`) clean, and `pnpm --dir frontend
  build` succeeds from the repo root. Separately proved the strict flags are actually enforced by
  TypeScript 7 rather than merely present in `tsconfig.json`: a scratch file violating
  `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride` and
  `strictNullChecks` produced TS2322, TS2412, TS4114 and TS18047; scratch file deleted.
  No React/Vue/Svelte/Angular anywhere in the lockfile, direct or transitive (0 matches).
- 2026-07-29 claude: Decisions and gaps for the next agent:
  - The backend origin is `http://127.0.0.1:8787`, exported as `DEFAULT_BACKEND_ORIGIN` and
    overridable with `FM_SERVER_ORIGIN`. **Task 0008 must bind `fm-server` to this port** or change
    both sides together.
  - pnpm's release-age gate flagged `jsdom@30.0.1` (published the same day) and auto-wrote a
    `pnpm-workspace.yaml` exclusion. I removed that file and pinned `jsdom@^29.1.1` instead rather
    than bypassing the supply-chain default. Task 0003 owns the real root workspace file.
  - `main.ts` imports the whole `mithril-materialized/index.css` (253 kB, 36 kB gzipped). Task 0022
    should consider the modular entries (`core.css`, `forms.css`, `components.css`, ...) instead.
  - There is no `mithril-inspector` package on npm; it is the scoped `@mithril-inspector/*` set.
    Recorded in task 0023's Implementation Notes so that task does not start with a dead end.
  - No linter or formatter yet — task 0003 owns those, so this code is unformatted by any shared
    tool. No `README.md` either; task 0074 owns it.
