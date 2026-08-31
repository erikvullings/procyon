//! Deterministic OpenAPI export (task 0009, spec §9).
//!
//! Serializes the same [`crate::openapi_document`] served at
//! `/api/v1/openapi.json` with sorted object keys, fixed two-space
//! indentation and a single trailing newline, so exporting twice in a row
//! produces byte-for-byte identical output and CI never sees ordering-only
//! diffs.

use std::io;
use std::path::Path;

use serde::Serialize;

/// Serializes the OpenAPI document deterministically.
///
/// Converting through [`serde_json::Value`] sorts object keys: this
/// workspace never enables serde_json's `preserve_order` feature, so
/// `Value`'s object map is `BTreeMap`-backed rather than insertion-ordered,
/// which also removes any dependence on `HashMap` iteration order.
pub fn canonical_json() -> serde_json::Result<Vec<u8>> {
    let document = serde_json::to_value(crate::openapi_document())?;

    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    document.serialize(&mut serializer)?;
    bytes.push(b'\n');

    Ok(bytes)
}

/// Writes the canonical OpenAPI document to `path`, creating parent
/// directories as needed. Never binds a network port.
pub fn write_to_file(path: &Path) -> io::Result<()> {
    let bytes = canonical_json().map_err(io::Error::other)?;

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, bytes)
}
