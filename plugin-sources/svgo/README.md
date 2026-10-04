# Procyon SVGO panel source

This is the source for the bundled [`plugins/svgo`](../../plugins/svgo/) panel,
copied from `erikvullings/svgo` commit
`ee29ff2c46912dc9318fbaf8f5d408e898a71f7b`. Make future Procyon panel
changes here, not in the generated `plugins/svgo/dist/` assets. This directory
is outside `plugins/` so Tauri bundles only the built panel, not the source or
its development dependencies.

From the Procyon repository root, with Node.js and pnpm installed:

```sh
pnpm --dir plugin-sources/svgo install --frozen-lockfile
pnpm --dir plugin-sources/svgo exec tsc --noEmit
pnpm --dir plugin-sources/svgo test
pnpm --dir plugin-sources/svgo build:procyon
rsync -a --delete plugin-sources/svgo/dist/procyon/ plugins/svgo/dist/
```

`build:procyon` runs `vite build --mode procyon` and produces the complete
package in `plugin-sources/svgo/dist/procyon/`; the last command replaces only
the checked-in bundled assets at `plugins/svgo/dist/`, including obsolete
hashed assets. On systems without `rsync`, replace the contents of
`plugins/svgo/dist/` with the complete contents of that generated directory.
Commit changes to both the source and bundled assets together. Procyon's
`plugin.toml` remains at `plugins/svgo/plugin.toml` and is not generated.

Do not run the default `build` script to update the plugin: it produces the
standalone GitHub Pages site in this source directory's `docs/`, not the
isolated Procyon package. The standalone SVGO repository evolves separately;
changes there do not automatically update this copy.
