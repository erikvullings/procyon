# Experimental EmbeddingGemma 2 release notes

EmbeddingGemma 2 is available as an **opt-in experimental** semantic-search
model. Multilingual E5 remains the default. Choosing Gemma requires explicit
consent before installing the model or creating its separate index; existing
E5 libraries are not converted.

The release owner explicitly approved this experimental package with the
known limitations below. This is approval to distribute an opt-in experiment,
not a finding that Gemma quality or the listed unqualified behavior has been
proven.

The signed component candidate was built from source
`4553da275e76638678c7fc0a6ddcea732a19165b` and qualified on Linux ARM64,
Linux x86-64, macOS ARM64, and Windows x86-64. macOS Intel is not supported
by this Gemma package. On macOS ARM, image acceleration selects Metal
automatically when available; other targets use CPU. CPU fallback behavior
has not been qualified.

Gemma libraries can optionally index PDF page images, in addition to PDF text.
On CPU-only devices this is off by default. Enabling it applies a four-image-
page-per-PDF budget and can take about eight minutes at the observed rate of
up to 106 seconds per indexed page. Text indexing is unaffected. On Metal
devices the option is on by default and can be turned off. Search results
report when page images were skipped by the configured budget.

**Known limitations**

- No model-quality claim is made. The candidate and evaluation remain `noGo`
  for model quality; the E5 evaluation is only a negative control.
- Resource ceilings and the peak temporary disk usage of anonymous PDF spools
  are unqualified. Sampled RSS values are measurements, not safe minimums or
  ceilings.
- CPU fallback, cross-host navigation, and upgrade/rollback have not been
  qualified.
- CPU PDF page-image indexing is slow: up to approximately 106 seconds per
  indexed page in the installed sample, with a four-page budget of about
  eight minutes.

This package is experimental and opt-in. Keep important work backed up and
continue to use E5 if the experimental model does not meet your needs.
