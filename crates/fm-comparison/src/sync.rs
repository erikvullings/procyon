//! Sync-plan generation (spec §16 milestone 5).
//!
//! [`generate_sync_plan`] is a pure, side-effect-free translation from a
//! materialized comparison into a reviewable, per-entry plan: it reads
//! nothing from and writes nothing to any filesystem, so generating (and
//! regenerating) a plan is always a dry run. The caller (spec §35: never
//! apply without confirmation) presents the plan for review/edits and only
//! then turns the accepted actions into ordinary `Copy`/`Trash` operations
//! through the existing operation engine — this module never touches a
//! provider.

use serde::{Deserialize, Serialize};

use crate::model::{ComparisonEntry, ComparisonEntrySide, ComparisonStatus};

/// Which side is authoritative when a sync plan proposes actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    /// The left side is the source of truth: differences copy left→right,
    /// and entries that exist only on the right are proposed for deletion.
    MirrorLeftToRight,
    /// The right side is the source of truth: differences copy right→left,
    /// and entries that exist only on the left are proposed for deletion.
    MirrorRightToLeft,
    /// Neither side is authoritative: only the newer copy of a differing
    /// entry is propagated, and ambiguous differences (same timestamp,
    /// different size or content) are left for the user to resolve.
    TwoWayUpdate,
}

/// A proposed action for one compared entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncAction {
    /// Copy the left entry onto the right side (recursively, if a directory).
    CopyLeftToRight,
    /// Copy the right entry onto the left side (recursively, if a directory).
    CopyRightToLeft,
    /// Remove the left entry.
    DeleteLeft,
    /// Remove the right entry.
    DeleteRight,
    /// Take no action.
    Skip,
}

/// One row of a sync plan: a proposed (and, before applying, user-editable)
/// action for a single compared path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncPlanItem {
    /// Path relative to both roots, using `/` separators.
    pub relative_path: String,
    /// The comparison outcome this action was proposed from.
    pub status: ComparisonStatus,
    /// The proposed action. The caller may overwrite this per row before
    /// applying (spec §35: the user reviews and edits before anything runs).
    pub action: SyncAction,
    /// Left-side metadata, for display.
    pub left: Option<ComparisonEntrySide>,
    /// Right-side metadata, for display.
    pub right: Option<ComparisonEntrySide>,
}

/// Proposes a sync plan from every non-identical entry in `entries`.
///
/// Entries already [`ComparisonStatus::Identical`] are omitted: there is
/// nothing to propose for them. Everything else is included, even when its
/// default action is [`SyncAction::Skip`] (for example
/// [`ComparisonStatus::TypeMismatch`]), so the caller can see and manually
/// resolve it.
#[must_use]
pub fn generate_sync_plan(entries: &[ComparisonEntry], mode: SyncMode) -> Vec<SyncPlanItem> {
    entries
        .iter()
        .filter(|entry| entry.status != ComparisonStatus::Identical)
        .map(|entry| SyncPlanItem {
            relative_path: entry.relative_path.clone(),
            status: entry.status,
            action: default_action(entry.status, mode),
            left: entry.left.clone(),
            right: entry.right.clone(),
        })
        .collect()
}

fn default_action(status: ComparisonStatus, mode: SyncMode) -> SyncAction {
    use ComparisonStatus::{
        DifferentSize, Identical, Newer, Older, OnlyLeft, OnlyRight, TypeMismatch,
    };
    match mode {
        SyncMode::MirrorLeftToRight => match status {
            OnlyLeft | Newer | Older | DifferentSize => SyncAction::CopyLeftToRight,
            OnlyRight => SyncAction::DeleteRight,
            Identical | TypeMismatch => SyncAction::Skip,
        },
        SyncMode::MirrorRightToLeft => match status {
            OnlyRight | Newer | Older | DifferentSize => SyncAction::CopyRightToLeft,
            OnlyLeft => SyncAction::DeleteLeft,
            Identical | TypeMismatch => SyncAction::Skip,
        },
        SyncMode::TwoWayUpdate => match status {
            OnlyLeft | Newer => SyncAction::CopyLeftToRight,
            OnlyRight | Older => SyncAction::CopyRightToLeft,
            DifferentSize | Identical | TypeMismatch => SyncAction::Skip,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::EntryKind;

    fn entry(relative_path: &str, status: ComparisonStatus) -> ComparisonEntry {
        let side = |present: bool| {
            present.then_some(ComparisonEntrySide {
                kind: EntryKind::File,
                size: Some(1),
                modified_at: None,
                content_hash: None,
            })
        };
        let (has_left, has_right) = match status {
            ComparisonStatus::OnlyLeft => (true, false),
            ComparisonStatus::OnlyRight => (false, true),
            _ => (true, true),
        };
        ComparisonEntry {
            relative_path: relative_path.to_owned(),
            left: side(has_left),
            right: side(has_right),
            status,
        }
    }

    #[test]
    fn identical_entries_are_omitted_from_every_mode() {
        let entries = vec![entry("same.txt", ComparisonStatus::Identical)];
        for mode in [
            SyncMode::MirrorLeftToRight,
            SyncMode::MirrorRightToLeft,
            SyncMode::TwoWayUpdate,
        ] {
            assert!(generate_sync_plan(&entries, mode).is_empty());
        }
    }

    #[test]
    fn mirror_left_to_right_copies_missing_and_differing_left_entries() {
        let entries = vec![
            entry("only-left.txt", ComparisonStatus::OnlyLeft),
            entry("newer.txt", ComparisonStatus::Newer),
            entry("older.txt", ComparisonStatus::Older),
            entry("size.txt", ComparisonStatus::DifferentSize),
        ];
        let plan = generate_sync_plan(&entries, SyncMode::MirrorLeftToRight);
        assert!(
            plan.iter()
                .all(|item| item.action == SyncAction::CopyLeftToRight)
        );
    }

    #[test]
    fn mirror_left_to_right_deletes_right_only_entries() {
        let entries = vec![entry("only-right.txt", ComparisonStatus::OnlyRight)];
        let plan = generate_sync_plan(&entries, SyncMode::MirrorLeftToRight);
        assert_eq!(plan[0].action, SyncAction::DeleteRight);
    }

    #[test]
    fn mirror_left_to_right_skips_type_mismatches_for_manual_resolution() {
        let entries = vec![entry("conflict", ComparisonStatus::TypeMismatch)];
        let plan = generate_sync_plan(&entries, SyncMode::MirrorLeftToRight);
        assert_eq!(plan.len(), 1, "the row is still surfaced for review");
        assert_eq!(plan[0].action, SyncAction::Skip);
    }

    #[test]
    fn mirror_right_to_left_is_the_mirror_image_of_mirror_left_to_right() {
        let entries = vec![
            entry("only-left.txt", ComparisonStatus::OnlyLeft),
            entry("only-right.txt", ComparisonStatus::OnlyRight),
            entry("newer.txt", ComparisonStatus::Newer),
        ];
        let plan = generate_sync_plan(&entries, SyncMode::MirrorRightToLeft);
        let action_for = |path: &str| {
            plan.iter()
                .find(|item| item.relative_path == path)
                .unwrap()
                .action
        };
        assert_eq!(action_for("only-left.txt"), SyncAction::DeleteLeft);
        assert_eq!(action_for("only-right.txt"), SyncAction::CopyRightToLeft);
        assert_eq!(action_for("newer.txt"), SyncAction::CopyRightToLeft);
    }

    #[test]
    fn two_way_update_propagates_only_the_newer_side() {
        let entries = vec![
            entry("only-left.txt", ComparisonStatus::OnlyLeft),
            entry("only-right.txt", ComparisonStatus::OnlyRight),
            entry("newer.txt", ComparisonStatus::Newer),
            entry("older.txt", ComparisonStatus::Older),
        ];
        let plan = generate_sync_plan(&entries, SyncMode::TwoWayUpdate);
        let action_for = |path: &str| {
            plan.iter()
                .find(|item| item.relative_path == path)
                .unwrap()
                .action
        };
        assert_eq!(action_for("only-left.txt"), SyncAction::CopyLeftToRight);
        assert_eq!(action_for("only-right.txt"), SyncAction::CopyRightToLeft);
        assert_eq!(action_for("newer.txt"), SyncAction::CopyLeftToRight);
        assert_eq!(action_for("older.txt"), SyncAction::CopyRightToLeft);
    }

    #[test]
    fn two_way_update_skips_ambiguous_size_differences() {
        let entries = vec![entry("ambiguous.txt", ComparisonStatus::DifferentSize)];
        let plan = generate_sync_plan(&entries, SyncMode::TwoWayUpdate);
        assert_eq!(plan[0].action, SyncAction::Skip);
    }

    #[test]
    fn generating_a_plan_never_touches_a_filesystem() {
        // No `std::fs` or provider call appears anywhere in this module;
        // this test documents the invariant so a future edit that adds one
        // is caught by review rather than by a slow integration test.
        let entries = vec![entry("a.txt", ComparisonStatus::OnlyLeft)];
        let plan_one = generate_sync_plan(&entries, SyncMode::MirrorLeftToRight);
        let plan_two = generate_sync_plan(&entries, SyncMode::MirrorLeftToRight);
        assert_eq!(plan_one, plan_two, "generation is deterministic and pure");
    }
}
