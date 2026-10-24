//! Saved synchronization profiles (task 0161).
//!
//! A profile is saved *input* to the comparison and sync-plan services: two provider-neutral
//! roots, direction, comparison mode, path filters and a conflict default. It never stores
//! credentials or connection-session tokens (location URIs are scrubbed on load and save), and a
//! schedule only ever describes *when to prepare a plan* and *whether a human must confirm it*.

use serde::{Deserialize, Serialize};

use crate::ConflictPolicy;
use fm_domain::Location;

/// Most profiles kept; later entries are dropped on load and save.
pub const MAX_SYNC_PROFILES: usize = 100;
/// Longest profile name, in characters.
pub const MAX_SYNC_PROFILE_NAME_CHARS: usize = 80;
/// Most include or exclude patterns per profile.
pub const MAX_SYNC_PATTERNS: usize = 100;
/// Longest single filter pattern, in characters.
pub const MAX_SYNC_PATTERN_CHARS: usize = 256;
/// Shortest allowed schedule interval, in minutes.
pub const MIN_SYNC_INTERVAL_MINUTES: u32 = 5;
/// Longest allowed schedule interval, in minutes (one week).
pub const MAX_SYNC_INTERVAL_MINUTES: u32 = 10_080;
/// Longest stored run message, in characters.
pub const MAX_SYNC_RUN_MESSAGE_CHARS: usize = 500;
/// Longest stored identifier, in characters.
pub const MAX_SYNC_PROFILE_ID_CHARS: usize = 64;

/// Which side wins when a profile is previewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SyncProfileMode {
    /// Left is the source of truth.
    #[default]
    MirrorLeftToRight,
    /// Right is the source of truth.
    MirrorRightToLeft,
    /// Newer copy wins; ambiguous differences are left alone.
    TwoWayUpdate,
}

/// How a profile compares entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SyncProfileCriteria {
    /// Names only.
    NameOnly,
    /// Size and modification time.
    #[default]
    SizeAndTimestamp,
    /// Content hash.
    ContentHash,
    /// Byte-for-byte content.
    Content,
}

/// Whether a scheduled run needs a human before anything changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SyncScheduleConfirmation {
    /// A person reviews and applies every scheduled plan (the default).
    #[default]
    Required,
    /// Only additions to a side that lacks the entry are applied unattended; overwrites and
    /// deletions still wait for a person.
    AutoApplyAdditions,
}

/// Optional schedule. Only honoured while the app (desktop or server-backed UI) is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncSchedule {
    /// Explicit opt-in; a stored schedule with `false` never fires.
    pub enabled: bool,
    /// Minutes between plan preparations.
    pub interval_minutes: u32,
    /// Human-confirmation policy for scheduled plans.
    pub confirmation: SyncScheduleConfirmation,
}

impl Default for SyncSchedule {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 60,
            confirmation: SyncScheduleConfirmation::Required,
        }
    }
}

/// What triggered a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SyncRunTrigger {
    /// The user ran the profile.
    #[default]
    Manual,
    /// The schedule fired.
    Scheduled,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SyncRunOutcome {
    /// A plan was prepared and awaits review.
    #[default]
    Planned,
    /// The plan was applied.
    Applied,
    /// Nothing differed.
    UpToDate,
    /// The user dismissed the plan without applying.
    Dismissed,
    /// The run failed before or while planning.
    Failed,
}

/// Summary of the most recent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncRunSummary {
    /// RFC 3339 timestamp.
    pub at: String,
    /// What started the run.
    pub trigger: SyncRunTrigger,
    /// How it ended.
    pub outcome: SyncRunOutcome,
    /// Rows in the plan.
    pub planned_items: u32,
    /// Rows applied.
    pub applied_items: u32,
    /// Actionable failure text, when the run failed.
    pub message: Option<String>,
}

/// A named, saved synchronization pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncProfile {
    /// Stable identifier, unique among profiles.
    pub id: String,
    /// User-visible name.
    pub name: String,
    /// Root compared as "left".
    pub left: Location,
    /// Root compared as "right".
    pub right: Location,
    /// Direction.
    pub mode: SyncProfileMode,
    /// Comparison mode.
    pub criteria: SyncProfileCriteria,
    /// Include hidden entries.
    pub show_hidden: bool,
    /// Glob patterns a path must match; empty admits all.
    pub include: Vec<String>,
    /// Glob patterns that are never planned.
    pub exclude: Vec<String>,
    /// Conflict default for copy rows whose destination exists.
    pub conflict_policy: ConflictPolicy,
    /// Optional schedule.
    pub schedule: Option<SyncSchedule>,
    /// Last-run summary.
    pub last_run: Option<SyncRunSummary>,
}

impl Default for SyncProfile {
    fn default() -> Self {
        let empty = Location::new(fm_domain::ProviderId::new("local"), "");
        Self {
            id: String::new(),
            name: String::new(),
            left: empty.clone(),
            right: empty,
            mode: SyncProfileMode::default(),
            criteria: SyncProfileCriteria::default(),
            show_hidden: false,
            include: Vec::new(),
            exclude: Vec::new(),
            conflict_policy: ConflictPolicy::Ask,
            schedule: None,
            last_run: None,
        }
    }
}

fn clean_patterns(patterns: &mut Vec<String>) {
    let mut cleaned: Vec<String> = Vec::new();
    for pattern in patterns.drain(..) {
        let trimmed: String = pattern
            .trim()
            .chars()
            .take(MAX_SYNC_PATTERN_CHARS)
            .collect();
        if !trimmed.is_empty() && !cleaned.contains(&trimmed) {
            cleaned.push(trimmed);
        }
    }
    cleaned.truncate(MAX_SYNC_PATTERNS);
    *patterns = cleaned;
}

/// Normalizes profiles in place: scrubs credentials from locations, trims and bounds text,
/// clamps schedule intervals, and drops profiles without a usable id, name or roots.
pub(crate) fn sanitize_sync_profiles(profiles: &mut Vec<SyncProfile>) {
    let mut seen: Vec<String> = Vec::new();
    profiles.retain_mut(|profile| {
        profile.id = profile
            .id
            .trim()
            .chars()
            .take(MAX_SYNC_PROFILE_ID_CHARS)
            .collect();
        profile.name = profile
            .name
            .trim()
            .chars()
            .take(MAX_SYNC_PROFILE_NAME_CHARS)
            .collect::<String>()
            .trim()
            .to_owned();
        profile.left.uri = crate::sanitize_persisted_location(&profile.left.uri);
        profile.right.uri = crate::sanitize_persisted_location(&profile.right.uri);
        if profile.id.is_empty()
            || profile.name.is_empty()
            || profile.left.uri.is_empty()
            || profile.right.uri.is_empty()
            || seen.contains(&profile.id)
        {
            return false;
        }
        seen.push(profile.id.clone());
        clean_patterns(&mut profile.include);
        clean_patterns(&mut profile.exclude);
        if let Some(schedule) = &mut profile.schedule {
            schedule.interval_minutes = schedule
                .interval_minutes
                .clamp(MIN_SYNC_INTERVAL_MINUTES, MAX_SYNC_INTERVAL_MINUTES);
        }
        if let Some(run) = &mut profile.last_run {
            run.at = run.at.chars().take(40).collect();
            run.message = run.message.take().map(|message| {
                message
                    .chars()
                    .take(MAX_SYNC_RUN_MESSAGE_CHARS)
                    .collect::<String>()
            });
        }
        true
    });
    profiles.truncate(MAX_SYNC_PROFILES);
}
