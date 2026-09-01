# olecfsdk

[![crates.io](https://img.shields.io/crates/v/olecfsdk.svg)](https://crates.io/crates/olecfsdk)
[![docs.rs](https://docs.rs/olecfsdk/badge.svg)](https://docs.rs/olecfsdk)

`olecfsdk` is a pure-Rust SDK for reading, inspecting, editing, and writing
Microsoft Office 97-2003 compound binary files. It exposes the actual CFB,
DOC, XLS, PPT, OLE property-set, VBA, OfficeArt, and Forms structures as typed
Rust trees and relationship views. It is not a plain-text extraction facade.

The minimum supported Rust version is 1.88; the workspace uses Rust 2024.

## Supported file roots

- CFB/OLE Structured Storage v3 and v4
- Word 97-2003 `.doc` and `.dot`
- Excel BIFF8 `.xls` and `.xlt`
- PowerPoint 97-2003 `.ppt`, `.pps`, and `.pot`
- OLE property sets, VBA projects, OfficeArt, Forms/ActiveX, and the shared
  persistent structures used by those hosts

DOC, XLS, and PPT roots preserve physical identities and expose borrowed
relationships to native content objects: document parts, paragraphs, styles,
tables and cells; workbook streams, sheets, sparse cells, formulas and cached
results; presentations, slides, notes, shapes, placeholders and text bodies.
Normal decoded scalars use ordinary Rust `String`, numbers, `bool`, enums,
`Option<T>`, and `Vec<T>` while source encoding and offset metadata remain
available for correct round trips.

## Quick start

```toml
[dependencies]
olecfsdk = "0.1.0"
```

```rust,no_run
use olecfsdk::{Result, xls::XlsFile};

fn main() -> Result<()> {
    let workbook = XlsFile::open("input.xls")?;
    for stream in workbook.workbooks.iter() {
        let view = stream.relationships()?;
        for sheet in view.sheets() {
            println!("{}", sheet.metadata().name.value);
            for cell in sheet.cells() {
                let cell = cell?;
                let position = cell.cell();
                println!(
                    "R{}C{}: {:?}",
                    position.row,
                    position.column,
                    cell.value()
                );
            }
        }
    }
    workbook.save("round-tripped.xls")?;
    Ok(())
}
```

Runnable semantic-edit examples cover every file root:

```sh
cargo run -p olecfsdk --example edit_doc -- input.doc output.doc
cargo run -p olecfsdk --example edit_xls -- input.xls output.xls
cargo run -p olecfsdk --example edit_ppt -- input.ppt output.ppt
```

## Direct OOXML conversion

The companion `olecfsdk-ooxml` crate converts each typed legacy file root
directly into the corresponding `ooxmlsdk` package. It does not create a
format-neutral IR, text projection, DOM, or temporary XML tree.

```toml
[dependencies]
olecfsdk = "0.1.0"
olecfsdk-ooxml = "0.1.0"
```

```rust,no_run
use std::{fs::File, io::BufWriter};
use olecfsdk::doc::DocFile;
use olecfsdk_ooxml::{
    ConversionOptions, LossPolicy, convert_doc_with_options,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = DocFile::open("input.doc")?;
    let converted = convert_doc_with_options(
        &source,
        ConversionOptions { unsupported: LossPolicy::Report },
    )?;
    for issue in converted.report.issues() {
        eprintln!("{:?} at {:?}", issue.code, issue.source);
    }
    let mut target = BufWriter::new(File::create("output.docx")?);
    converted.document.save(&mut target)?;
    Ok(())
}
```

`convert_doc`, `convert_xls`, and `convert_ppt` use the default strict policy
and stop at the first known semantic loss. The `*_with_options` variants only
continue when the caller explicitly selects `LossPolicy::Report`; every such
loss remains a typed source-located issue in the returned report.

The DOC lane currently maps paragraph/run/style/section/table structure,
complex fields, inline images, standard bookmarks, footnotes, endnotes, and
comments directly into typed WordprocessingML nodes and parts. Bookmark and
comment ranges retain paired identities and exact CP boundaries without a
text-projection or DOM intermediate.

## Strict and compatible operation

Ordinary open and save methods are strict. A producer deviation must be opened
through a `*_compatible` entry point, inspected through structured diagnostics,
and saved with `SaveOptions::preserving_compatibility()` only when retaining
that exact compatibility state is intentional.

The parse-time CFB is a private preservation snapshot. Typed edits update the
Rust tree; `to_compound_file`, `to_bytes`, `write_to`, and `save` rebuild every
managed stream while retaining unrelated CFB entries. Saves do not return the
source file as a shortcut. PPT incremental-history policy is independently
controlled by `PptHistoryStrategy`.

Owned `from_vec` entry points avoid copying the complete input archive.
File-root clones share immutable archive and typed-tree backing; mutation uses
copy-on-write. Prefer `write_to` or `save` when a final in-memory file image is
not needed.

For CFB-level access to large files, `CompoundFileReader<File>` keeps stream
payloads file-backed and returns fallible positional stream cursors. It does
not hide I/O behind the infallible borrowed-slice API used by an owned
`CompoundFile`:

```rust,no_run
use std::io::Read;
use olecfsdk::{Result, cfb::CompoundFileReader, doc::WORD_DOCUMENT_STREAM_PATH};

fn read_prefix(path: &str) -> Result<[u8; 32]> {
    let compound = CompoundFileReader::open(path)?;
    let mut stream = compound.open_stream(WORD_DOCUMENT_STREAM_PATH)?;
    let mut prefix = [0; 32];
    stream.read_exact(&mut prefix)?;
    Ok(prefix)
}
```

DOC, XLS, and PPT typed roots currently own a shared parsed archive. Use
`from_vec` when transferring an existing input buffer, or the file-backed CFB
reader when only selected streams are needed. Converting a file-backed reader
with `into_owned` is the explicit full-feature fallback.

## 0.1.0 support matrix

| Area | 0.1.0 contract |
| --- | --- |
| CFB | v3/v4 tree and stream read/write, mini/regular streams, file-backed cursors, deterministic owned rebuild |
| DOC | Word 97-2003 typed file root, document parts, text/formatting, paragraphs, sections, tables, fields, drawings and managed-stream rebuild |
| XLS | BIFF8 typed workbook roots, sheets, cells, formulas/cached values, formatting, classic/12-era AutoFilter and SortData records, comments, hyperlinks, drawings and pointer relayout |
| PPT | PowerPoint 97-2003 typed history/live views, slides, masters, notes, shapes, placeholders, text, pictures and persist relayout |
| OOXML conversion | Direct typed DOC→DOCX (fields, bookmarks, notes/comments, textboxes, floating shapes/pictures), XLS→XLSX (workbook properties, calculation and workbook/sheet/range protection settings, workbook/sheet views, worksheet properties and used dimensions, AutoFilter criteria and nested/sheet sort states, default row/column formatting and outline maxima, panes/selections, print settings, headers/footers, page breaks, row/column layout, worksheet phonetic defaults/visibility, rich and phonetic shared strings, comments, and worksheet pictures), and PPT→PPTX (master/layout/notes, tables/media, legacy palette themes and slide transitions); shared OLEPS core properties, explicit loss policy, and source-located diagnostics |
| Shared | OLE property sets, VBA, OfficeArt, Forms/ActiveX and host relationships |
| Compatibility | Explicit diagnostics and preserving save policy; no silent downgrade of known structures |

The SDK models stored structure and relationships; it is not an Office layout
or formula-calculation engine. Rendering, pagination fidelity, formula
evaluation, password-based encryption, and legacy pre-97 formats are outside
this release claim.

## Safety and limits

Parsing uses checked offsets, bounded readers, configurable `Limits`, explicit
strict/compatible diagnostics, and no native Office or COM dependency. Unknown
bounded extensions and specification-defined opaque payloads retain their
identity and exact bytes; known structures do not silently degrade to generic
raw payloads.

Encrypted documents are recognized but password-based decryption and
re-encryption are intentionally outside 0.1.0. Word 1/2/6/95, BIFF2-5, and
PowerPoint 4/95 are separate legacy work and do not share the Office 97-2003
support claim.

## Validation

The SDK is developed whole-file and round-trip first. Supported corpus files
are opened through typed roots, traversed, rebuilt, reopened, compared for
logical stability, and saved a second time. Focused tests cover transaction
rollback, damaged input, resource limits, relationship integrity, and real
semantic edits. Corpus-scale coverage lives in the adjacent
[`ooxmlsdk-test-suite`](https://github.com/KaiserY/ooxmlsdk-test-suite), in
`crates/olecfsdk-test`.

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## License

MIT OR Apache-2.0. See
[LICENSE-MIT](https://github.com/KaiserY/olecfsdk/blob/main/LICENSE-MIT) and
[LICENSE-APACHE](https://github.com/KaiserY/olecfsdk/blob/main/LICENSE-APACHE).
