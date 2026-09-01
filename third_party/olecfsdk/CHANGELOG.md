# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

### Added

- Native CFB v3/v4 parsing, editing, deterministic rebuilding, bounded I/O,
  resource limits, and strict/compatible diagnostics.
- Typed, whole-file DOC, XLS, and PPT roots with managed-stream rebuilding,
  semantic relationship views, transactional edits, save/reopen validation,
  and compatibility-preserving write policies.
- Typed OLE property sets, VBA projects, OfficeArt, Forms/ActiveX, embedded
  object relationships, and shared Office content.
- Rust-native document objects and scalar values for DOC paragraphs/tables,
  XLS sheets/cells/formulas, and PPT slides/shapes/text bodies without a lossy
  cross-format extraction layer.
- Corpus coverage and round-trip ratchets maintained in the adjacent
  `olecfsdk-test` crate.

### Performance

- Shared archive and record-tree backing with copy-on-write mutation.
- Lazy CFB regular-stream ranges, zero-allocation file-root clones, owned input
  entry points, fallible file-backed CFB stream cursors, and sequential `Write`
  sinks.
- Exact-length CFB stream plans for DOC WordDocument/Table/Data, XLS BIFF
  workbooks, and PPT document/Pictures streams, avoiding complete managed-stream
  buffers on `write_to` paths.
- Measured reductions in DOC text traversal, BIFF relayout, PPT recursive
  serialization, OfficeArt serialization, and strict save validation.

### Known limitations

- Password-based decryption and re-encryption are intentionally deferred.
- Word 1/2/6/95, BIFF2-5, and PowerPoint 4/95 are outside the Office 97-2003
  release lane.
