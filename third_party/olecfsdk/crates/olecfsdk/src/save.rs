//! File-root save policy.

/// Controls whether a file root may emit nonconforming bytes retained by an
/// explicit compatibility node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompatibilityWritePolicy {
  /// Reject output while a managed compatibility node remains.
  #[default]
  Reject,
  /// Preserve the exact bytes held by a compatibility node.
  ///
  /// This is intended for lossless corpus handling and deliberate repair
  /// workflows. It does not make those bytes conform to the specification.
  Preserve,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveOptions {
  pub compatibility: CompatibilityWritePolicy,
}

impl SaveOptions {
  pub const fn preserving_compatibility() -> Self {
    Self {
      compatibility: CompatibilityWritePolicy::Preserve,
    }
  }

  pub const fn preserves_compatibility(self) -> bool {
    matches!(self.compatibility, CompatibilityWritePolicy::Preserve)
  }
}
