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

## Automated candidate evidence

These CI runs use source revision
`60c91ce615b7e4cce19079ebadbf4c4b56a29d78`. They exercise signed candidate
artifacts on ephemeral runners; they do not complete the local qualification
gates or change the `noGo` decisions. No component release or desktop tag was
published, and task 0248 remains open.

- [PR #137 CI](https://github.com/erikvullings/procyon/actions/runs/38958177323):
  all 15 Actions jobs passed; the PR remains open and unmerged.
- [Signed component candidate](https://github.com/erikvullings/procyon/actions/runs/38958774038):
  four target payloads, signed catalogs, and the fingerprint lock were
  generated with Gemma originals and the experimental PDF renderer enabled.
  The build and publication are separate; this run did not publish a release.
- [Installed lifecycle and synthetic PDF vision](https://github.com/erikvullings/procyon/actions/runs/38967273437)
  and [host-bound PDF sample](https://github.com/erikvullings/procyon/actions/runs/38971737158):
  all four target jobs passed against signed candidate run
  `38958774038` at the source revision above. Each
  lifecycle report has 13/16 checks passing, one blocked because no preceding
  signed production candidate was supplied, and two manual-required UX checks.
  Lifecycle checks reported zero network artifact reads and successful
  synthetic image/audio/video indexing with worker restart. All four host-bound
  reports have decision `noGo` and scope
  `installed-host-vfs-8-page-synthetic-pdf-sample`; replacement and deletion
  passed. Their sampled worker peaks and PDF image-evidence counts were:

  | Target | Backend | Sampled worker peak RSS | Image pages |
  | --- | --- | ---: | ---: |
  | macOS ARM64 | Metal | 2,334,687,232 B | 8/8 |
  | Linux x86-64 | CPU | 2,160,611,328 B | 4/8 |
  | Linux ARM64 | CPU | 2,139,082,752 B | 4/8 |
  | Windows x86-64 | CPU | 2,048,323,584 B | 4/8 |

  These eight-page synthetic CI samples can miss transient RSS peaks;
  `temporaryDiskPeakBytes` is null and `resourceCeilingQualified` is false on
  every target. Full scanned-PDF runs, cross-host navigation, upgrade/rollback,
  and CPU fallback remain unqualified. None of this is an installed local
  Gate 1 result or a release-owner decision.
- [Four-target synthetic runner measurements](https://github.com/erikvullings/procyon/actions/runs/38971992914)
  used one small synthetic image, audio, video, and text file per dimension,
  not the local scanned-PDF workload:

  | Target | Backend | Maximum worker RSS across 128/256/512/768d |
  | --- | --- | --- |
  | Linux x86-64 | CPU | 3.729 GB (3.473 GiB) |
  | Linux ARM64 | CPU | 3.718 GB (3.463 GiB) |
  | Windows x86-64 | CPU | 3.674 GB (3.421 GiB) |
  | macOS ARM64 virtual runner | FP32 Metal | 3.248 GB (3.025 GiB) |

  Applying the checklist's 25% headroom rule to these synthetic peaks would
  imply 4.5 GiB on the CPU runners and 4.0 GiB on the virtual Metal runner.
  This is preliminary runner evidence, not Gate 1: the required local
  large-scanned-PDF measurement and anonymous temporary-disk peak are still
  outstanding.
