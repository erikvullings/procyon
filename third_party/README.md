# Vendored dependencies

## ooxmlsdk

- Repository: https://github.com/KaiserY/ooxmlsdk
- Tag: `v0.13.0`
- Commit: `e17c04db15ef76514ac55f4a73d731cf62e40cee`
- License: MIT OR Apache-2.0

The unpublished `ooxmlsdk-fonts`, `ooxmlsdk-formula`, `ooxmlsdk-layout`, and `ooxmlsdk-pdf`
packages are retained together so they remain version-aligned. Procyon accesses them only through
`crates/fm-pptx-renderer`.

Local modifications:

- workspace dependencies for `emfsdk` and `olecfsdk` use the sibling vendored source trees below
  instead of upstream Git revisions;
- missing Office font families fall back through common system sans-serif families, preventing
  Aptos-authored presentations from silently losing text on hosts without Aptos;
- the PPTX layout/PDF crates expose a first-slide conversion entry point so Procyon can display the
  first page while the complete presentation renders (the adapter caps that inline PDF at 8 MiB).

## emfsdk

- Repository: https://github.com/KaiserY/emfsdk
- Commit: `711e1bdee925037234103b8ddb24efd8b0be6b6f`
- License: MIT OR Apache-2.0

The renderer tag requires metafile APIs added after the published `emfsdk 0.2.0`.

## olecfsdk

- Repository: https://github.com/KaiserY/olecfsdk
- Commit: `41b277582d1368ed1d4157d79862552bfcebfc58`
- License: MIT OR Apache-2.0

`ooxmlsdk-layout` uses this unpublished package for embedded Office object handling.
