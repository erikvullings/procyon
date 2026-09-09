# 0213 EPUB semantic indexing

Status: done
Priority: high
Subsystem: semantic conversion, kg
Depends on: 0180, 0212

## Context

Semantic ingestion supports text, source code, PDF, OOXML, spreadsheets, and delimited data, but
does not currently recognize EPUB containers. EPUB is a ZIP package whose readable book content is
declared by an OPF manifest and ordered by its spine. Treating it as an arbitrary ZIP or scanning
every archive entry would either reject useful books or accidentally index bundled images,
navigation files, fonts, and other resources.

EPUB indexing must remain provider-neutral and CPU-only. The conversion engine already owns
bounded ZIP validation and structural HTML extraction, so EPUB support should compose those
capabilities rather than add filesystem access, network resolution, OCR, or a second markup parser.

## Acceptance Criteria

- Recognize an EPUB only when a safe ZIP package contains the required uncompressed `mimetype`
  entry with the exact value `application/epub+zip`; an `.epub` extension alone is insufficient.
- Resolve `META-INF/container.xml`, the declared OPF rootfile, its manifest, and its spine without
  escaping the package or resolving external resources.
- Convert readable XHTML/HTML spine items in declared reading order through the existing structural
  HTML extraction so headings, paragraphs, lists, quotations, and tables retain their normal unit
  semantics.
- Ignore bundled images, SVG, stylesheets, fonts, audio, video, scripts, navigation resources, and
  non-spine manifest items; no image or media bytes may contribute semantic text.
- Preserve chapter order and heading hierarchy with EPUB-specific chapter/line provenance, while
  never inventing page numbers for reflowable EPUB content.
- Surface malformed metadata, missing readable spine resources, unsafe package paths, encryption,
  cancellation, and source/package/output budget overruns as the existing typed outcomes rather
  than empty success or partial silent indexing.
- Bump the baseline converter identity and every production composite identity that embeds it so
  retained semantic libraries rebuild instead of mixing pre-EPUB and EPUB-capable conversion.
- Document the supported EPUB behavior and add public-interface regressions for recognition,
  ordered text extraction, image exclusion, malformed/encrypted/unsafe packages, cancellation,
  budgets, deterministic chunking, and rendered provenance labels.

## Implementation Notes

- Keep archive access behind the existing bounded ZIP preflight and bounded-part reader. Extract
  format-neutral package helpers only where EPUB and OOXML genuinely share behavior.
- Parse EPUB XML with the crate's existing bounded XML tooling. Match XML elements by local name so
  namespace prefixes do not alter behavior.
- Resolve OPF and manifest paths relative to their containing package part, normalize `/` and `.`,
  and reject `..`, absolute paths, backslashes, URI schemes, fragments that escape resource lookup,
  or missing targets.
- Do not crawl archive entries. Only the rootfile and readable manifest entries referenced by the
  spine are eligible for semantic conversion.
- Use a distinct top-level boundary for each spine item so adjacent chapters cannot be merged by the
  structural chunker.
- Advance `BASELINE_CONVERTER_VERSION` from `baseline/1` to `baseline/2` and update checked
  production bundle identities and documentation consistently.

## Agent Notes

- 2026-09-09 Copilot: Scoped EPUB as signature-verified, manifest-driven, text-only package
  conversion. Reflowable content will expose chapter/line provenance rather than synthetic pages,
  and `baseline/2` will force retained indexes to migrate to the EPUB-capable converter.
- 2026-09-09 Copilot: Added exact uncompressed-mimetype recognition, bounded container/OPF/spine
  parsing, strict package-local path resolution with safe percent decoding, and ordered structural
  HTML extraction. Images, navigation, styles, fonts, scripts, SVG, audio, video, and non-spine
  resources do not enter semantic text. Font-only obfuscation remains indexable, while encryption
  of a readable spine resource is a typed encrypted outcome.
- 2026-09-09 Copilot: Added EPUB chapter/line provenance and per-spine chunk boundaries, rendered
  that provenance in Search Knowledge and grounded Ask, and advanced baseline plus production
  composite converter identities to `baseline/2`.
- 2026-09-09 Copilot: Verified 9 EPUB public converter regressions, the full 132-test conversion
  crate, a provider-neutral application regression, the full 836-test application package,
  247 conversion/component/Docling tests, 103 affected frontend tests, 5 production-bundle script
  tests, frontend typechecking, and the repository-wide Rust/Biome lint gate.
