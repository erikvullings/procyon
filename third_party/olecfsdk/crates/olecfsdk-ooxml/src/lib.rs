//! Direct typed conversion from `olecfsdk` file roots to `ooxmlsdk` packages.
//!
//! The crate does not build a format-neutral document tree. Each converter
//! borrows the legacy typed source model and constructs the final OOXML schema
//! nodes and package relationships directly.

#![forbid(unsafe_code)]
#![cfg_attr(doc, recursion_limit = "512")]

mod doc;
mod error;
mod metadata;
mod ppt;
mod report;
mod xls;

pub use doc::{convert_doc, convert_doc_with_options};
pub use error::{Error, Result};
pub use ppt::{convert_ppt, convert_ppt_with_options};
pub use report::{
  ConversionCode, ConversionIssue, ConversionOptions, ConversionOutput, ConversionReport,
  Disposition, DispositionCounts, LossPolicy, SourceLocation,
};
pub use xls::{convert_xls, convert_xls_with_options};
