//! File-root parsing policy and structured compatibility diagnostics.

use std::{fs::File, path::Path};

use crate::{
  Result,
  cfb::CompoundFile,
  io::{BinaryFormat, ParseMode},
  limits::Limits,
};

/// File-root parsing policy.
///
/// Strict parsing is the default. Compatibility parsing must be requested
/// explicitly. As format parsers are migrated to this control plane, every
/// accepted nonconforming shape is required to add a
/// [`ParseOutcome::diagnostics`] entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseOptions {
  pub mode: ParseMode,
  pub limits: Limits,
}

impl ParseOptions {
  pub const fn strict(limits: Limits) -> Self {
    Self {
      mode: ParseMode::Strict,
      limits,
    }
  }

  pub const fn compatible(limits: Limits) -> Self {
    Self {
      mode: ParseMode::Compatible,
      limits,
    }
  }

  pub const fn is_strict(self) -> bool {
    matches!(self.mode, ParseMode::Strict)
  }
}

impl Default for ParseOptions {
  fn default() -> Self {
    Self::strict(Limits::default())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseDiagnosticSeverity {
  Warning,
}

/// Stable classification for a nonconforming shape accepted by compatibility
/// parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseDiagnosticCode {
  NoncanonicalCompoundFile,
  NonconformingRecord,
  TruncatedRecord,
  InvalidStreamPreserved,
  InvalidReference,
}

impl ParseDiagnosticCode {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::NoncanonicalCompoundFile => "noncanonical_compound_file",
      Self::NonconformingRecord => "nonconforming_record",
      Self::TruncatedRecord => "truncated_record",
      Self::InvalidStreamPreserved => "invalid_stream_preserved",
      Self::InvalidReference => "invalid_reference",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseDiagnosticLocation {
  pub format: BinaryFormat,
  /// MS-CFB storage or stream path when the diagnostic belongs to an entry.
  pub path: Option<String>,
  /// Byte offset relative to `path`, or relative to the compound file when
  /// `path` is absent.
  pub offset: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecificationReference {
  pub document: &'static str,
  pub section: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseDiagnostic {
  pub code: ParseDiagnosticCode,
  pub severity: ParseDiagnosticSeverity,
  pub location: ParseDiagnosticLocation,
  /// Structure name used by the applicable Microsoft Open Specification.
  pub structure: &'static str,
  pub specification: SpecificationReference,
  pub case_id: Option<&'static str>,
  pub message: String,
}

impl ParseDiagnostic {
  pub(crate) fn warning(
    code: ParseDiagnosticCode,
    format: BinaryFormat,
    path: Option<&str>,
    offset: Option<u64>,
    structure: &'static str,
    specification: SpecificationReference,
    message: impl Into<String>,
  ) -> Self {
    Self {
      code,
      severity: ParseDiagnosticSeverity::Warning,
      location: ParseDiagnosticLocation {
        format,
        path: path.map(str::to_owned),
        offset,
      },
      structure,
      specification,
      case_id: None,
      message: message.into(),
    }
  }
}

/// A parsed static tree together with the structured compatibility diagnostics
/// emitted while constructing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOutcome<T> {
  pub value: T,
  pub diagnostics: Vec<ParseDiagnostic>,
}

impl<T> ParseOutcome<T> {
  pub const fn new(value: T, diagnostics: Vec<ParseDiagnostic>) -> Self {
    Self { value, diagnostics }
  }

  pub fn into_value(self) -> T {
    self.value
  }

  pub fn map<U>(self, map: impl FnOnce(T) -> U) -> ParseOutcome<U> {
    ParseOutcome {
      value: map(self.value),
      diagnostics: self.diagnostics,
    }
  }
}

pub(crate) fn compound_from_path(
  path: &Path,
  options: ParseOptions,
  format: BinaryFormat,
) -> Result<ParseOutcome<CompoundFile>> {
  let compound = CompoundFile::from_reader_with_limits(File::open(path)?, options.limits)?;
  compound_outcome(compound, options, format)
}

pub(crate) fn compound_from_bytes(
  bytes: &[u8],
  options: ParseOptions,
  format: BinaryFormat,
) -> Result<ParseOutcome<CompoundFile>> {
  let compound = CompoundFile::from_bytes_with_limits(bytes, options.limits)?;
  compound_outcome(compound, options, format)
}

pub(crate) fn compound_from_vec(
  bytes: Vec<u8>,
  options: ParseOptions,
  format: BinaryFormat,
) -> Result<ParseOutcome<CompoundFile>> {
  let compound = CompoundFile::from_vec_with_limits(bytes, options.limits)?;
  compound_outcome(compound, options, format)
}

pub(crate) fn compound_outcome(
  compound: CompoundFile,
  options: ParseOptions,
  format: BinaryFormat,
) -> Result<ParseOutcome<CompoundFile>> {
  match compound.validate_strict() {
    Ok(()) => Ok(ParseOutcome::new(compound, Vec::new())),
    Err(error) if options.is_strict() => Err(error),
    Err(error) => {
      let offset = error.offset();
      Ok(ParseOutcome::new(
        compound,
        vec![ParseDiagnostic::warning(
          ParseDiagnosticCode::NoncanonicalCompoundFile,
          format,
          None,
          offset,
          "Compound File",
          SpecificationReference {
            document: "MS-CFB",
            section: "2",
          },
          error.to_string(),
        )],
      ))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cfb::Version;

  #[test]
  fn strict_is_the_explicit_default() {
    assert!(ParseOptions::default().is_strict());
    assert_eq!(
      ParseOptions::compatible(Limits::default()).mode,
      ParseMode::Compatible
    );
    assert_eq!(
      ParseDiagnosticCode::InvalidStreamPreserved.as_str(),
      "invalid_stream_preserved"
    );
  }

  #[test]
  fn compatible_mode_reports_noncanonical_compound_fields() {
    let mut bytes = CompoundFile::new(Version::V3).unwrap().to_bytes().unwrap();
    bytes[8] = 1;

    assert!(
      compound_from_bytes(
        &bytes,
        ParseOptions::strict(Limits::default()),
        BinaryFormat::Cfb,
      )
      .is_err()
    );
    let outcome = compound_from_bytes(
      &bytes,
      ParseOptions::compatible(Limits::default()),
      BinaryFormat::Cfb,
    )
    .unwrap();
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(
      outcome.diagnostics[0].code,
      ParseDiagnosticCode::NoncanonicalCompoundFile
    );
    assert_eq!(outcome.diagnostics[0].location.format, BinaryFormat::Cfb);
    assert_eq!(outcome.diagnostics[0].location.offset, Some(8));
  }
}
