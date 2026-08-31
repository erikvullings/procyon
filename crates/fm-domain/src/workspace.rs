//! Workspace, pane and tab state (spec §5.3).
//!
//! The engine never assumes exactly two panes: [`WorkspaceLayout`] is a
//! recursive binary split tree, so a three-or-more-pane layout nests further
//! splits instead of requiring a data-model rewrite.
//!
//! Only the durable, persisted configuration lives here (spec §5.3.3's
//! `WorkspaceDefinition` layer). Process-local runtime state (`WorkspaceRuntime`)
//! and frontend-only cursor/selection/dialog state (`WorkspaceViewState`, task
//! 0082) are deliberately out of scope for this crate: [`DirectoryViewConfiguration`]
//! holds only persisted view configuration and cannot represent selection or
//! cursor state.

use std::collections::HashSet;
use std::ops::RangeInclusive;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{PaneId, TabId, WorkspaceId};
use crate::location::Location;

/// Current schema version for persisted [`Workspace`] data (spec §5.3.6
/// invariant 13). [`Workspace::validate`] rejects any `schema_version` newer
/// than this; an older version is expected to already have been migrated up
/// to this version before validation runs (see `fm_application`'s
/// `WorkspaceService` startup lifecycle, task 0079).
pub const CURRENT_WORKSPACE_SCHEMA_VERSION: u32 = 4;

/// Maximum number of locations retained across both sides of a tab's
/// navigation history (spec §5.3.4, §5.3.6 invariant 10).
pub const MAX_NAVIGATION_HISTORY_LEN: usize = 100;

/// Allowed range for a [`WorkspaceLayout::Split`] ratio (spec §5.3.5, §5.3.6
/// invariant 8).
pub const SPLIT_RATIO_RANGE: RangeInclusive<f32> = 0.1..=0.9;

/// A workspace: a named collection of panes arranged in a [`WorkspaceLayout`]
/// (spec §5.3.3's `WorkspaceDefinition`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    /// Version of the persisted workspace schema, for migrations.
    pub schema_version: u32,
    /// Stable identifier for this workspace.
    pub id: WorkspaceId,
    /// A user-facing name for this workspace.
    pub name: String,
    /// How the panes are arranged on screen.
    pub layout: WorkspaceLayout,
    /// The panes making up this workspace. Never assumed to be exactly two.
    pub panes: Vec<PaneState>,
    /// The pane that currently has focus.
    pub active_pane_id: PaneId,
    /// Operation-centre visibility and sizing preferences for this workspace.
    pub operation_centre: OperationCentrePreferences,
    /// When this workspace was first created.
    pub created_at: DateTime<Utc>,
    /// When this workspace was last persisted.
    pub updated_at: DateTime<Utc>,
    /// Monotonically increasing revision, used for optimistic conflict checks.
    pub revision: u64,
    /// True for a per-window fork created for one desktop window's private use, false
    /// for a named/template workspace that only changes when explicitly resynced.
    /// Ephemeral workspaces are excluded from the workspace switcher and never become
    /// the last-active workspace (spec follow-up: ephemeral per-window workspaces).
    /// `#[serde(default)]` lets hand-written or pre-this-field JSON deserialize as a
    /// named (non-ephemeral) workspace without going through `migrate_v3_to_v4` -
    /// the on-disk persistence path still migrates explicitly regardless.
    #[serde(default)]
    pub ephemeral: bool,
    /// The named workspace this ephemeral workspace was forked from, if any. `None`
    /// for a non-ephemeral workspace, and also `None` for an ephemeral workspace
    /// seeded from the hardcoded default (no template existed yet) until its first
    /// resync links it to a newly created named workspace.
    #[serde(default)]
    pub forked_from: Option<WorkspaceId>,
}

/// Workspace-level operation-centre visibility and sizing preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationCentrePreferences {
    /// Whether the operation centre panel is visible.
    pub visible: bool,
    /// The panel's height in pixels.
    pub height: u32,
}

/// A single pane, holding one or more tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneState {
    /// Stable identifier for this pane.
    pub id: PaneId,
    /// An optional user-facing title override for this pane.
    pub title: Option<String>,
    /// The tabs open in this pane.
    pub tabs: Vec<TabState>,
    /// The tab currently shown in this pane.
    pub active_tab_id: TabId,
    /// The view configuration new tabs in this pane start from.
    pub default_view: DirectoryViewConfiguration,
}

/// A single tab: a location, its navigation history and its view configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    /// Stable identifier for this tab.
    pub id: TabId,
    /// The location currently shown in this tab.
    pub location: Location,
    /// An optional user-facing title override for this tab.
    pub title_override: Option<String>,
    /// Back/forward navigation history for this tab.
    pub history: NavigationHistory,
    /// Persisted view configuration (sort, columns, filters) for this tab.
    pub view: DirectoryViewConfiguration,
    /// Whether this tab is pinned (protected from ordinary "close tab" actions).
    pub pinned: bool,
    /// Whether this tab belongs only to the current application session.
    #[serde(default)]
    pub transient: bool,
}

/// Back/forward navigation history for a single tab.
///
/// Deviation from spec §5.3.4: the spec's `NavigationHistory` adds an explicit
/// `current: Location` field. This type deliberately omits it and keeps
/// [`TabState::location`] as the single source of truth for the current
/// location, so navigating never requires updating two fields in lockstep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationHistory {
    /// Locations reachable by navigating back, most recent last.
    pub back: Vec<Location>,
    /// Locations reachable by navigating forward, most recent last.
    pub forward: Vec<Location>,
}

/// Persisted view configuration for a directory listing: sorting, columns and
/// filters (spec §5.3.4).
///
/// Contains no frontend-only fields: current row selection and keyboard
/// cursor are frontend session state (spec §5.3.2) and are never represented
/// here, so a workspace save can never persist them by accident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryViewConfiguration {
    /// The active sort descriptors, in priority order. A single element
    /// today; kept as a list so multiple sort keys (spec §15) need no
    /// data-model change.
    pub sort: Vec<SortDescriptor>,
    /// Per-column width and visibility.
    pub columns: Vec<ColumnConfiguration>,
    /// Whether hidden entries are shown.
    pub show_hidden: bool,
    /// Whether directories are grouped before files.
    pub folders_first: bool,
    /// A persisted quick-filter query, if one is saved with the tab.
    pub quick_filter: Option<PersistedFilter>,
    /// The active view mode (task 0134). `#[serde(default)]` so a workspace
    /// saved before this field existed still deserializes, defaulting to the
    /// table view it was already showing.
    #[serde(default)]
    pub view_mode: DirectoryViewMode,
    /// Grid tile size, used only when `view_mode` is [`DirectoryViewMode::Grid`]
    /// (task 0134).
    #[serde(default)]
    pub icon_size: IconSize,
}

/// The active view mode for a directory listing tab (task 0134): the
/// existing dense table, or a thumbnail grid. Deliberately an open-ended
/// enum (not a bool) so a future "brief"/"full details" mode - flagged as a
/// prerequisite by task 0129's view-mode cluster - can slot in beside
/// [`Self::Grid`] without another data-model change; only `Table`/`Grid`
/// ship now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DirectoryViewMode {
    /// The existing dense, row-based listing (task 0024).
    #[default]
    Table,
    /// A grid of larger thumbnails with the filename below (task 0134).
    Grid,
}

/// Grid tile / icon size (task 0134 acceptance criteria: "Icon size is
/// small, medium and large").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IconSize {
    /// Icon-sized, for the directory table's icon column.
    Small,
    /// The default grid-view tile size.
    #[default]
    Medium,
    /// The largest grid-view tile size.
    Large,
}

/// A single sort descriptor: a column and a direction.
///
/// Uses an open, string-valued `column_id` (matching [`ColumnConfiguration`])
/// rather than a closed field enum, so plugin-provided columns can be sorted
/// on the same footing as built-in ones (spec §5.3.6 invariant 12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortDescriptor {
    /// The column to sort by, e.g. `"core.name"` or a plugin-provided column.
    pub column_id: String,
    /// The direction to sort in.
    pub direction: SortDirection,
}

/// A sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    /// Smallest/earliest first.
    Ascending,
    /// Largest/latest first.
    Descending,
}

/// Persisted width and visibility for a single directory-table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnConfiguration {
    /// The column's identifier, e.g. `"core.name"` or a plugin-provided column.
    pub column_id: String,
    /// The column's width in pixels.
    pub width: u32,
    /// Whether the column is currently visible.
    pub visible: bool,
}

/// A persisted quick-filter query (spec §24: plain text initially, glob or
/// regex support is a later addition).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedFilter {
    /// The filter's plain-text query.
    pub query: String,
}

/// How a workspace's panes are arranged on screen.
///
/// A recursive binary tree of splits: each [`WorkspaceLayout::Split`] is
/// exactly the two sides of one draggable splitter, so any number of panes
/// can be represented by nesting further splits, without hard-coding an
/// assumption of exactly two panes at the workspace level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkspaceLayout {
    /// A leaf holding a single pane.
    #[serde(rename_all = "camelCase")]
    Pane {
        /// The pane occupying this leaf.
        pane_id: PaneId,
    },
    /// Two regions separated by a single draggable splitter.
    #[serde(rename_all = "camelCase")]
    Split {
        /// The axis the splitter is arranged on.
        axis: SplitAxis,
        /// The fraction of space (0.0-1.0) given to `first`.
        ratio: f32,
        /// The first (left or top) region.
        first: Box<WorkspaceLayout>,
        /// The second (right or bottom) region.
        second: Box<WorkspaceLayout>,
    },
}

/// The axis a [`WorkspaceLayout::Split`] is arranged on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SplitAxis {
    /// Side by side, splitter runs vertically.
    Horizontal,
    /// Stacked, splitter runs horizontally.
    Vertical,
}

/// A single violated invariant from [`Workspace::validate`] (spec §5.3.6).
///
/// Validation collects every violation instead of stopping at the first one,
/// so a corrupt or hand-edited workspace can be reported completely rather
/// than one opaque failure at a time.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WorkspaceValidationError {
    /// Invariant 1: two panes in the same workspace share a [`PaneId`].
    #[error("duplicate pane id {pane_id}")]
    DuplicatePaneId {
        /// The id that appears more than once.
        pane_id: PaneId,
    },
    /// Invariant 1: two tabs in the same pane share a [`TabId`].
    #[error("duplicate tab id {tab_id} in pane {pane_id}")]
    DuplicateTabId {
        /// The pane containing the duplicate.
        pane_id: PaneId,
        /// The id that appears more than once in that pane.
        tab_id: TabId,
    },
    /// Invariant 2: the workspace has no panes at all.
    #[error("workspace has no panes")]
    NoPanes,
    /// Invariant 3: a pane has no tabs at all.
    #[error("pane {pane_id} has no tabs")]
    EmptyPane {
        /// The empty pane.
        pane_id: PaneId,
    },
    /// Invariant 4: `active_pane_id` does not name an existing pane.
    #[error("active pane {pane_id} does not exist")]
    ActivePaneNotFound {
        /// The dangling pane id.
        pane_id: PaneId,
    },
    /// Invariant 5: a pane's `active_tab_id` does not name one of its own tabs.
    #[error("active tab {tab_id} in pane {pane_id} does not exist")]
    ActiveTabNotFound {
        /// The pane whose active tab is dangling.
        pane_id: PaneId,
        /// The dangling tab id.
        tab_id: TabId,
    },
    /// Invariant 6: the layout tree references a pane that does not exist.
    #[error("layout references unknown pane {pane_id}")]
    LayoutReferencesUnknownPane {
        /// The dangling pane id.
        pane_id: PaneId,
    },
    /// Invariant 7: a pane exists but never appears in the layout tree.
    #[error("pane {pane_id} does not appear in the layout")]
    PaneMissingFromLayout {
        /// The orphaned pane.
        pane_id: PaneId,
    },
    /// Invariant 7: a pane appears more than once in the layout tree.
    #[error("pane {pane_id} appears more than once in the layout")]
    PaneDuplicatedInLayout {
        /// The pane appearing multiple times.
        pane_id: PaneId,
    },
    /// Invariant 8: a split ratio is non-finite or outside [`SPLIT_RATIO_RANGE`].
    #[error("split ratio {ratio} is outside the allowed range")]
    InvalidSplitRatio {
        /// The offending ratio.
        ratio: f32,
    },
    /// Invariant 9: a tab's location is missing a provider id or URI.
    #[error("tab {tab_id} has an invalid location")]
    InvalidLocation {
        /// The tab with the invalid location.
        tab_id: TabId,
    },
    /// Invariant 10: a tab's navigation history exceeds [`MAX_NAVIGATION_HISTORY_LEN`].
    #[error("tab {tab_id} navigation history has {len} entries, exceeding the bound")]
    NavigationHistoryExceedsBound {
        /// The tab whose history is too long.
        tab_id: TabId,
        /// The offending combined length of `back` and `forward`.
        len: usize,
    },
    /// Invariant 11: two columns in the same view share a `column_id`.
    #[error("duplicate column id {column_id:?} in pane {pane_id}")]
    DuplicateColumnId {
        /// The pane the offending view belongs to.
        pane_id: PaneId,
        /// The tab the offending view belongs to, or `None` for a pane's `default_view`.
        tab_id: Option<TabId>,
        /// The id that appears more than once.
        column_id: String,
    },
    /// Invariant 13: `schema_version` is newer than this crate understands.
    #[error("workspace schema version {schema_version} is not supported")]
    UnsupportedSchemaVersion {
        /// The unsupported version.
        schema_version: u32,
    },
}

impl Workspace {
    /// Validates every invariant from spec §5.3.6, returning every violation
    /// found rather than stopping at the first one.
    ///
    /// Two invariants are deliberately not checked here:
    /// - Invariant 12 ("unknown plugin columns are preserved but marked
    ///   unavailable") has nothing for a structural validator to reject: there
    ///   is no known-column registry to compare against in `fm-domain`, so an
    ///   unrecognised `column_id` is never itself treated as invalid.
    /// - Invariant 14 ("the revision increases monotonically") requires
    ///   comparing against a previously persisted revision, which only
    ///   `WorkspaceRepository::save` has access to (`fm-application`, task
    ///   0079).
    pub fn validate(&self) -> Result<(), Vec<WorkspaceValidationError>> {
        let mut errors = Vec::new();

        if self.schema_version > CURRENT_WORKSPACE_SCHEMA_VERSION {
            errors.push(WorkspaceValidationError::UnsupportedSchemaVersion {
                schema_version: self.schema_version,
            });
        }

        if self.panes.is_empty() {
            errors.push(WorkspaceValidationError::NoPanes);
        }

        let mut seen_pane_ids = HashSet::new();
        for pane in &self.panes {
            if !seen_pane_ids.insert(pane.id) {
                errors.push(WorkspaceValidationError::DuplicatePaneId { pane_id: pane.id });
            }

            if pane.tabs.is_empty() {
                errors.push(WorkspaceValidationError::EmptyPane { pane_id: pane.id });
            }

            let mut seen_tab_ids = HashSet::new();
            for tab in &pane.tabs {
                if !seen_tab_ids.insert(tab.id) {
                    errors.push(WorkspaceValidationError::DuplicateTabId {
                        pane_id: pane.id,
                        tab_id: tab.id,
                    });
                }

                if tab.location.provider_id.as_str().is_empty() || tab.location.uri.is_empty() {
                    errors.push(WorkspaceValidationError::InvalidLocation { tab_id: tab.id });
                }

                let history_len = tab.history.back.len() + tab.history.forward.len();
                if history_len > MAX_NAVIGATION_HISTORY_LEN {
                    errors.push(WorkspaceValidationError::NavigationHistoryExceedsBound {
                        tab_id: tab.id,
                        len: history_len,
                    });
                }

                push_duplicate_column_errors(&mut errors, pane.id, Some(tab.id), &tab.view.columns);
            }

            if !pane.tabs.iter().any(|tab| tab.id == pane.active_tab_id) {
                errors.push(WorkspaceValidationError::ActiveTabNotFound {
                    pane_id: pane.id,
                    tab_id: pane.active_tab_id,
                });
            }

            push_duplicate_column_errors(&mut errors, pane.id, None, &pane.default_view.columns);
        }

        if !self.panes.iter().any(|pane| pane.id == self.active_pane_id) {
            errors.push(WorkspaceValidationError::ActivePaneNotFound {
                pane_id: self.active_pane_id,
            });
        }

        let mut layout_pane_ids = Vec::new();
        collect_layout_pane_ids(&self.layout, &mut layout_pane_ids, &mut errors);

        for pane in &self.panes {
            match layout_pane_ids.iter().filter(|id| **id == pane.id).count() {
                0 => errors
                    .push(WorkspaceValidationError::PaneMissingFromLayout { pane_id: pane.id }),
                1 => {}
                _ => errors
                    .push(WorkspaceValidationError::PaneDuplicatedInLayout { pane_id: pane.id }),
            }
        }

        let mut reported_unknown_layout_panes = HashSet::new();
        for pane_id in &layout_pane_ids {
            if !self.panes.iter().any(|pane| pane.id == *pane_id)
                && reported_unknown_layout_panes.insert(*pane_id)
            {
                errors.push(WorkspaceValidationError::LayoutReferencesUnknownPane {
                    pane_id: *pane_id,
                });
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Walks a layout tree, collecting every referenced [`PaneId`] (duplicates
/// included) and reporting any out-of-range split ratio along the way.
fn collect_layout_pane_ids(
    layout: &WorkspaceLayout,
    pane_ids: &mut Vec<PaneId>,
    errors: &mut Vec<WorkspaceValidationError>,
) {
    match layout {
        WorkspaceLayout::Pane { pane_id } => pane_ids.push(*pane_id),
        WorkspaceLayout::Split {
            ratio,
            first,
            second,
            ..
        } => {
            if !ratio.is_finite() || !SPLIT_RATIO_RANGE.contains(ratio) {
                errors.push(WorkspaceValidationError::InvalidSplitRatio { ratio: *ratio });
            }
            collect_layout_pane_ids(first, pane_ids, errors);
            collect_layout_pane_ids(second, pane_ids, errors);
        }
    }
}

/// Pushes an error for every `column_id` that appears more than once among
/// `columns` (invariant 11).
fn push_duplicate_column_errors(
    errors: &mut Vec<WorkspaceValidationError>,
    pane_id: PaneId,
    tab_id: Option<TabId>,
    columns: &[ColumnConfiguration],
) {
    let mut seen = HashSet::new();
    for column in columns {
        if !seen.insert(column.column_id.as_str()) {
            errors.push(WorkspaceValidationError::DuplicateColumnId {
                pane_id,
                tab_id,
                column_id: column.column_id.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProviderId;

    fn sample_view() -> DirectoryViewConfiguration {
        DirectoryViewConfiguration {
            sort: vec![SortDescriptor {
                column_id: "core.name".to_owned(),
                direction: SortDirection::Ascending,
            }],
            columns: vec![ColumnConfiguration {
                column_id: "core.name".to_owned(),
                width: 360,
                visible: true,
            }],
            show_hidden: true,
            folders_first: true,
            quick_filter: None,
            view_mode: DirectoryViewMode::Table,
            icon_size: IconSize::Medium,
        }
    }

    fn sample_tab() -> TabState {
        TabState {
            id: TabId::new(),
            location: Location::new(ProviderId::new("file"), "file:///Users/erik"),
            title_override: None,
            history: NavigationHistory {
                back: vec![Location::new(ProviderId::new("file"), "file:///Users")],
                forward: vec![],
            },
            view: sample_view(),
            pinned: false,
            transient: false,
        }
    }

    fn sample_workspace() -> Workspace {
        let pane_a = PaneId::new();
        let pane_b = PaneId::new();
        let tab = sample_tab();
        Workspace {
            schema_version: 1,
            id: WorkspaceId::new(),
            name: "Default".to_owned(),
            panes: vec![
                PaneState {
                    id: pane_a,
                    title: None,
                    tabs: vec![tab.clone()],
                    active_tab_id: tab.id,
                    default_view: sample_view(),
                },
                PaneState {
                    id: pane_b,
                    title: None,
                    tabs: vec![tab.clone()],
                    active_tab_id: tab.id,
                    default_view: sample_view(),
                },
            ],
            active_pane_id: pane_a,
            layout: WorkspaceLayout::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(WorkspaceLayout::Pane { pane_id: pane_a }),
                second: Box::new(WorkspaceLayout::Pane { pane_id: pane_b }),
            },
            operation_centre: OperationCentrePreferences {
                visible: true,
                height: 180,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
            revision: 1,
            ephemeral: false,
            forked_from: None,
        }
    }

    #[test]
    fn workspace_round_trips_through_serde_json_with_a_two_pane_layout() {
        let workspace = sample_workspace();

        let json = serde_json::to_string(&workspace).expect("serialization must succeed");
        let parsed: Workspace = serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(workspace, parsed);
    }

    #[test]
    fn workspace_layout_supports_more_than_two_panes_via_nested_splits() {
        let layout = WorkspaceLayout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.33,
            first: Box::new(WorkspaceLayout::Pane {
                pane_id: PaneId::new(),
            }),
            second: Box::new(WorkspaceLayout::Split {
                axis: SplitAxis::Vertical,
                ratio: 0.5,
                first: Box::new(WorkspaceLayout::Pane {
                    pane_id: PaneId::new(),
                }),
                second: Box::new(WorkspaceLayout::Pane {
                    pane_id: PaneId::new(),
                }),
            }),
        };

        let json = serde_json::to_string(&layout).expect("serialization must succeed");
        let parsed: WorkspaceLayout =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(layout, parsed);
    }

    #[test]
    fn workspace_layout_pane_is_a_struct_variant_matching_the_spec_json_shape() {
        let pane_id = PaneId::new();
        let layout = WorkspaceLayout::Pane { pane_id };

        let json = serde_json::to_string(&layout).expect("serialization must succeed");
        assert_eq!(json, format!(r#"{{"type":"pane","paneId":"{pane_id}"}}"#));
    }

    #[test]
    fn navigation_history_round_trips_with_empty_stacks() {
        let history = NavigationHistory {
            back: vec![],
            forward: vec![],
        };
        let json = serde_json::to_string(&history).expect("serialization must succeed");
        let parsed: NavigationHistory =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(history, parsed);
    }

    #[test]
    fn directory_view_configuration_multi_key_sort_can_hold_a_single_key_without_rewrite() {
        let view = DirectoryViewConfiguration {
            sort: vec![SortDescriptor {
                column_id: "core.modified".to_owned(),
                direction: SortDirection::Descending,
            }],
            ..sample_view()
        };
        assert_eq!(view.sort.len(), 1);

        let json = serde_json::to_string(&view).expect("serialization must succeed");
        let parsed: DirectoryViewConfiguration =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(view, parsed);
    }

    #[test]
    fn directory_view_configuration_defaults_view_mode_and_icon_size_for_pre_0134_json() {
        // A workspace saved before task 0134 introduced these fields must
        // still load, defaulting to the table view it was already showing.
        let json = serde_json::json!({
            "sort": [],
            "columns": [],
            "show_hidden": false,
            "folders_first": false,
            "quick_filter": null,
        });

        let parsed: DirectoryViewConfiguration =
            serde_json::from_value(json).expect("pre-0134 JSON must still deserialize");
        assert_eq!(parsed.view_mode, DirectoryViewMode::Table);
        assert_eq!(parsed.icon_size, IconSize::Medium);
    }

    #[test]
    fn directory_view_configuration_cannot_represent_selection_or_cursor_state() {
        let json = serde_json::json!({
            "sort": [],
            "columns": [],
            "showHidden": false,
            "foldersFirst": false,
            "quickFilter": null,
            "selectedEntryIds": ["not-a-real-field"],
            "cursorEntryId": "also-not-real",
        });

        let result: Result<DirectoryViewConfiguration, _> = serde_json::from_value(json);
        assert!(
            result.is_err(),
            "DirectoryViewConfiguration must reject selection/cursor fields, got {result:?}"
        );
    }

    /// Literal example from spec §5.3.15 (timestamps normalized to a fixed
    /// offset, values otherwise verbatim).
    /// Content transcribed from the spec §5.3.15 example, using this crate's
    /// own field-naming convention (snake_case, matching every other
    /// `fm-domain` type such as [`Location`]) rather than the wire-facing
    /// camelCase JSON shown in the spec. `WorkspaceLayout` is the one
    /// exception: it already carries `#[serde(tag = "type", rename_all =
    /// "camelCase")]` directly (verbatim from §5.3.5), so its keys stay
    /// camelCase here too. The literal, byte-for-byte camelCase JSON from
    /// §5.3.15 is exercised against `WorkspaceDto` in `fm-transport-dto`,
    /// which is the layer responsible for wire compatibility.
    const SPEC_EXAMPLE_JSON: &str = r#"{
      "schema_version": 1,
      "id": "985d4d6e-c37b-4135-90a0-ce0afe165fd9",
      "name": "Development",
      "revision": 12,
      "layout": {
        "type": "split",
        "axis": "horizontal",
        "ratio": 0.52,
        "first": { "type": "pane", "paneId": "11e67e3e-813c-44c5-9426-53be347ad5da" },
        "second": { "type": "pane", "paneId": "479ec0f0-0ea6-4a34-b67e-f654373596af" }
      },
      "panes": [
        {
          "id": "11e67e3e-813c-44c5-9426-53be347ad5da",
          "title": null,
          "active_tab_id": "97512c58-9cf8-4f17-a931-94f0be87a1da",
          "default_view": {
            "sort": [{ "column_id": "core.name", "direction": "Ascending" }],
            "columns": [
              { "column_id": "core.name", "width": 360, "visible": true },
              { "column_id": "core.size", "width": 100, "visible": true },
              { "column_id": "core.modified", "width": 170, "visible": true }
            ],
            "show_hidden": true,
            "folders_first": true,
            "quick_filter": null
          },
          "tabs": [
            {
              "id": "97512c58-9cf8-4f17-a931-94f0be87a1da",
              "location": { "provider_id": "local", "uri": "file:///Users/erik/dev" },
              "title_override": null,
              "history": { "back": [], "forward": [] },
              "view": {
                "sort": [{ "column_id": "core.name", "direction": "Ascending" }],
                "columns": [
                  { "column_id": "core.name", "width": 360, "visible": true },
                  { "column_id": "core.size", "width": 100, "visible": true },
                  { "column_id": "core.modified", "width": 170, "visible": true }
                ],
                "show_hidden": true,
                "folders_first": true,
                "quick_filter": null
              },
              "pinned": false
            }
          ]
        },
        {
          "id": "479ec0f0-0ea6-4a34-b67e-f654373596af",
          "title": null,
          "active_tab_id": "5e8be42f-d6ef-45fb-89ea-d77122076bc3",
          "default_view": {
            "sort": [{ "column_id": "core.modified", "direction": "Descending" }],
            "columns": [
              { "column_id": "core.name", "width": 340, "visible": true },
              { "column_id": "core.size", "width": 100, "visible": true },
              { "column_id": "core.modified", "width": 170, "visible": true }
            ],
            "show_hidden": false,
            "folders_first": true,
            "quick_filter": null
          },
          "tabs": [
            {
              "id": "5e8be42f-d6ef-45fb-89ea-d77122076bc3",
              "location": { "provider_id": "local", "uri": "file:///Users/erik/Downloads" },
              "title_override": null,
              "history": { "back": [], "forward": [] },
              "view": {
                "sort": [{ "column_id": "core.modified", "direction": "Descending" }],
                "columns": [
                  { "column_id": "core.name", "width": 340, "visible": true },
                  { "column_id": "core.size", "width": 100, "visible": true },
                  { "column_id": "core.modified", "width": 170, "visible": true }
                ],
                "show_hidden": false,
                "folders_first": true,
                "quick_filter": null
              },
              "pinned": false
            }
          ]
        }
      ],
      "active_pane_id": "11e67e3e-813c-44c5-9426-53be347ad5da",
      "operation_centre": { "visible": true, "height": 180 },
      "created_at": "2026-07-29T18:00:00+02:00",
      "updated_at": "2026-07-29T18:40:00+02:00"
    }"#;

    #[test]
    fn workspace_round_trips_against_the_literal_spec_example_json() {
        let workspace: Workspace =
            serde_json::from_str(SPEC_EXAMPLE_JSON).expect("the §5.3.15 example must deserialize");

        assert_eq!(workspace.schema_version, 1);
        assert_eq!(workspace.name, "Development");
        assert_eq!(workspace.revision, 12);
        assert_eq!(workspace.panes.len(), 2);
        assert!(workspace.operation_centre.visible);
        assert_eq!(workspace.operation_centre.height, 180);
        assert_eq!(
            workspace.panes[0].tabs[0].view.sort[0].column_id,
            "core.name"
        );
        assert!(!workspace.panes[0].tabs[0].pinned);
        assert!(!workspace.ephemeral);
        assert_eq!(workspace.forked_from, None);

        let json = serde_json::to_string(&workspace).expect("serialization must succeed");
        let round_tripped: Workspace =
            serde_json::from_str(&json).expect("re-deserialization must succeed");
        assert_eq!(workspace, round_tripped);
    }

    // §5.3.6 invariants. `sample_workspace()` is valid against every one of
    // them, so each test flips exactly one thing and asserts the matching
    // `WorkspaceValidationError` comes back. Invariant 14 (monotonically
    // increasing revision) is exercised at the repository layer in
    // `fm_application`, not here: see this module's `validate` doc comment.

    #[test]
    fn invariant_1_duplicate_pane_id_is_rejected() {
        let mut workspace = sample_workspace();
        let duplicate_id = workspace.panes[0].id;
        workspace.panes[1].id = duplicate_id;
        // Keep the layout in sync so this test isolates the duplicate-id check.
        workspace.layout = WorkspaceLayout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(WorkspaceLayout::Pane {
                pane_id: duplicate_id,
            }),
            second: Box::new(WorkspaceLayout::Pane {
                pane_id: duplicate_id,
            }),
        };

        let errors = workspace
            .validate()
            .expect_err("duplicate pane id must be rejected");
        assert!(errors.contains(&WorkspaceValidationError::DuplicatePaneId {
            pane_id: duplicate_id
        }));
    }

    #[test]
    fn invariant_1_duplicate_tab_id_within_a_pane_is_rejected() {
        let mut workspace = sample_workspace();
        let tab = workspace.panes[0].tabs[0].clone();
        workspace.panes[0].tabs.push(tab.clone());

        let errors = workspace
            .validate()
            .expect_err("duplicate tab id must be rejected");
        assert!(errors.contains(&WorkspaceValidationError::DuplicateTabId {
            pane_id: workspace.panes[0].id,
            tab_id: tab.id,
        }));
    }

    #[test]
    fn invariant_2_workspace_with_no_panes_is_rejected() {
        let mut workspace = sample_workspace();
        workspace.panes.clear();

        let errors = workspace
            .validate()
            .expect_err("empty workspace must be rejected");
        assert!(errors.contains(&WorkspaceValidationError::NoPanes));
    }

    #[test]
    fn invariant_3_pane_with_no_tabs_is_rejected() {
        let mut workspace = sample_workspace();
        workspace.panes[0].tabs.clear();

        let errors = workspace
            .validate()
            .expect_err("empty pane must be rejected");
        assert!(errors.contains(&WorkspaceValidationError::EmptyPane {
            pane_id: workspace.panes[0].id,
        }));
    }

    #[test]
    fn invariant_4_dangling_active_pane_id_is_rejected() {
        let mut workspace = sample_workspace();
        workspace.active_pane_id = PaneId::new();

        let errors = workspace
            .validate()
            .expect_err("dangling active pane must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::ActivePaneNotFound {
                pane_id: workspace.active_pane_id,
            })
        );
    }

    #[test]
    fn invariant_5_dangling_active_tab_id_is_rejected() {
        let mut workspace = sample_workspace();
        let dangling = TabId::new();
        workspace.panes[0].active_tab_id = dangling;

        let errors = workspace
            .validate()
            .expect_err("dangling active tab must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::ActiveTabNotFound {
                pane_id: workspace.panes[0].id,
                tab_id: dangling,
            })
        );
    }

    #[test]
    fn invariant_6_layout_referencing_unknown_pane_is_rejected() {
        let mut workspace = sample_workspace();
        let unknown = PaneId::new();
        let pane_b = workspace.panes[1].id;
        workspace.layout = WorkspaceLayout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(WorkspaceLayout::Pane { pane_id: unknown }),
            second: Box::new(WorkspaceLayout::Pane { pane_id: pane_b }),
        };

        let errors = workspace
            .validate()
            .expect_err("layout referencing an unknown pane must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::LayoutReferencesUnknownPane {
                pane_id: unknown,
            })
        );
    }

    #[test]
    fn invariant_7_pane_missing_from_layout_is_rejected() {
        let mut workspace = sample_workspace();
        let pane_a = workspace.panes[0].id;
        workspace.layout = WorkspaceLayout::Pane { pane_id: pane_a };

        let errors = workspace
            .validate()
            .expect_err("pane absent from the layout must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::PaneMissingFromLayout {
                pane_id: workspace.panes[1].id,
            })
        );
    }

    #[test]
    fn invariant_7_pane_duplicated_in_layout_is_rejected() {
        let mut workspace = sample_workspace();
        let pane_a = workspace.panes[0].id;
        workspace.layout = WorkspaceLayout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(WorkspaceLayout::Pane { pane_id: pane_a }),
            second: Box::new(WorkspaceLayout::Pane { pane_id: pane_a }),
        };

        let errors = workspace
            .validate()
            .expect_err("pane appearing twice in the layout must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::PaneDuplicatedInLayout { pane_id: pane_a })
        );
        assert!(
            errors.contains(&WorkspaceValidationError::PaneMissingFromLayout {
                pane_id: workspace.panes[1].id,
            })
        );
    }

    #[test]
    fn invariant_8_out_of_range_split_ratio_is_rejected() {
        let mut workspace = sample_workspace();
        if let WorkspaceLayout::Split { ratio, .. } = &mut workspace.layout {
            *ratio = 0.95;
        }

        let errors = workspace
            .validate()
            .expect_err("out-of-range ratio must be rejected");
        assert!(errors.contains(&WorkspaceValidationError::InvalidSplitRatio { ratio: 0.95 }));
    }

    #[test]
    fn invariant_8_non_finite_split_ratio_is_rejected() {
        let mut workspace = sample_workspace();
        if let WorkspaceLayout::Split { ratio, .. } = &mut workspace.layout {
            *ratio = f32::NAN;
        }

        let errors = workspace
            .validate()
            .expect_err("non-finite ratio must be rejected");
        assert!(errors.iter().any(|error| matches!(
            error,
            WorkspaceValidationError::InvalidSplitRatio { ratio } if ratio.is_nan()
        )));
    }

    #[test]
    fn invariant_9_tab_with_empty_uri_is_rejected() {
        let mut workspace = sample_workspace();
        let tab_id = workspace.panes[0].tabs[0].id;
        workspace.panes[0].tabs[0].location.uri.clear();

        let errors = workspace
            .validate()
            .expect_err("empty location uri must be rejected");
        assert!(errors.contains(&WorkspaceValidationError::InvalidLocation { tab_id }));
    }

    #[test]
    fn invariant_10_navigation_history_exceeding_the_bound_is_rejected() {
        let mut workspace = sample_workspace();
        let tab_id = workspace.panes[0].tabs[0].id;
        workspace.panes[0].tabs[0].history.back = (0..MAX_NAVIGATION_HISTORY_LEN + 1)
            .map(|i| Location::new(ProviderId::new("file"), format!("file:///{i}")))
            .collect();

        let errors = workspace
            .validate()
            .expect_err("history exceeding the bound must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::NavigationHistoryExceedsBound {
                tab_id,
                len: MAX_NAVIGATION_HISTORY_LEN + 1,
            })
        );
    }

    #[test]
    fn invariant_11_duplicate_column_id_in_a_tab_view_is_rejected() {
        let mut workspace = sample_workspace();
        let pane_id = workspace.panes[0].id;
        let column = workspace.panes[0].tabs[0].view.columns[0].clone();
        workspace.panes[0].tabs[0].view.columns.push(column.clone());

        let errors = workspace
            .validate()
            .expect_err("duplicate column id must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::DuplicateColumnId {
                pane_id,
                tab_id: Some(workspace.panes[0].tabs[0].id),
                column_id: column.column_id,
            })
        );
    }

    #[test]
    fn invariant_12_unrecognised_plugin_column_id_does_not_fail_validation() {
        let mut workspace = sample_workspace();
        workspace.panes[0].tabs[0]
            .view
            .columns
            .push(ColumnConfiguration {
                column_id: "plugins.example-plugin.rating".to_owned(),
                width: 80,
                visible: true,
            });

        assert!(
            workspace.validate().is_ok(),
            "an unrecognised, plugin-namespaced column id must never itself be a validation error"
        );
    }

    #[test]
    fn invariant_13_unsupported_schema_version_is_rejected() {
        let mut workspace = sample_workspace();
        workspace.schema_version = CURRENT_WORKSPACE_SCHEMA_VERSION + 1;

        let errors = workspace
            .validate()
            .expect_err("a schema version newer than this crate understands must be rejected");
        assert!(
            errors.contains(&WorkspaceValidationError::UnsupportedSchemaVersion {
                schema_version: CURRENT_WORKSPACE_SCHEMA_VERSION + 1,
            })
        );
    }

    #[test]
    fn a_freshly_built_sample_workspace_validates_successfully() {
        assert!(sample_workspace().validate().is_ok());
    }
}
