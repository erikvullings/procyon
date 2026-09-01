use std::{cmp::Ordering, path::Path};

use crate::{Error, Result};

const MAX_NAME_CODE_UNITS: usize = 31;

/// Compares two CFB directory names using the MS-CFB shortlex ordering.
///
/// Names are ordered by their UTF-16 code-unit length first and then by the
/// simple uppercase value of each individual UTF-16 code unit. Surrogates are
/// deliberately left unchanged, as required by MS-CFB section 2.6.4.
pub fn compare_names(left: &str, right: &str) -> Ordering {
  if left.is_ascii() && right.is_ascii() {
    return left.len().cmp(&right.len()).then_with(|| {
      left
        .bytes()
        .map(|value| value.to_ascii_uppercase())
        .cmp(right.bytes().map(|value| value.to_ascii_uppercase()))
    });
  }

  let left: Vec<_> = left.encode_utf16().collect();
  let right: Vec<_> = right.encode_utf16().collect();
  left.len().cmp(&right.len()).then_with(|| {
    left
      .into_iter()
      .map(cfb_simple_uppercase)
      .cmp(right.into_iter().map(cfb_simple_uppercase))
  })
}

pub(crate) fn names_equal(left: &str, right: &str) -> bool {
  compare_names(left, right) == Ordering::Equal
}

pub(crate) fn validate_entry_name(name: &str) -> Result<()> {
  if name.is_empty() {
    return Err(Error::invalid(0, "CFB name is empty"));
  }
  // Do not reject an embedded NUL here.  Although unusual, MS-CFB only
  // forbids '/', '\\', ':', and '!' in the name's declared UTF-16 span;
  // the required terminator is represented separately by Name Length.
  if name
    .chars()
    .any(|value| matches!(value, '/' | '\\' | ':' | '!'))
  {
    return Err(Error::invalid(0, "CFB name contains a forbidden character"));
  }
  if name.encode_utf16().count() > MAX_NAME_CODE_UNITS {
    return Err(Error::invalid(0, "CFB name exceeds 31 UTF-16 code units"));
  }
  Ok(())
}

/// Converts a CFB path to root-relative UTF-8 components.
///
/// Like rust-cfb, relative paths are interpreted from the CFB root, `.` is
/// ignored, and `..` is allowed only while it remains inside the root.
pub(crate) fn path_components(path: &Path) -> Option<Vec<String>> {
  let mut names = Vec::new();
  for component in path.components() {
    match component {
      std::path::Component::Prefix(_) => return None,
      std::path::Component::RootDir => names.clear(),
      std::path::Component::CurDir => {}
      std::path::Component::ParentDir => {
        names.pop()?;
      }
      std::path::Component::Normal(name) => names.push(name.to_str()?.to_owned()),
    }
  }
  Some(names)
}

fn cfb_simple_uppercase(unit: u16) -> u16 {
  if (0xd800..=0xdfff).contains(&unit) {
    return unit;
  }
  let Some(value) = char::from_u32(u32::from(unit)) else {
    return unit;
  };

  // Rust exposes full uppercase conversion. MS-CFB requires the one-to-one
  // simple mapping, so characters whose full mapping expands are retained
  // unless a known one-to-one simple mapping is listed below.
  match unit {
    0x1f80..=0x1f87 | 0x1f90..=0x1f97 | 0x1fa0..=0x1fa7 => unit + 8,
    0x1fb3 => 0x1fbc,
    0x1fc3 => 0x1fcc,
    0x1ff3 => 0x1ffc,
    _ => {
      let mut uppercase = value.to_uppercase();
      let first = uppercase.next().unwrap_or(value);
      if uppercase.next().is_some() {
        unit
      } else {
        u16::try_from(u32::from(first)).unwrap_or(unit)
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn name_ordering_is_utf16_shortlex_and_case_insensitive() {
    assert_eq!(compare_names("foobar", "FOOBAR"), Ordering::Equal);
    assert_eq!(compare_names("foo", "barfoo"), Ordering::Less);
    assert_eq!(compare_names("Foo", "bar"), Ordering::Greater);
    assert_eq!(compare_names("ßY", "UF"), Ordering::Greater);
    assert_eq!(compare_names("\u{1f80}", "\u{1f88}"), Ordering::Equal);
    assert_eq!(compare_names("\u{10400}", "\u{10428}"), Ordering::Less);
  }

  #[test]
  fn paths_follow_root_relative_cfb_semantics() {
    assert_eq!(path_components(Path::new("/A/B")).unwrap(), ["A", "B"]);
    assert_eq!(path_components(Path::new("A/./B")).unwrap(), ["A", "B"]);
    assert_eq!(path_components(Path::new("A/C/../B")).unwrap(), ["A", "B"]);
    assert!(path_components(Path::new("../A")).is_none());
  }

  #[test]
  fn entry_names_enforce_the_specified_utf16_boundary() {
    assert!(validate_entry_name("Workbook").is_ok());
    assert!(validate_entry_name("").is_err());
    assert!(validate_entry_name("Bad:Name").is_err());
    assert!(validate_entry_name(&"x".repeat(31)).is_ok());
    assert!(validate_entry_name(&"x".repeat(32)).is_err());
    assert!(validate_entry_name(&"\u{1f600}".repeat(15)).is_ok());
    assert!(validate_entry_name(&"\u{1f600}".repeat(16)).is_err());
  }
}
