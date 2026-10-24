//! Provider-neutral permission, ownership and attribute editing (task 0258).

use serde::{Deserialize, Serialize};

/// Bits of a Unix mode a user may edit: rwx for user/group/other plus setuid, setgid and sticky.
pub const EDITABLE_MODE_MASK: u32 = 0o7777;

/// What a provider can edit at one location.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeSupport {
    /// Unix permission bits (rwx, setuid, setgid, sticky) can be changed.
    pub mode: bool,
    /// Numeric owner and group can be changed (the server may still refuse for unprivileged users).
    pub ownership: bool,
    /// Windows read-only, hidden and archive attributes can be changed.
    pub windows_attributes: bool,
}

impl AttributeSupport {
    /// Whether nothing can be edited.
    #[must_use]
    pub const fn is_none(self) -> bool {
        !self.mode && !self.ownership && !self.windows_attributes
    }
}

/// The editable attribute state of one entry. A `None` field is not exposed by the provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeSnapshot {
    /// Unix permission bits, masked with [`EDITABLE_MODE_MASK`].
    pub mode: Option<u32>,
    /// Numeric owner.
    pub uid: Option<u32>,
    /// Numeric group.
    pub gid: Option<u32>,
    /// Windows read-only attribute.
    pub read_only: Option<bool>,
    /// Windows hidden attribute.
    pub hidden: Option<bool>,
    /// Windows archive attribute.
    pub archive: Option<bool>,
}

/// A requested change; `None` leaves the corresponding attribute untouched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeChange {
    /// New Unix permission bits, masked with [`EDITABLE_MODE_MASK`].
    pub mode: Option<u32>,
    /// Which bits of `mode` to apply; the rest keep the entry's own value. Absent means all
    /// twelve, so a mixed multi-selection can change only the bits the user touched.
    pub mode_mask: Option<u32>,
    /// New numeric owner.
    pub uid: Option<u32>,
    /// New numeric group.
    pub gid: Option<u32>,
    /// New Windows read-only attribute.
    pub read_only: Option<bool>,
    /// New Windows hidden attribute.
    pub hidden: Option<bool>,
    /// New Windows archive attribute.
    pub archive: Option<bool>,
}

impl AttributeChange {
    /// Whether the change requests nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.uid.is_none()
            && self.gid.is_none()
            && self.read_only.is_none()
            && self.hidden.is_none()
            && self.archive.is_none()
    }

    /// The mode an entry currently at `current` ends up with, when this change sets one.
    #[must_use]
    pub fn resolve_mode(&self, current: u32) -> Option<u32> {
        let mask = self.mode_mask.unwrap_or(EDITABLE_MODE_MASK) & EDITABLE_MODE_MASK;
        self.mode
            .map(|mode| (current & EDITABLE_MODE_MASK & !mask) | (mode & mask))
    }

    /// Whether a set field is outside what `support` allows.
    #[must_use]
    pub const fn exceeds(&self, support: AttributeSupport) -> bool {
        (self.mode.is_some() && !support.mode)
            || ((self.uid.is_some() || self.gid.is_some()) && !support.ownership)
            || ((self.read_only.is_some() || self.hidden.is_some() || self.archive.is_some())
                && !support.windows_attributes)
    }

    /// Whether every field this change sets already has that value in `current`.
    #[must_use]
    pub fn is_applied_to(&self, current: &AttributeSnapshot) -> bool {
        fn holds<T: PartialEq>(wanted: Option<T>, actual: Option<T>) -> bool {
            wanted.is_none_or(|wanted| actual == Some(wanted))
        }
        let mask = self.mode_mask.unwrap_or(EDITABLE_MODE_MASK) & EDITABLE_MODE_MASK;
        holds(
            self.mode.map(|mode| mode & mask),
            current.mode.map(|mode| mode & mask),
        ) && holds(self.uid, current.uid)
            && holds(self.gid, current.gid)
            && holds(self.read_only, current.read_only)
            && holds(self.hidden, current.hidden)
            && holds(self.archive, current.archive)
    }

    /// Builds the change that moves an entry from its current state back to `previous`,
    /// touching only the fields this change modified.
    #[must_use]
    pub const fn inverse_from(&self, previous: &AttributeSnapshot) -> Self {
        Self {
            mode: if self.mode.is_some() {
                previous.mode
            } else {
                None
            },
            mode_mask: if self.mode.is_some() {
                self.mode_mask
            } else {
                None
            },
            uid: if self.uid.is_some() {
                previous.uid
            } else {
                None
            },
            gid: if self.gid.is_some() {
                previous.gid
            } else {
                None
            },
            read_only: if self.read_only.is_some() {
                previous.read_only
            } else {
                None
            },
            hidden: if self.hidden.is_some() {
                previous.hidden
            } else {
                None
            },
            archive: if self.archive.is_some() {
                previous.archive
            } else {
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_only_restores_fields_the_change_touched() {
        let change = AttributeChange {
            mode: Some(0o600),
            hidden: Some(true),
            ..AttributeChange::default()
        };
        let previous = AttributeSnapshot {
            mode: Some(0o644),
            uid: Some(501),
            hidden: Some(false),
            ..AttributeSnapshot::default()
        };
        assert_eq!(
            change.inverse_from(&previous),
            AttributeChange {
                mode: Some(0o644),
                hidden: Some(false),
                ..AttributeChange::default()
            }
        );
    }

    #[test]
    fn is_applied_to_compares_only_the_fields_the_change_sets() {
        let current = AttributeSnapshot {
            mode: Some(0o640),
            uid: Some(1),
            ..AttributeSnapshot::default()
        };
        let mode = |mode| AttributeChange {
            mode: Some(mode),
            ..AttributeChange::default()
        };
        assert!(mode(0o640).is_applied_to(&current));
        assert!(
            mode(0o100_640).is_applied_to(&current),
            "file-type bits are ignored"
        );
        assert!(!mode(0o600).is_applied_to(&current));
        assert!(AttributeChange::default().is_applied_to(&current));
        assert!(
            !AttributeChange {
                hidden: Some(true),
                ..AttributeChange::default()
            }
            .is_applied_to(&current),
            "an attribute the provider does not expose cannot match"
        );
    }

    #[test]
    fn mode_mask_limits_which_bits_change_and_is_inverted_with_the_same_mask() {
        let change = AttributeChange {
            mode: Some(0o070),
            mode_mask: Some(0o070),
            ..AttributeChange::default()
        };
        assert_eq!(change.resolve_mode(0o4604), Some(0o4674));
        let absolute = AttributeChange {
            mode: Some(0o600),
            ..AttributeChange::default()
        };
        assert_eq!(absolute.resolve_mode(0o755), Some(0o600));
        let previous = AttributeSnapshot {
            mode: Some(0o4604),
            ..AttributeSnapshot::default()
        };
        let inverse = change.inverse_from(&previous);
        assert_eq!(inverse.resolve_mode(0o4674), Some(0o4604));
        assert!(change.is_applied_to(&AttributeSnapshot {
            mode: Some(0o3071),
            ..AttributeSnapshot::default()
        }));
    }

    #[test]
    fn exceeds_detects_unsupported_requests() {
        let mode_only = AttributeSupport {
            mode: true,
            ..AttributeSupport::default()
        };
        assert!(
            AttributeChange {
                uid: Some(0),
                ..AttributeChange::default()
            }
            .exceeds(mode_only)
        );
        assert!(
            !AttributeChange {
                mode: Some(0o700),
                ..AttributeChange::default()
            }
            .exceeds(mode_only)
        );
        assert!(AttributeChange::default().is_empty());
    }
}
