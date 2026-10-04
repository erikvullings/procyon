# Procyon SVGO panel source

This is the source for the bundled [`plugins/svgo`](../../plugins/svgo/) panel,
copied from `erikvullings/svgo` commit
`ee29ff2c46912dc9318fbaf8f5d408e898a71f7b`. Make future Procyon panel
changes here, not in the generated `plugins/svgo/dist/` assets. This directory
is outside `plugins/` so Tauri bundles only the built panel, not the source or
its development dependencies.

Desktop packaging (`pnpm run build:tauri`, including CI and release builds)
automatically installs this directory's separately locked dependencies, builds
the Procyon panel, and replaces `plugins/svgo/dist/` before Tauri bundles
`plugins/`. Direct Tauri builds use the same pre-build hook. From the Procyon
repository root, with Node.js and pnpm installed, the equivalent standalone
preparation is:

```sh
node scripts/build-svgo-plugin.mjs
```

Run this preparation before direct `cargo build -p fm-desktop` commands when
testing bundled SVGO. The script runs `build:procyon` (`vite build --mode
procyon`) and replaces obsolete hashed assets while preserving
`plugins/svgo/plugin.toml`. The checked-in dist is only a baseline for direct
workflows; do not commit generated changes for source updates. To check the
source independently, run `pnpm --dir plugin-sources/svgo exec tsc --noEmit`
and `pnpm --dir plugin-sources/svgo test` after installation.

Do not run the default `build` script to update the plugin: it produces the
standalone GitHub Pages site in this source directory's `docs/`, not the
isolated Procyon package. The standalone SVGO repository evolves separately;
changes there do not automatically update this copy.
