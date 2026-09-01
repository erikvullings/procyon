# Changelog

## 0.2.0

### Changed

- Added borrowed EMF, EMF+, and WMF views with validated, allocation-free
  record iteration and explicit `into_owned` conversion.
- Made compatible parsing preserve producer extensions by default; use
  `validate_strict` or `from_bytes_strict` for specification-only input.
- Standardized typed reconstruction with `rebuild_typed`, `SdkWrite`, and an
  owned `EmfPlusStream`; `from_bytes_exact` rejects trailing stream data.
- Corrected EMF+ continued-object framing and record-relative
  `EMR_COMMENT_MULTIFORMATS` offsets.
- Consolidated simple fixed layouts under `SdkObject` while keeping strings,
  offsets, bitmap payloads, and conditional data hand-written.

### Performance

- Removed `Seek` from write APIs. `Writer` tracks its position and top-level
  `write_to` methods accept any `std::io::Write` sink.
- Reduced record cloning and intermediate buffers through borrowed views,
  direct `Vec` serialization, and preallocated output.

### Testing

- Exact typed and whole-file round trips cover 987,606 standalone records and
  107,584 Office-embedded records. The remaining 15 and 138 compatibility
  fallbacks are counted and never silently skipped.

## 0.1.0

- Added typed EMF, EMF+, WMF, DIB, and optional raster-rendering support.
- Added byte-preserving round trips for unknown records and opaque data.
- Added compatibility-first parsing, strict validation, and fixed-layout
  derive macros.
