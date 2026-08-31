//! `Settings` (`fm-settings`) <-> `SettingsDto` (`fm-transport-dto`) conversion.
//!
//! Split out of the `FileManagerService` facade (task 0119) — a self-contained pair of pure
//! conversion functions with no dependency on the rest of the facade.

use fm_settings::{
    ConflictPolicy, DateFormat, DefaultPaneLayout, Language, MultiRenameCaseTransform,
    MultiRenamePreset, MultiRenameRules, MultiRenameSequence, Settings, SizeFormat, Theme,
};
use fm_transport_dto::{
    ConflictPolicyDto, DateFormatDto, DefaultPaneLayoutDto, LanguageDto,
    MultiRenameCaseTransformDto, MultiRenamePresetDto, MultiRenameRulesDto, MultiRenameSequenceDto,
    SavedSearchDto, SearchContentPredicateDto, SearchEntryKindDto, SearchGitStatusDto,
    SearchNameModeDto, SearchNamePredicateDto, SearchQueryDto, SearchScopeDto, SettingsDto,
    SizeFormatDto, ThemeDto,
};

pub(crate) fn settings_to_dto(settings: Settings) -> SettingsDto {
    SettingsDto {
        schema_version: settings.schema_version,
        theme: match settings.theme {
            Theme::Auto => ThemeDto::Auto,
            Theme::Light => ThemeDto::Light,
            Theme::Dark => ThemeDto::Dark,
        },
        language: match settings.language {
            Language::En => LanguageDto::En,
            Language::Nl => LanguageDto::Nl,
        },
        font_size: settings.font_size,
        row_height: settings.row_height,
        date_format: match settings.date_format {
            DateFormat::Short => DateFormatDto::Short,
            DateFormat::Medium => DateFormatDto::Medium,
            DateFormat::Iso => DateFormatDto::Iso,
        },
        size_format: match settings.size_format {
            SizeFormat::Binary => SizeFormatDto::Binary,
            SizeFormat::Decimal => SizeFormatDto::Decimal,
            SizeFormat::Bytes => SizeFormatDto::Bytes,
        },
        show_hidden_files: settings.show_hidden_files,
        confirm_permanent_delete: settings.confirm_permanent_delete,
        default_conflict_policy: match settings.default_conflict_policy {
            ConflictPolicy::Ask => ConflictPolicyDto::Ask,
            ConflictPolicy::Overwrite => ConflictPolicyDto::Overwrite,
            ConflictPolicy::KeepBoth => ConflictPolicyDto::KeepBoth,
            ConflictPolicy::Skip => ConflictPolicyDto::Skip,
        },
        operation_concurrency: settings.operation_concurrency,
        default_pane_layout: match settings.default_pane_layout {
            DefaultPaneLayout::Dual => DefaultPaneLayoutDto::Dual,
            DefaultPaneLayout::Single => DefaultPaneLayoutDto::Single,
        },
        default_columns: settings.default_columns,
        column_widths: settings.column_widths,
        keybindings: settings.keybindings,
        enabled_plugins: settings.enabled_plugins,
        plugin_settings: serde_json::to_value(settings.plugin_settings)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
        terminal_command: settings.terminal_command,
        editor_command: settings.editor_command,
        default_start_locations: settings.default_start_locations,
        favourite_locations: settings
            .favourite_locations
            .into_iter()
            .map(|favourite| fm_transport_dto::FavouriteLocationDto {
                label: favourite.label,
                location: favourite.location.into(),
            })
            .collect(),
        recent_locations_by_workspace: settings
            .recent_locations_by_workspace
            .into_iter()
            .map(|(workspace_id, locations)| {
                (
                    workspace_id,
                    locations.into_iter().map(Into::into).collect(),
                )
            })
            .collect(),
        multi_rename_presets: settings
            .multi_rename_presets
            .into_iter()
            .map(|preset| MultiRenamePresetDto {
                name: preset.name,
                rules: MultiRenameRulesDto {
                    search: preset.rules.search,
                    replace: preset.rules.replace,
                    use_regex: preset.rules.use_regex,
                    name_mask: preset.rules.name_mask,
                    extension_mask: preset.rules.extension_mask,
                    sequence: MultiRenameSequenceDto {
                        start: preset.rules.sequence.start,
                        step: preset.rules.sequence.step,
                        padding: preset.rules.sequence.padding,
                    },
                    case_transform: match preset.rules.case_transform {
                        MultiRenameCaseTransform::Unchanged => {
                            MultiRenameCaseTransformDto::Unchanged
                        }
                        MultiRenameCaseTransform::Upper => MultiRenameCaseTransformDto::Upper,
                        MultiRenameCaseTransform::Lower => MultiRenameCaseTransformDto::Lower,
                        MultiRenameCaseTransform::Title => MultiRenameCaseTransformDto::Title,
                    },
                },
            })
            .collect(),
        saved_searches: settings
            .saved_searches
            .into_iter()
            .map(|saved| SavedSearchDto {
                id: saved.id,
                name: saved.name,
                pinned: saved.pinned,
                query: search_query_to_dto(saved.query),
            })
            .collect(),
        icon_theme: settings.icon_theme,
    }
}

pub(crate) fn settings_from_dto(settings: SettingsDto) -> Settings {
    Settings {
        schema_version: fm_settings::CURRENT_SCHEMA_VERSION,
        theme: match settings.theme {
            ThemeDto::Auto => Theme::Auto,
            ThemeDto::Light => Theme::Light,
            ThemeDto::Dark => Theme::Dark,
        },
        language: match settings.language {
            LanguageDto::En => Language::En,
            LanguageDto::Nl => Language::Nl,
        },
        font_size: settings.font_size,
        row_height: settings.row_height,
        date_format: match settings.date_format {
            DateFormatDto::Short => DateFormat::Short,
            DateFormatDto::Medium => DateFormat::Medium,
            DateFormatDto::Iso => DateFormat::Iso,
        },
        size_format: match settings.size_format {
            SizeFormatDto::Binary => SizeFormat::Binary,
            SizeFormatDto::Decimal => SizeFormat::Decimal,
            SizeFormatDto::Bytes => SizeFormat::Bytes,
        },
        show_hidden_files: settings.show_hidden_files,
        confirm_permanent_delete: settings.confirm_permanent_delete,
        default_conflict_policy: match settings.default_conflict_policy {
            ConflictPolicyDto::Ask => ConflictPolicy::Ask,
            ConflictPolicyDto::Overwrite => ConflictPolicy::Overwrite,
            ConflictPolicyDto::KeepBoth => ConflictPolicy::KeepBoth,
            ConflictPolicyDto::Skip => ConflictPolicy::Skip,
        },
        operation_concurrency: settings.operation_concurrency,
        default_pane_layout: match settings.default_pane_layout {
            DefaultPaneLayoutDto::Dual => DefaultPaneLayout::Dual,
            DefaultPaneLayoutDto::Single => DefaultPaneLayout::Single,
        },
        default_columns: settings.default_columns,
        column_widths: settings.column_widths,
        keybindings: settings.keybindings,
        enabled_plugins: settings.enabled_plugins,
        plugin_settings: serde_json::from_value(settings.plugin_settings).unwrap_or_default(),
        terminal_command: settings.terminal_command,
        editor_command: settings.editor_command,
        default_start_locations: settings.default_start_locations,
        favourite_locations: settings
            .favourite_locations
            .into_iter()
            .map(|favourite| fm_settings::FavouriteLocation {
                label: favourite.label,
                location: favourite.location.into(),
            })
            .collect(),
        recent_locations_by_workspace: settings
            .recent_locations_by_workspace
            .into_iter()
            .map(|(workspace_id, locations)| {
                (
                    workspace_id,
                    locations.into_iter().map(Into::into).collect(),
                )
            })
            .collect(),
        multi_rename_presets: settings
            .multi_rename_presets
            .into_iter()
            .map(|preset| MultiRenamePreset {
                name: preset.name,
                rules: MultiRenameRules {
                    search: preset.rules.search,
                    replace: preset.rules.replace,
                    use_regex: preset.rules.use_regex,
                    name_mask: preset.rules.name_mask,
                    extension_mask: preset.rules.extension_mask,
                    sequence: MultiRenameSequence {
                        start: preset.rules.sequence.start,
                        step: preset.rules.sequence.step,
                        padding: preset.rules.sequence.padding,
                    },
                    case_transform: match preset.rules.case_transform {
                        MultiRenameCaseTransformDto::Unchanged => {
                            MultiRenameCaseTransform::Unchanged
                        }
                        MultiRenameCaseTransformDto::Upper => MultiRenameCaseTransform::Upper,
                        MultiRenameCaseTransformDto::Lower => MultiRenameCaseTransform::Lower,
                        MultiRenameCaseTransformDto::Title => MultiRenameCaseTransform::Title,
                    },
                },
            })
            .collect(),
        saved_searches: settings
            .saved_searches
            .into_iter()
            .map(|saved| fm_domain::SavedSearch {
                id: saved.id,
                name: saved.name,
                pinned: saved.pinned,
                query: search_query_from_dto(saved.query),
            })
            .collect(),
        icon_theme: settings.icon_theme,
    }
}

fn search_query_to_dto(query: fm_domain::SearchQuery) -> SearchQueryDto {
    SearchQueryDto {
        schema_version: query.schema_version,
        scope: SearchScopeDto {
            locations: query.scope.locations.into_iter().map(Into::into).collect(),
            recurse: query.scope.recurse,
            show_hidden: query.scope.show_hidden,
        },
        name: query.name.map(|name| SearchNamePredicateDto {
            pattern: name.pattern,
            mode: match name.mode {
                fm_domain::SearchNameMode::Substring => SearchNameModeDto::Substring,
                fm_domain::SearchNameMode::Glob => SearchNameModeDto::Glob,
            },
            case_sensitive: name.case_sensitive,
        }),
        entry_kinds: query
            .entry_kinds
            .into_iter()
            .map(|kind| match kind {
                fm_domain::SearchEntryKind::File => SearchEntryKindDto::File,
                fm_domain::SearchEntryKind::Directory => SearchEntryKindDto::Directory,
                fm_domain::SearchEntryKind::Symlink => SearchEntryKindDto::Symlink,
            })
            .collect(),
        mime_types: query.mime_types,
        min_size_bytes: query.min_size_bytes,
        max_size_bytes: query.max_size_bytes,
        modified_after: query.modified_after,
        modified_before: query.modified_before,
        content: query.content.map(|content| SearchContentPredicateDto {
            query: content.query,
            regex: content.regex,
            case_sensitive: content.case_sensitive,
            whole_word: content.whole_word,
        }),
        git_statuses: query
            .git_statuses
            .into_iter()
            .map(|status| match status {
                fm_domain::GitFileStatus::Clean => SearchGitStatusDto::Clean,
                fm_domain::GitFileStatus::Modified => SearchGitStatusDto::Modified,
                fm_domain::GitFileStatus::Staged => SearchGitStatusDto::Staged,
                fm_domain::GitFileStatus::Untracked => SearchGitStatusDto::Untracked,
                fm_domain::GitFileStatus::Ignored => SearchGitStatusDto::Ignored,
            })
            .collect(),
        tags: query.tags,
        metadata: query.metadata,
    }
}

fn search_query_from_dto(query: SearchQueryDto) -> fm_domain::SearchQuery {
    fm_domain::SearchQuery {
        schema_version: query.schema_version,
        scope: fm_domain::SearchScope {
            locations: query.scope.locations.into_iter().map(Into::into).collect(),
            recurse: query.scope.recurse,
            show_hidden: query.scope.show_hidden,
        },
        name: query.name.map(|name| fm_domain::SearchNamePredicate {
            pattern: name.pattern,
            mode: match name.mode {
                SearchNameModeDto::Substring => fm_domain::SearchNameMode::Substring,
                SearchNameModeDto::Glob => fm_domain::SearchNameMode::Glob,
            },
            case_sensitive: name.case_sensitive,
        }),
        entry_kinds: query
            .entry_kinds
            .into_iter()
            .map(|kind| match kind {
                SearchEntryKindDto::File => fm_domain::SearchEntryKind::File,
                SearchEntryKindDto::Directory => fm_domain::SearchEntryKind::Directory,
                SearchEntryKindDto::Symlink => fm_domain::SearchEntryKind::Symlink,
            })
            .collect(),
        mime_types: query.mime_types,
        min_size_bytes: query.min_size_bytes,
        max_size_bytes: query.max_size_bytes,
        modified_after: query.modified_after,
        modified_before: query.modified_before,
        content: query
            .content
            .map(|content| fm_domain::SearchContentPredicate {
                query: content.query,
                regex: content.regex,
                case_sensitive: content.case_sensitive,
                whole_word: content.whole_word,
            }),
        git_statuses: query
            .git_statuses
            .into_iter()
            .map(|status| match status {
                SearchGitStatusDto::Clean => fm_domain::GitFileStatus::Clean,
                SearchGitStatusDto::Modified => fm_domain::GitFileStatus::Modified,
                SearchGitStatusDto::Staged => fm_domain::GitFileStatus::Staged,
                SearchGitStatusDto::Untracked => fm_domain::GitFileStatus::Untracked,
                SearchGitStatusDto::Ignored => fm_domain::GitFileStatus::Ignored,
            })
            .collect(),
        tags: query.tags,
        metadata: query.metadata,
    }
}
