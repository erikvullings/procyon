# Experimental EmbeddingGemma 2 release notes

EmbeddingGemma 2 is available as an **opt-in experimental** semantic-search
model. Multilingual E5 remains the default. Choosing Gemma requires explicit
consent before installing the model or creating its separate index; existing
E5 libraries are not converted.

This candidate is for installed-app qualification, not promotion. No model
quality claim is made. The candidate and evaluation remain `noGo` for model
quality, and E5 is only a negative control. The exact worker source revision
and signed target catalogs are recorded in the candidate's semantic component
manifest; the desktop release gate requires that revision to match the
desktop source commit.

Resource ceilings and peak temporary disk usage of anonymous PDF spools remain
unqualified. CPU fallback, cross-host navigation, and upgrade/rollback remain
unqualified. CPU PDF page-image indexing is slow: up to approximately 106
seconds per indexed page in the installed sample, with a four-page budget of
about eight minutes. PDF text indexing is unaffected.

The Gemma macOS package requires Apple Silicon (arm64); it does not run on
Intel Macs. On macOS ARM, image acceleration selects Metal automatically when
available; other targets use CPU. CPU fallback behavior has not been
qualified.

This package is experimental and opt-in. Keep important work backed up and
continue to use E5 if the experimental model does not meet your needs.
