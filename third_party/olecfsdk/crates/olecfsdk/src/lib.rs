//! Typed Rust SDK for Microsoft Office compound binary file formats.
//!
//! `olecfsdk` reads, navigates, edits, and rebuilds CFB/OLE containers and
//! Office 97-2003 DOC, XLS, and PPT files. File roots own the actual native
//! record trees and expose borrowed relationship views; the crate deliberately
//! does not flatten documents into a lossy cross-format text model.
//!
//! Ordinary open and save methods are strict. Producer deviations require a
//! compatible open method and an explicit compatibility-preserving save policy.
//! Callers that already own a complete file image can use `from_vec` to avoid a
//! second archive copy, and callers that do not need output bytes can use
//! `write_to` or `save` for sequential output.
//! [`cfb::CompoundFileReader`] provides a separate fallible, file-backed CFB
//! lane for callers that only need selected streams; DOC/XLS/PPT typed roots
//! currently use an owned shared archive.
//!
//! # Example
//!
//! ```no_run
//! use olecfsdk::{Result, xls::XlsFile};
//!
//! fn main() -> Result<()> {
//!     let file = XlsFile::open("input.xls")?;
//!     for workbook in file.workbooks.iter() {
//!         let view = workbook.relationships()?;
//!         for sheet in view.sheets() {
//!             println!("{}", sheet.metadata().name.value);
//!             for cell in sheet.cells() {
//!                 let cell = cell?;
//!                 println!("{:?}: {:?}", cell.cell(), cell.value());
//!             }
//!         }
//!     }
//!     file.save("round-tripped.xls")?;
//!     Ok(())
//! }
//! ```

#![forbid(unsafe_code)]

extern crate self as olecfsdk;

pub mod cfb;
pub mod common;
pub mod doc;
pub mod error;
pub mod forms;
pub mod io;
pub mod limits;
pub mod office_art;
pub mod ograph;
pub mod parse;
pub mod ppt;
pub mod property_set;
pub mod save;
pub mod shared;
pub mod shared_content;
pub mod vba;
pub mod xls;

pub use error::{Error, Result};
pub use olecfsdk_derive::{SdkBitfield, SdkEnum, SdkObject};
pub use parse::{
  ParseDiagnostic, ParseDiagnosticCode, ParseDiagnosticLocation, ParseDiagnosticSeverity,
  ParseOptions, ParseOutcome, SpecificationReference,
};
pub use save::{CompatibilityWritePolicy, SaveOptions};
