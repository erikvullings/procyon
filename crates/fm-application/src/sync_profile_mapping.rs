//! Settings <-> wire mapping for saved synchronization profiles (task 0161).

use fm_settings::{
    ConflictPolicy, SyncProfile, SyncProfileCriteria, SyncProfileMode, SyncRunOutcome,
    SyncRunSummary, SyncRunTrigger, SyncSchedule, SyncScheduleConfirmation,
};
use fm_transport_dto::{
    ComparisonCriteriaDto, ConflictPolicyDto, SyncModeDto, SyncProfileDto, SyncRunOutcomeDto,
    SyncRunSummaryDto, SyncRunTriggerDto, SyncScheduleConfirmationDto, SyncScheduleDto,
};

pub(crate) fn sync_profile_to_dto(profile: SyncProfile) -> SyncProfileDto {
    SyncProfileDto {
        id: profile.id,
        name: profile.name,
        left: profile.left.into(),
        right: profile.right.into(),
        mode: match profile.mode {
            SyncProfileMode::MirrorLeftToRight => SyncModeDto::MirrorLeftToRight,
            SyncProfileMode::MirrorRightToLeft => SyncModeDto::MirrorRightToLeft,
            SyncProfileMode::TwoWayUpdate => SyncModeDto::TwoWayUpdate,
        },
        criteria: match profile.criteria {
            SyncProfileCriteria::NameOnly => ComparisonCriteriaDto::NameOnly,
            SyncProfileCriteria::SizeAndTimestamp => ComparisonCriteriaDto::SizeAndTimestamp,
            SyncProfileCriteria::ContentHash => ComparisonCriteriaDto::ContentHash,
            SyncProfileCriteria::Content => ComparisonCriteriaDto::Content,
        },
        show_hidden: profile.show_hidden,
        include: profile.include,
        exclude: profile.exclude,
        conflict_policy: match profile.conflict_policy {
            ConflictPolicy::Ask => ConflictPolicyDto::Ask,
            ConflictPolicy::Overwrite => ConflictPolicyDto::Overwrite,
            ConflictPolicy::KeepBoth => ConflictPolicyDto::KeepBoth,
            ConflictPolicy::Skip => ConflictPolicyDto::Skip,
        },
        schedule: profile.schedule.map(|schedule| SyncScheduleDto {
            enabled: schedule.enabled,
            interval_minutes: schedule.interval_minutes,
            confirmation: match schedule.confirmation {
                SyncScheduleConfirmation::Required => SyncScheduleConfirmationDto::Required,
                SyncScheduleConfirmation::AutoApplyAdditions => {
                    SyncScheduleConfirmationDto::AutoApplyAdditions
                }
            },
        }),
        last_run: profile.last_run.map(|run| SyncRunSummaryDto {
            at: run.at,
            trigger: match run.trigger {
                SyncRunTrigger::Manual => SyncRunTriggerDto::Manual,
                SyncRunTrigger::Scheduled => SyncRunTriggerDto::Scheduled,
            },
            outcome: match run.outcome {
                SyncRunOutcome::Planned => SyncRunOutcomeDto::Planned,
                SyncRunOutcome::Applied => SyncRunOutcomeDto::Applied,
                SyncRunOutcome::UpToDate => SyncRunOutcomeDto::UpToDate,
                SyncRunOutcome::Dismissed => SyncRunOutcomeDto::Dismissed,
                SyncRunOutcome::Failed => SyncRunOutcomeDto::Failed,
            },
            planned_items: run.planned_items,
            applied_items: run.applied_items,
            message: run.message,
        }),
    }
}

pub(crate) fn sync_profile_from_dto(profile: SyncProfileDto) -> SyncProfile {
    SyncProfile {
        id: profile.id,
        name: profile.name,
        left: profile.left.into(),
        right: profile.right.into(),
        mode: match profile.mode {
            SyncModeDto::MirrorLeftToRight => SyncProfileMode::MirrorLeftToRight,
            SyncModeDto::MirrorRightToLeft => SyncProfileMode::MirrorRightToLeft,
            SyncModeDto::TwoWayUpdate => SyncProfileMode::TwoWayUpdate,
        },
        criteria: match profile.criteria {
            ComparisonCriteriaDto::NameOnly => SyncProfileCriteria::NameOnly,
            ComparisonCriteriaDto::SizeAndTimestamp => SyncProfileCriteria::SizeAndTimestamp,
            ComparisonCriteriaDto::ContentHash => SyncProfileCriteria::ContentHash,
            ComparisonCriteriaDto::Content => SyncProfileCriteria::Content,
        },
        show_hidden: profile.show_hidden,
        include: profile.include,
        exclude: profile.exclude,
        conflict_policy: match profile.conflict_policy {
            ConflictPolicyDto::Ask => ConflictPolicy::Ask,
            ConflictPolicyDto::Overwrite => ConflictPolicy::Overwrite,
            ConflictPolicyDto::KeepBoth => ConflictPolicy::KeepBoth,
            ConflictPolicyDto::Skip => ConflictPolicy::Skip,
        },
        schedule: profile.schedule.map(|schedule| SyncSchedule {
            enabled: schedule.enabled,
            interval_minutes: schedule.interval_minutes,
            confirmation: match schedule.confirmation {
                SyncScheduleConfirmationDto::Required => SyncScheduleConfirmation::Required,
                SyncScheduleConfirmationDto::AutoApplyAdditions => {
                    SyncScheduleConfirmation::AutoApplyAdditions
                }
            },
        }),
        last_run: profile.last_run.map(|run| SyncRunSummary {
            at: run.at,
            trigger: match run.trigger {
                SyncRunTriggerDto::Manual => SyncRunTrigger::Manual,
                SyncRunTriggerDto::Scheduled => SyncRunTrigger::Scheduled,
            },
            outcome: match run.outcome {
                SyncRunOutcomeDto::Planned => SyncRunOutcome::Planned,
                SyncRunOutcomeDto::Applied => SyncRunOutcome::Applied,
                SyncRunOutcomeDto::UpToDate => SyncRunOutcome::UpToDate,
                SyncRunOutcomeDto::Dismissed => SyncRunOutcome::Dismissed,
                SyncRunOutcomeDto::Failed => SyncRunOutcome::Failed,
            },
            planned_items: run.planned_items,
            applied_items: run.applied_items,
            message: run.message,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::{Location, ProviderId};

    #[test]
    fn profile_survives_the_dto_round_trip_with_schedule_and_last_run() {
        let profile = SyncProfile {
            id: "p1".into(),
            name: "Photos".into(),
            left: Location::new(ProviderId::new("local"), "file:///a"),
            right: Location::new(ProviderId::new("sftp"), "sftp://me@nas/b"),
            mode: SyncProfileMode::TwoWayUpdate,
            criteria: SyncProfileCriteria::Content,
            show_hidden: true,
            include: vec!["*.jpg".into()],
            exclude: vec!["cache".into()],
            conflict_policy: ConflictPolicy::Skip,
            schedule: Some(SyncSchedule {
                enabled: true,
                interval_minutes: 30,
                confirmation: SyncScheduleConfirmation::AutoApplyAdditions,
            }),
            last_run: Some(SyncRunSummary {
                at: "2026-10-24T10:00:00Z".into(),
                trigger: SyncRunTrigger::Scheduled,
                outcome: SyncRunOutcome::Failed,
                planned_items: 2,
                applied_items: 1,
                message: Some("Connection missing".into()),
            }),
        };
        assert_eq!(
            sync_profile_from_dto(sync_profile_to_dto(profile.clone())),
            profile
        );
    }
}
