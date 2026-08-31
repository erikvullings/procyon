//! `fm-comparison` domain types <-> transport DTO conversions for directory comparison and sync
//! (task 0075).
//!
//! Split out of the `FileManagerService` facade (task 0119) — a self-contained cluster of pure
//! conversion functions with no dependency on the rest of the facade.

use fm_comparison::{
    ComparisonCriteria, ComparisonEntry, ComparisonEntrySide, ComparisonStatus, SyncAction,
    SyncMode, SyncPlanItem,
};
use fm_transport_dto::{
    ComparisonCriteriaDto, ComparisonEntryDto, ComparisonEntrySideDto, ComparisonStatusDto,
    SyncActionDto, SyncModeDto, SyncPlanItemDto,
};

pub(crate) const fn comparison_criteria(dto: ComparisonCriteriaDto) -> ComparisonCriteria {
    match dto {
        ComparisonCriteriaDto::NameOnly => ComparisonCriteria::NameOnly,
        ComparisonCriteriaDto::SizeAndTimestamp => ComparisonCriteria::SizeAndTimestamp,
        ComparisonCriteriaDto::ContentHash => ComparisonCriteria::ContentHash,
    }
}

pub(crate) const fn comparison_criteria_dto(criteria: ComparisonCriteria) -> ComparisonCriteriaDto {
    match criteria {
        ComparisonCriteria::NameOnly => ComparisonCriteriaDto::NameOnly,
        ComparisonCriteria::SizeAndTimestamp => ComparisonCriteriaDto::SizeAndTimestamp,
        ComparisonCriteria::ContentHash => ComparisonCriteriaDto::ContentHash,
    }
}

const fn comparison_status_dto(status: ComparisonStatus) -> ComparisonStatusDto {
    match status {
        ComparisonStatus::OnlyLeft => ComparisonStatusDto::OnlyLeft,
        ComparisonStatus::OnlyRight => ComparisonStatusDto::OnlyRight,
        ComparisonStatus::Newer => ComparisonStatusDto::Newer,
        ComparisonStatus::Older => ComparisonStatusDto::Older,
        ComparisonStatus::DifferentSize => ComparisonStatusDto::DifferentSize,
        ComparisonStatus::Identical => ComparisonStatusDto::Identical,
        ComparisonStatus::TypeMismatch => ComparisonStatusDto::TypeMismatch,
    }
}

fn comparison_entry_side_dto(side: &ComparisonEntrySide) -> ComparisonEntrySideDto {
    ComparisonEntrySideDto {
        kind: side.kind.into(),
        size: side.size,
        modified_at: side.modified_at,
        content_hash: side.content_hash.clone(),
    }
}

pub(crate) fn comparison_entry_dto(entry: &ComparisonEntry) -> ComparisonEntryDto {
    ComparisonEntryDto {
        relative_path: entry.relative_path.clone(),
        left: entry.left.as_ref().map(comparison_entry_side_dto),
        right: entry.right.as_ref().map(comparison_entry_side_dto),
        status: comparison_status_dto(entry.status),
    }
}

pub(crate) const fn sync_mode(dto: SyncModeDto) -> SyncMode {
    match dto {
        SyncModeDto::MirrorLeftToRight => SyncMode::MirrorLeftToRight,
        SyncModeDto::MirrorRightToLeft => SyncMode::MirrorRightToLeft,
        SyncModeDto::TwoWayUpdate => SyncMode::TwoWayUpdate,
    }
}

pub(crate) const fn sync_action(dto: SyncActionDto) -> SyncAction {
    match dto {
        SyncActionDto::CopyLeftToRight => SyncAction::CopyLeftToRight,
        SyncActionDto::CopyRightToLeft => SyncAction::CopyRightToLeft,
        SyncActionDto::DeleteLeft => SyncAction::DeleteLeft,
        SyncActionDto::DeleteRight => SyncAction::DeleteRight,
        SyncActionDto::Skip => SyncAction::Skip,
    }
}

const fn sync_action_dto(action: SyncAction) -> SyncActionDto {
    match action {
        SyncAction::CopyLeftToRight => SyncActionDto::CopyLeftToRight,
        SyncAction::CopyRightToLeft => SyncActionDto::CopyRightToLeft,
        SyncAction::DeleteLeft => SyncActionDto::DeleteLeft,
        SyncAction::DeleteRight => SyncActionDto::DeleteRight,
        SyncAction::Skip => SyncActionDto::Skip,
    }
}

pub(crate) fn sync_plan_item_dto(item: &SyncPlanItem) -> SyncPlanItemDto {
    SyncPlanItemDto {
        relative_path: item.relative_path.clone(),
        status: comparison_status_dto(item.status),
        action: sync_action_dto(item.action),
        left: item.left.as_ref().map(comparison_entry_side_dto),
        right: item.right.as_ref().map(comparison_entry_side_dto),
    }
}
