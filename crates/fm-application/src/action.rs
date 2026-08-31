//! Backend action registry (spec §18).
//!
//! Pure registration, lookup and availability-evaluation logic, independent
//! of any host. `FileManagerService::invoke_action` couples this registry's
//! availability check with dispatch to the operation engine for actions that
//! mutate files.

use std::collections::BTreeMap;

use fm_domain::{
    ActionContextRequirements, ActionDescriptor, ActionId, ActionInvocationContext, ActionSource,
    KeyChord,
};
use fm_platform::PlatformCapabilities;

use crate::error::ApplicationError;

/// Holds every registered action, keyed by its stable id.
///
/// A `BTreeMap` keeps [`ActionRegistry::list`] output in a stable,
/// deterministic order, which keeps OpenAPI examples and tests reproducible.
#[derive(Debug, Clone, Default)]
pub struct ActionRegistry {
    actions: BTreeMap<ActionId, ActionDescriptor>,
}

/// Registers an action under an id that is already taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("an action is already registered with id {0:?}")]
pub struct DuplicateActionId(pub ActionId);

impl ActionRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry pre-populated with every core action (spec §18),
    /// plus the selection/navigation ids reserved by task 0028.
    ///
    /// `capabilities` is the injected [`fm_platform::PlatformAdapter`]'s
    /// reported capabilities (task 0061): `core.open`, `core.openWith`,
    /// `core.revealInSystemFileManager` and `core.openTerminal` derive their
    /// `feature_available` from it, the same way
    /// [`crate::FileManagerService::runtime_capabilities`] derives its DTO
    /// (task 0058) - so browser/server mode (an empty
    /// [`FallbackPlatformAdapter`](fm_platform::FallbackPlatformAdapter))
    /// reports these actions unavailable, not merely hidden (spec §22).
    #[must_use]
    pub fn with_core_actions(capabilities: PlatformCapabilities) -> Self {
        let mut registry = Self::new();
        for descriptor in core_actions(capabilities) {
            registry
                .register(descriptor)
                .expect("core action ids must be unique");
        }
        registry
    }

    /// Registers a new action, rejecting a duplicate id.
    pub fn register(&mut self, descriptor: ActionDescriptor) -> Result<(), DuplicateActionId> {
        if self.actions.contains_key(&descriptor.id) {
            return Err(DuplicateActionId(descriptor.id));
        }
        self.actions.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    /// Looks up one action by id.
    #[must_use]
    pub fn get(&self, id: &ActionId) -> Option<&ActionDescriptor> {
        self.actions.get(id)
    }

    /// Lists every registered action in a stable order.
    #[must_use]
    pub fn list(&self) -> Vec<ActionDescriptor> {
        self.actions.values().cloned().collect()
    }

    /// Confirms `id` is registered and currently available for `context`,
    /// returning the matching descriptor. Never panics: an unknown or
    /// unavailable action is reported as a typed error (spec §18).
    pub fn require_available(
        &self,
        id: &ActionId,
        context: &ActionInvocationContext,
    ) -> Result<&ActionDescriptor, ApplicationError> {
        let descriptor = self
            .actions
            .get(id)
            .ok_or_else(|| ApplicationError::ActionNotFound(id.clone()))?;
        if !descriptor.context_requirements.is_satisfied_by(context) {
            return Err(ApplicationError::ActionUnavailable(id.clone()));
        }
        Ok(descriptor)
    }
}

fn core_action(
    id: &str,
    title: &str,
    category: &str,
    shortcuts: Vec<KeyChord>,
    context_requirements: ActionContextRequirements,
) -> ActionDescriptor {
    ActionDescriptor {
        id: ActionId::new(id),
        title: title.to_owned(),
        description: None,
        category: category.to_owned(),
        default_shortcuts: shortcuts,
        context_requirements,
        parameter_schema: None,
        source: ActionSource::Core,
    }
}

fn key(key: &str) -> KeyChord {
    KeyChord {
        key: key.to_owned(),
        ..KeyChord::default()
    }
}

fn primary(key: &str) -> KeyChord {
    KeyChord {
        key: key.to_owned(),
        ctrl: true,
        ..KeyChord::default()
    }
}

/// Requires exactly one selected entry, like [`ActionContextRequirements::single_selection`],
/// but with `feature_available` computed from the injected platform
/// adapter's capabilities (task 0061) rather than hardcoded `true`.
fn capability_gated_single_selection(feature_available: bool) -> ActionContextRequirements {
    ActionContextRequirements {
        feature_available,
        requires_selection: true,
        requires_single_selection: true,
    }
}

/// No selection requirement, like [`ActionContextRequirements::none`], but
/// with `feature_available` computed from the injected platform adapter's
/// capabilities (task 0061) rather than hardcoded `true`.
fn capability_gated_none(feature_available: bool) -> ActionContextRequirements {
    ActionContextRequirements {
        feature_available,
        requires_selection: false,
        requires_single_selection: false,
    }
}

/// At least one selected entry, like [`ActionContextRequirements::selection`],
/// but with `feature_available` computed from the injected platform
/// adapter's capabilities (task 0043) rather than hardcoded `true`.
fn capability_gated_selection(feature_available: bool) -> ActionContextRequirements {
    ActionContextRequirements {
        feature_available,
        requires_selection: true,
        requires_single_selection: false,
    }
}

/// Core actions named by spec §18, plus the selection/navigation ids
/// reserved by task 0028's frontend keybinding table.
///
/// `core.open`, `core.openWith`, `core.revealInSystemFileManager` and
/// `core.openTerminal` derive `feature_available` from `capabilities` (the
/// injected [`fm_platform::PlatformAdapter`]'s reported capabilities, task
/// 0061) rather than a permanent hardcoded value. `core.openWith` is tied to
/// the same [`PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION`] flag as
/// `core.open` (no platform adapter exposes a distinct capability bit for
/// it), but dispatches to [`fm_platform::PlatformAdapter::open_with_chooser`],
/// a genuinely distinct native "choose application" dialog on adapters that
/// implement it (macOS), falling back to opening with the default
/// application on adapters that don't (a documented gap, not silently
/// over-claimed - see that method's doc comment). `core.openWith`
/// additionally carries a `Ctrl+Enter` (`Cmd+Enter` on macOS, via
/// [`primary`]) shortcut, matching the Marta file manager's "open with"
/// convention, alongside its existing command-palette-only binding.
/// `core.view` (task 0087) originally shared the same capability and
/// dispatch as `core.open`; task 0088's in-app Lister viewer
/// ([`crate::FileManagerService`]'s platform dispatch doc comment) now
/// handles the common case on every host without needing OS integration, so
/// `core.view` itself is never permanently gated - only the OS-open
/// fallback it still dispatches to for directories, multi-selections,
/// single-pane workspaces and the forced Alt+F3 shortcut is capability-
/// dependent, and the frontend checks `core.open`'s `feature_available` for
/// that fallback path instead.
/// `core.edit` (task 0086) opens the selected file in a text editor rather
/// than its default application, gated by the same capability since no
/// platform adapter exposes a distinct "text editor" capability bit yet
/// (see [`fm_platform::PlatformAdapter::open_in_text_editor`]'s doc
/// comment).
/// `core.copyName`, `core.copyPath` and `core.copyRelativePath` are
/// frontend-owned system-clipboard actions (task 0093). They are available
/// whenever at least one entry is selected; the frontend derives their text
/// from the loaded entry locations and writes it to the host clipboard.
///
/// `core.trash` and `core.delete` split ownership of the `F8`/`Delete` keys
/// based on [`PlatformCapabilities::TRASH`] (task 0043): when trash is
/// available, it owns the bare keys (the safe, reversible default) and
/// `core.delete` moves to `Shift+F8`/`Shift+Delete`; when trash is
/// unavailable (e.g. browser/server mode), `core.delete` keeps the bare keys
/// exactly as before, since permanent delete is the only option and still
/// requires its own mandatory confirmation (task 0044).
fn core_actions(capabilities: PlatformCapabilities) -> Vec<ActionDescriptor> {
    let open_available = capabilities.contains(PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION);
    let reveal_available = capabilities.contains(PlatformCapabilities::REVEAL_IN_FILE_MANAGER);
    let open_terminal_available = capabilities.contains(PlatformCapabilities::OPEN_TERMINAL);
    let trash_available = capabilities.contains(PlatformCapabilities::TRASH);
    let finder_tags_available = capabilities.contains(PlatformCapabilities::FINDER_TAGS);
    let extended_attributes_available =
        capabilities.contains(PlatformCapabilities::EXTENDED_ATTRIBUTES);
    let uninstall_available = capabilities.contains(PlatformCapabilities::APPLICATION_UNINSTALL);
    let quick_look_available = capabilities.contains(PlatformCapabilities::QUICK_LOOK);
    let (trash_shortcuts, delete_shortcuts) = if trash_available {
        (
            vec![key("F8"), key("Delete")],
            vec![
                KeyChord {
                    key: "F8".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
                KeyChord {
                    key: "Delete".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
        )
    } else {
        (Vec::new(), vec![key("F8"), key("Delete")])
    };
    vec![
        core_action(
            "core.open",
            "Open",
            "fileOperations",
            vec![
                key("Enter"),
                KeyChord {
                    key: "F3".to_owned(),
                    alt: true,
                    ..KeyChord::default()
                },
            ],
            capability_gated_single_selection(open_available),
        ),
        core_action(
            "core.view",
            "View",
            "fileOperations",
            vec![key("F3")],
            // Never platform-gated (task 0088): the in-app Lister viewer works on every
            // host, so `feature_available` must stay true even without
            // `OPEN_WITH_DEFAULT_APPLICATION` - unlike `core.open`/`core.edit`/`core.openWith`.
            ActionContextRequirements::single_selection(),
        ),
        core_action(
            "core.calculateFolderSize",
            "Calculate Folder Size",
            "fileOperations",
            // Plain Ctrl+Space (`primary(" ")`) doesn't work: the frontend dispatcher maps any
            // `ctrl: true` chord to the platform's primary modifier, which is Cmd on macOS
            // (`hasPrimaryModifier`) - so "Ctrl+Space" is actually "Cmd+Space" there, and that's
            // Spotlight, a system-wide shortcut the OS intercepts before any app (browser or
            // Tauri) ever sees the keystroke. There is no per-chord way to request the literal
            // Control key on macOS instead of the Cmd translation - `KeyChord`/`matches()` only
            // special-cases that for Tab (see the dispatcher's comment on that), and adding a
            // second one-off "literal ctrl" concept just for this action isn't worth it. Ctrl+.
            // (Cmd+. on macOS) was picked instead by explicit user preference over the
            // alternative of adding Shift (Ctrl+Shift+Space) once Cmd+Space turned out reserved.
            vec![primary(".")],
            // Never platform-gated (task 0071): the recursive walk only needs
            // `ProviderCapabilities::LIST`, which every provider advertises. The frontend
            // additionally only invokes this when the cursor entry is a directory - there's no
            // "cursor entry kind" predicate in `ActionContextRequirements` today, so that check
            // stays client-side (mirroring how `core.view`'s "not a directory" case is also a
            // frontend-side distinction, not a backend one).
            ActionContextRequirements::single_selection(),
        ),
        core_action(
            "core.edit",
            "Edit",
            "fileOperations",
            vec![
                key("F4"),
                KeyChord {
                    key: "F4".to_owned(),
                    alt: true,
                    shift: true,
                    ..KeyChord::default()
                },
            ],
            capability_gated_single_selection(open_available),
        ),
        core_action(
            "core.parent",
            "Parent Directory",
            "navigation",
            vec![key("Backspace")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.switchPane",
            "Switch Pane",
            "navigation",
            vec![
                key("Tab"),
                KeyChord {
                    key: "Tab".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.openWith",
            "Open With…",
            "fileOperations",
            vec![primary("Enter")],
            capability_gated_single_selection(open_available),
        ),
        core_action(
            "core.quickLook",
            "Quick Look",
            "fileOperations",
            vec![
                primary("y"),
                KeyChord {
                    key: "F3".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
            capability_gated_single_selection(quick_look_available),
        ),
        core_action(
            "core.revealInSystemFileManager",
            "Reveal in File Manager",
            "fileOperations",
            Vec::new(),
            capability_gated_single_selection(reveal_available),
        ),
        core_action(
            "core.uninstallApplication",
            "Uninstall Application…",
            "fileOperations",
            Vec::new(),
            // Gated by `PlatformCapabilities::APPLICATION_UNINSTALL` (task 0148, macOS only).
            // Like `core.calculateFolderSize`'s "cursor entry must be a directory" check, "the
            // selected entry must actually be a `.app` bundle" has no backend predicate to check
            // against - it stays a frontend-side distinction (the entry's name/extension is
            // already known client-side), so this only gates on the capability plus a normal
            // single selection.
            capability_gated_single_selection(uninstall_available),
        ),
        core_action(
            "core.showProperties",
            "Properties",
            "fileOperations",
            vec![KeyChord {
                key: "Enter".to_owned(),
                alt: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.copy",
            "Copy",
            "fileOperations",
            vec![key("F5")],
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.pack",
            "Pack to Archive",
            "fileOperations",
            vec![KeyChord {
                key: "F5".to_owned(),
                alt: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.moveToArchive",
            "Move to Archive",
            "fileOperations",
            vec![KeyChord {
                key: "F5".to_owned(),
                alt: true,
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.extract",
            "Extract Archive",
            "fileOperations",
            vec![KeyChord {
                key: "F6".to_owned(),
                alt: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::single_selection(),
        ),
        core_action(
            "core.move",
            "Move",
            "fileOperations",
            vec![key("F6")],
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.rename",
            "Rename",
            "fileOperations",
            // Shift+F6 is Total Commander's rename shortcut; fm keeps F2 as the
            // primary Windows/macOS-convention binding and adds Shift+F6 as an
            // alias to the same action rather than a second action id.
            vec![
                key("F2"),
                KeyChord {
                    key: "F6".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
            ActionContextRequirements::single_selection(),
        ),
        core_action(
            "core.duplicate",
            "Duplicate",
            "fileOperations",
            vec![KeyChord {
                key: "F5".to_owned(),
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.createFile",
            "New File",
            "fileOperations",
            vec![KeyChord {
                key: "F4".to_owned(),
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.trash",
            "Trash",
            "fileOperations",
            trash_shortcuts,
            capability_gated_selection(trash_available),
        ),
        core_action(
            "core.delete",
            "Delete",
            "fileOperations",
            delete_shortcuts,
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.createDirectory",
            "New Folder",
            "fileOperations",
            vec![key("F7")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.editFinderTags",
            "Edit Tags…",
            "fileOperations",
            Vec::new(),
            capability_gated_single_selection(finder_tags_available),
        ),
        core_action(
            "core.editSpotlightComment",
            "Edit Comment…",
            "fileOperations",
            Vec::new(),
            capability_gated_single_selection(extended_attributes_available),
        ),
        core_action(
            "core.paste",
            "Paste",
            "fileOperations",
            Vec::new(),
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.refresh",
            "Refresh",
            "navigation",
            vec![primary("r")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.palette",
            "Command Palette",
            "navigation",
            vec![primary("p")],
            ActionContextRequirements::unimplemented(),
        ),
        core_action(
            "core.focusLocation",
            "Focus Location",
            "navigation",
            vec![primary("l")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.quickFilter",
            "Quick Filter",
            "navigation",
            vec![primary("f")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.findFiles",
            "Find Files",
            "navigation",
            vec![KeyChord {
                key: "F7".to_owned(),
                alt: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.newTab",
            "New Tab",
            "navigation",
            vec![primary("t")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.closeTab",
            "Close Tab",
            "navigation",
            vec![primary("w")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.nextTab",
            "Next Tab",
            "navigation",
            vec![KeyChord {
                key: "Tab".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.previousTab",
            "Previous Tab",
            "navigation",
            vec![KeyChord {
                key: "Tab".to_owned(),
                ctrl: true,
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.reopenClosedTab",
            "Reopen Closed Tab",
            "navigation",
            vec![KeyChord {
                key: "T".to_owned(),
                ctrl: true,
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.openTerminal",
            "Open Terminal Here",
            "tools",
            Vec::new(),
            capability_gated_none(open_terminal_available),
        ),
        core_action(
            "core.copyName",
            "Copy Filename",
            "clipboard",
            Vec::new(),
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.copyPath",
            "Copy Full Path",
            "clipboard",
            Vec::new(),
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.copyRelativePath",
            "Copy Relative Path",
            "clipboard",
            Vec::new(),
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.rootDirectory",
            "Go to Root Directory",
            "navigation",
            vec![KeyChord {
                key: "Backspace".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.openInNewTab",
            "Open in New Tab",
            "navigation",
            vec![KeyChord {
                key: "ArrowUp".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.openInNewTabOtherPane",
            "Open in New Tab (Other Pane)",
            "navigation",
            vec![KeyChord {
                key: "ArrowUp".to_owned(),
                ctrl: true,
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.duplicateLocationToOtherPane",
            "Duplicate Directory to Other Pane",
            "navigation",
            vec![
                KeyChord {
                    key: "ArrowLeft".to_owned(),
                    ctrl: true,
                    ..KeyChord::default()
                },
                KeyChord {
                    key: "ArrowRight".to_owned(),
                    ctrl: true,
                    ..KeyChord::default()
                },
            ],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.swapPanes",
            "Swap Pane Directories",
            "navigation",
            vec![primary("u")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.swapPaneTabs",
            "Swap Pane Tab Sets",
            "navigation",
            vec![KeyChord {
                key: "u".to_owned(),
                ctrl: true,
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.compareDirectories",
            "Compare Panes' Directories",
            "navigation",
            // Shift+F2 is Total Commander's "Compare directories" shortcut. Marking the
            // differing entries selected happens client-side once the comparison completes
            // (spec §16 milestone 5, task 0075), so this action carries no context requirements
            // of its own beyond needing two open panes, checked at invocation time.
            vec![KeyChord {
                key: "F2".to_owned(),
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.calculateChecksum",
            "Calculate Checksums for Selection",
            "tools",
            // No default shortcut: checksum calculation is a deliberate,
            // occasional action reached from the command palette (spec §18
            // `core.calculateChecksum`, task 0077). Availability additionally
            // depends on the provider advertising `CHECKSUM`, which only the
            // client knows for the current pane, so it is gated there.
            Vec::new(),
            ActionContextRequirements::selection(),
        ),
        core_action(
            "core.findDuplicates",
            "Find Duplicate Files",
            "tools",
            // Operates on the pane's current root rather than a selection, so
            // it carries no selection requirement (task 0077).
            Vec::new(),
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.closeAllTabs",
            "Close All Tabs",
            "navigation",
            vec![KeyChord {
                key: "w".to_owned(),
                ctrl: true,
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.newConnection",
            "New Connection…",
            "navigation",
            vec![primary("n")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.reactivateQuickFilter",
            "Reactivate Last Quick Filter",
            "navigation",
            vec![KeyChord {
                key: "s".to_owned(),
                ctrl: true,
                shift: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.clearQuickFilter",
            "Show All Files",
            "navigation",
            vec![KeyChord {
                key: "F10".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.sortByName",
            "Sort by Name",
            "navigation",
            vec![KeyChord {
                key: "F3".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.sortByExtension",
            "Sort by Extension",
            "navigation",
            vec![KeyChord {
                key: "F4".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.sortByDate",
            "Sort by Date",
            "navigation",
            vec![KeyChord {
                key: "F5".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.sortBySize",
            "Sort by Size",
            "navigation",
            vec![KeyChord {
                key: "F6".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.sortUnsorted",
            "Unsorted",
            "navigation",
            vec![KeyChord {
                key: "F7".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.openMultiRename",
            "Multi-Rename Tool",
            "fileOperations",
            vec![primary("m")],
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.quit",
            "Quit",
            "application",
            vec![KeyChord {
                key: "F4".to_owned(),
                alt: true,
                ..KeyChord::default()
            }],
            // Desktop-only: gated by KeybindingRuntime on the frontend (the backend registry
            // has no concept of browser vs. desktop runtime), the same way F12's terminal
            // toggle is gated in `global-keydown-handler.ts`'s `isTerminalToggleShortcut`.
            ActionContextRequirements::none(),
        ),
        core_action(
            "core.showShortcutsHelp",
            "Keyboard Shortcuts",
            "application",
            vec![key("F1")],
            ActionContextRequirements::none(),
        ),
    ]
    .into_iter()
    .chain(selection_actions())
    .collect()
}

/// Selection/navigation ids reserved by task 0028
/// (`frontend/src/features/selection/keybindings.ts`'s
/// `CORE_SELECTION_ACTION_IDS`). Selection state lives entirely in the
/// frontend reducer, so these have no backend effect to gate; the registry
/// only carries their metadata for menus and the command palette.
fn selection_actions() -> Vec<ActionDescriptor> {
    [
        ("core.moveCursorUp", "Move Cursor Up", vec![key("ArrowUp")]),
        (
            "core.moveCursorDown",
            "Move Cursor Down",
            vec![key("ArrowDown")],
        ),
        (
            "core.moveCursorPageUp",
            "Move Cursor Page Up",
            vec![key("PageUp")],
        ),
        (
            "core.moveCursorPageDown",
            "Move Cursor Page Down",
            vec![key("PageDown")],
        ),
        (
            "core.moveCursorFirst",
            "Move Cursor to First",
            vec![key("Home")],
        ),
        (
            "core.moveCursorLast",
            "Move Cursor to Last",
            vec![key("End")],
        ),
        (
            "core.extendSelectionUp",
            "Extend Selection Up",
            vec![KeyChord {
                key: "ArrowUp".to_owned(),
                shift: true,
                ..KeyChord::default()
            }],
        ),
        (
            "core.extendSelectionDown",
            "Extend Selection Down",
            vec![KeyChord {
                key: "ArrowDown".to_owned(),
                shift: true,
                ..KeyChord::default()
            }],
        ),
        ("core.toggleSelection", "Toggle Selection", vec![]),
        ("core.selectAll", "Select All", vec![primary("a")]),
        (
            "core.invertSelection",
            "Invert Selection",
            vec![
                key("*"),
                KeyChord {
                    key: "*".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
        ),
        (
            "core.selectByMask",
            "Select by Mask",
            vec![
                key("+"),
                KeyChord {
                    key: "+".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
        ),
        ("core.deselectByMask", "Deselect by Mask", vec![key("-")]),
        (
            "core.clearSelection",
            "Clear Selection",
            vec![key("Escape")],
        ),
        (
            "core.toggleSelectionAndAdvance",
            "Toggle Selection and Advance",
            vec![key("Insert"), key(" ")],
        ),
        (
            "core.restoreSelection",
            "Restore Previous Selection",
            vec![key("/")],
        ),
    ]
    .into_iter()
    .map(|(id, title, shortcuts)| {
        core_action(
            id,
            title,
            "selection",
            shortcuts,
            ActionContextRequirements::none(),
        )
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_descriptor(id: &str) -> ActionDescriptor {
        core_action(
            id,
            "Sample",
            "test",
            Vec::new(),
            ActionContextRequirements::none(),
        )
    }

    #[test]
    fn with_core_actions_registers_every_required_and_reserved_id() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let ids: Vec<String> = registry
            .list()
            .into_iter()
            .map(|action| action.id.as_str().to_owned())
            .collect();

        for expected in [
            "core.open",
            "core.view",
            "core.calculateFolderSize",
            "core.edit",
            "core.parent",
            "core.switchPane",
            "core.openWith",
            "core.quickLook",
            "core.revealInSystemFileManager",
            "core.copy",
            "core.pack",
            "core.moveToArchive",
            "core.extract",
            "core.move",
            "core.rename",
            "core.trash",
            "core.delete",
            "core.createDirectory",
            "core.editFinderTags",
            "core.editSpotlightComment",
            "core.palette",
            "core.focusLocation",
            "core.quickFilter",
            "core.findFiles",
            "core.newTab",
            "core.closeTab",
            "core.nextTab",
            "core.previousTab",
            "core.reopenClosedTab",
            "core.openTerminal",
            "core.copyName",
            "core.copyPath",
            "core.copyRelativePath",
            "core.moveCursorUp",
            "core.moveCursorDown",
            "core.moveCursorPageUp",
            "core.moveCursorPageDown",
            "core.moveCursorFirst",
            "core.moveCursorLast",
            "core.extendSelectionUp",
            "core.extendSelectionDown",
            "core.toggleSelection",
            "core.selectAll",
            "core.invertSelection",
            "core.selectByMask",
            "core.deselectByMask",
            "core.clearSelection",
            "core.toggleSelectionAndAdvance",
            "core.restoreSelection",
            "core.duplicate",
            "core.createFile",
            "core.rootDirectory",
            "core.openInNewTab",
            "core.openInNewTabOtherPane",
            "core.duplicateLocationToOtherPane",
            "core.swapPanes",
            "core.swapPaneTabs",
            "core.closeAllTabs",
            "core.newConnection",
            "core.reactivateQuickFilter",
            "core.clearQuickFilter",
            "core.sortByName",
            "core.sortByExtension",
            "core.sortByDate",
            "core.sortBySize",
            "core.sortUnsorted",
            "core.openMultiRename",
            "core.quit",
            "core.showShortcutsHelp",
        ] {
            assert!(ids.iter().any(|id| id == expected), "missing {expected}");
        }
    }

    #[test]
    fn selection_toggle_actions_have_numpad_and_non_numpad_shortcuts() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let shortcuts = |id: &str| {
            registry
                .get(&ActionId::new(id))
                .expect("selection action must be registered")
                .default_shortcuts
                .clone()
        };

        assert_eq!(
            shortcuts("core.invertSelection"),
            vec![
                key("*"),
                KeyChord {
                    key: "*".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ]
        );
        assert_eq!(
            shortcuts("core.selectByMask"),
            vec![
                key("+"),
                KeyChord {
                    key: "+".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ]
        );
        assert_eq!(shortcuts("core.deselectByMask"), vec![key("-")]);
    }

    #[test]
    fn copy_selection_actions_are_available_for_a_non_empty_selection() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let mut context = ActionInvocationContext::default();
        context.selected_entry_ids.push(fm_domain::EntryId::new());
        for id in ["core.copyName", "core.copyPath", "core.copyRelativePath"] {
            let action_id = ActionId::new(id);
            registry
                .require_available(&action_id, &context)
                .expect("copy selection action must be available");
        }
    }

    #[test]
    fn capability_gated_actions_are_unavailable_when_the_adapter_reports_no_capabilities() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let mut context = ActionInvocationContext::default();
        context.selected_entry_ids.push(fm_domain::EntryId::new());
        for id in [
            "core.open",
            "core.edit",
            "core.openWith",
            "core.quickLook",
            "core.revealInSystemFileManager",
            "core.trash",
            "core.editFinderTags",
            "core.editSpotlightComment",
        ] {
            let action_id = ActionId::new(id);
            let error = registry
                .require_available(&action_id, &context)
                .expect_err("a capability-gated action must be unavailable without the capability");
            assert_eq!(error, ApplicationError::ActionUnavailable(action_id));
        }
        let action_id = ActionId::new("core.openTerminal");
        let error = registry
            .require_available(&action_id, &ActionInvocationContext::default())
            .expect_err("openTerminal must be unavailable without OPEN_TERMINAL");
        assert_eq!(error, ApplicationError::ActionUnavailable(action_id));
    }

    #[test]
    fn view_stays_available_without_any_platform_capability() {
        // Unlike core.open/core.edit/core.openWith, core.view (task 0088) is backed by an
        // in-app viewer that needs no OS integration, so it must never report unavailable.
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let mut context = ActionInvocationContext::default();
        context.selected_entry_ids.push(fm_domain::EntryId::new());
        registry
            .require_available(&ActionId::new("core.view"), &context)
            .expect("core.view must stay available with no platform capabilities");
    }

    #[test]
    fn capability_gated_actions_are_available_when_the_adapter_reports_the_matching_capability() {
        let registry = ActionRegistry::with_core_actions(
            PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION
                | PlatformCapabilities::REVEAL_IN_FILE_MANAGER
                | PlatformCapabilities::OPEN_TERMINAL
                | PlatformCapabilities::TRASH,
        );
        let mut single_selection = ActionInvocationContext::default();
        single_selection
            .selected_entry_ids
            .push(fm_domain::EntryId::new());
        for id in [
            "core.open",
            "core.view",
            "core.edit",
            "core.openWith",
            "core.revealInSystemFileManager",
            "core.trash",
        ] {
            let action_id = ActionId::new(id);
            registry
                .require_available(&action_id, &single_selection)
                .expect("the capability is granted and exactly one entry is selected");
        }
        registry
            .require_available(
                &ActionId::new("core.openTerminal"),
                &ActionInvocationContext::default(),
            )
            .expect("openTerminal has no selection requirement");
    }

    #[test]
    fn edit_finder_tags_and_edit_spotlight_comment_require_their_own_capability_and_a_single_selection()
     {
        let registry = ActionRegistry::with_core_actions(
            PlatformCapabilities::FINDER_TAGS | PlatformCapabilities::EXTENDED_ATTRIBUTES,
        );
        let mut single_selection = ActionInvocationContext::default();
        single_selection
            .selected_entry_ids
            .push(fm_domain::EntryId::new());

        for id in ["core.editFinderTags", "core.editSpotlightComment"] {
            let action_id = ActionId::new(id);
            registry
                .require_available(&action_id, &single_selection)
                .expect("granted capability and exactly one selected entry");
            registry
                .require_available(&action_id, &ActionInvocationContext::default())
                .expect_err("must require a selection");

            let mut two_selected = ActionInvocationContext::default();
            two_selected
                .selected_entry_ids
                .push(fm_domain::EntryId::new());
            two_selected
                .selected_entry_ids
                .push(fm_domain::EntryId::new());
            registry
                .require_available(&action_id, &two_selected)
                .expect_err("must require exactly one selected entry, not a multi-selection");
        }

        // FINDER_TAGS granted alone must not also unlock the EXTENDED_ATTRIBUTES-gated
        // comment action, and vice versa - the two capabilities are independent.
        let finder_tags_only = ActionRegistry::with_core_actions(PlatformCapabilities::FINDER_TAGS);
        finder_tags_only
            .require_available(&ActionId::new("core.editFinderTags"), &single_selection)
            .expect("FINDER_TAGS alone unlocks core.editFinderTags");
        let error = finder_tags_only
            .require_available(
                &ActionId::new("core.editSpotlightComment"),
                &single_selection,
            )
            .expect_err("FINDER_TAGS alone must not unlock core.editSpotlightComment");
        assert_eq!(
            error,
            ApplicationError::ActionUnavailable(ActionId::new("core.editSpotlightComment"))
        );
    }

    #[test]
    fn application_uninstall_capability_alone_unlocks_only_uninstall_application() {
        let registry =
            ActionRegistry::with_core_actions(PlatformCapabilities::APPLICATION_UNINSTALL);
        let mut single_selection = ActionInvocationContext::default();
        single_selection
            .selected_entry_ids
            .push(fm_domain::EntryId::new());

        registry
            .require_available(
                &ActionId::new("core.uninstallApplication"),
                &single_selection,
            )
            .expect("APPLICATION_UNINSTALL alone unlocks core.uninstallApplication");

        let error = registry
            .require_available(&ActionId::new("core.trash"), &single_selection)
            .expect_err("APPLICATION_UNINSTALL alone must not unlock core.trash");
        assert_eq!(
            error,
            ApplicationError::ActionUnavailable(ActionId::new("core.trash"))
        );
    }

    #[test]
    fn capability_gating_is_independent_per_action() {
        // Only OPEN_TERMINAL is granted: core.open/openWith/reveal must stay
        // unavailable even though *some* platform capability is present, so
        // gating isn't accidentally coarse-grained to "any capability at all".
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::OPEN_TERMINAL);
        let mut single_selection = ActionInvocationContext::default();
        single_selection
            .selected_entry_ids
            .push(fm_domain::EntryId::new());
        for id in [
            "core.open",
            "core.edit",
            "core.openWith",
            "core.revealInSystemFileManager",
            "core.trash",
            "core.uninstallApplication",
        ] {
            let action_id = ActionId::new(id);
            let error = registry
                .require_available(&action_id, &single_selection)
                .expect_err("must not be granted by an unrelated capability");
            assert_eq!(error, ApplicationError::ActionUnavailable(action_id));
        }
        registry
            .require_available(
                &ActionId::new("core.openTerminal"),
                &ActionInvocationContext::default(),
            )
            .expect("OPEN_TERMINAL was granted");
    }

    #[test]
    fn trash_owns_f8_and_delete_when_available_and_delete_moves_to_shift() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::TRASH);
        let trash = registry
            .get(&ActionId::new("core.trash"))
            .expect("core.trash must be registered");
        assert_eq!(
            trash.default_shortcuts,
            vec![key("F8"), key("Delete")],
            "trash must own the bare keys once it is available"
        );
        let delete = registry
            .get(&ActionId::new("core.delete"))
            .expect("core.delete must be registered");
        assert_eq!(
            delete.default_shortcuts,
            vec![
                KeyChord {
                    key: "F8".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
                KeyChord {
                    key: "Delete".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
            "delete must move to the shift variants once trash owns the bare keys"
        );
    }

    #[test]
    fn delete_keeps_f8_and_delete_when_trash_is_unavailable() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let trash = registry
            .get(&ActionId::new("core.trash"))
            .expect("core.trash must still be registered, just unavailable");
        assert!(
            trash.default_shortcuts.is_empty(),
            "an unavailable trash action must not claim any shortcut"
        );
        let delete = registry
            .get(&ActionId::new("core.delete"))
            .expect("core.delete must be registered");
        assert_eq!(
            delete.default_shortcuts,
            vec![key("F8"), key("Delete")],
            "delete must keep the bare keys unchanged when trash is unavailable"
        );
    }

    #[test]
    fn view_and_edit_default_to_f3_and_f4_between_open_and_openwith() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let view = registry
            .get(&ActionId::new("core.view"))
            .expect("core.view must be registered");
        assert_eq!(
            view.default_shortcuts,
            vec![key("F3")],
            "core.view must default to F3 (Total Commander convention)"
        );
        let edit = registry
            .get(&ActionId::new("core.edit"))
            .expect("core.edit must be registered");
        assert_eq!(
            edit.default_shortcuts,
            vec![
                key("F4"),
                KeyChord {
                    key: "F4".to_owned(),
                    alt: true,
                    shift: true,
                    ..KeyChord::default()
                },
            ],
            "core.edit must default to F4 (Total Commander convention)"
        );
    }

    #[test]
    fn open_with_defaults_to_ctrl_or_cmd_enter_alongside_open() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let open = registry
            .get(&ActionId::new("core.open"))
            .expect("core.open must be registered");
        assert_eq!(
            open.default_shortcuts,
            vec![
                key("Enter"),
                KeyChord {
                    key: "F3".to_owned(),
                    alt: true,
                    ..KeyChord::default()
                },
            ],
            "core.open must keep its bare Enter shortcut"
        );
        let open_with = registry
            .get(&ActionId::new("core.openWith"))
            .expect("core.openWith must be registered");
        assert_eq!(
            open_with.default_shortcuts,
            vec![primary("Enter")],
            "core.openWith must default to the Marta-style Ctrl+Enter (Cmd+Enter on macOS) shortcut"
        );
    }

    #[test]
    fn quick_look_is_capability_gated_and_uses_finder_and_f3_shortcuts() {
        let unavailable = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let action_id = ActionId::new("core.quickLook");
        let action = unavailable
            .get(&action_id)
            .expect("core.quickLook must be registered");
        assert_eq!(
            action.default_shortcuts,
            vec![
                KeyChord {
                    key: "y".to_owned(),
                    ctrl: true,
                    ..KeyChord::default()
                },
                KeyChord {
                    key: "F3".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ]
        );

        let mut context = ActionInvocationContext::default();
        context.selected_entry_ids.push(fm_domain::EntryId::new());
        assert_eq!(
            unavailable
                .require_available(&action_id, &context)
                .expect_err("Quick Look must be unavailable without its platform capability"),
            ApplicationError::ActionUnavailable(action_id.clone())
        );

        ActionRegistry::with_core_actions(PlatformCapabilities::QUICK_LOOK)
            .require_available(&action_id, &context)
            .expect("Quick Look must be available for one selection with its capability");
    }

    #[test]
    fn find_files_has_no_selection_requirement_and_uses_alt_f7() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let action_id = ActionId::new("core.findFiles");
        let find_files = registry
            .get(&action_id)
            .expect("core.findFiles must be registered");
        assert_eq!(
            find_files.default_shortcuts,
            vec![KeyChord {
                key: "F7".to_owned(),
                alt: true,
                ..KeyChord::default()
            }],
            "core.findFiles must default to the Total Commander Alt+F7 shortcut"
        );
        registry
            .require_available(&action_id, &ActionInvocationContext::default())
            .expect("core.findFiles must not require a selection");
    }

    #[test]
    fn rename_defaults_to_f2_and_shift_f6_alias() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let rename = registry
            .get(&ActionId::new("core.rename"))
            .expect("core.rename must be registered");
        assert_eq!(
            rename.default_shortcuts,
            vec![
                key("F2"),
                KeyChord {
                    key: "F6".to_owned(),
                    shift: true,
                    ..KeyChord::default()
                },
            ],
            "core.rename must keep F2 as primary and gain Shift+F6 as a Total Commander alias"
        );
    }

    #[test]
    fn duplicate_requires_selection_and_uses_shift_f5() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let action_id = ActionId::new("core.duplicate");
        let duplicate = registry
            .get(&action_id)
            .expect("core.duplicate must be registered");
        assert_eq!(
            duplicate.default_shortcuts,
            vec![KeyChord {
                key: "F5".to_owned(),
                shift: true,
                ..KeyChord::default()
            }]
        );
        registry
            .require_available(&action_id, &ActionInvocationContext::default())
            .expect_err("duplicate must require a selection");
    }

    #[test]
    fn create_file_has_no_selection_requirement_and_uses_shift_f4() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let action_id = ActionId::new("core.createFile");
        let create_file = registry
            .get(&action_id)
            .expect("core.createFile must be registered");
        assert_eq!(
            create_file.default_shortcuts,
            vec![KeyChord {
                key: "F4".to_owned(),
                shift: true,
                ..KeyChord::default()
            }]
        );
        registry
            .require_available(&action_id, &ActionInvocationContext::default())
            .expect("core.createFile must not require a selection");
    }

    #[test]
    fn root_directory_uses_ctrl_backspace_with_no_selection_requirement() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let action_id = ActionId::new("core.rootDirectory");
        let root = registry
            .get(&action_id)
            .expect("core.rootDirectory must be registered");
        assert_eq!(
            root.default_shortcuts,
            vec![KeyChord {
                key: "Backspace".to_owned(),
                ctrl: true,
                ..KeyChord::default()
            }]
        );
        registry
            .require_available(&action_id, &ActionInvocationContext::default())
            .expect("core.rootDirectory must not require a selection");
    }

    #[test]
    fn duplicate_location_to_other_pane_binds_both_ctrl_arrow_directions() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let action = registry
            .get(&ActionId::new("core.duplicateLocationToOtherPane"))
            .expect("core.duplicateLocationToOtherPane must be registered");
        assert_eq!(
            action.default_shortcuts,
            vec![
                KeyChord {
                    key: "ArrowLeft".to_owned(),
                    ctrl: true,
                    ..KeyChord::default()
                },
                KeyChord {
                    key: "ArrowRight".to_owned(),
                    ctrl: true,
                    ..KeyChord::default()
                },
            ]
        );
    }

    #[test]
    fn sort_shortcuts_use_ctrl_f3_through_ctrl_f7() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let expected = [
            ("core.sortByName", "F3"),
            ("core.sortByExtension", "F4"),
            ("core.sortByDate", "F5"),
            ("core.sortBySize", "F6"),
            ("core.sortUnsorted", "F7"),
        ];
        for (id, function_key) in expected {
            let action = registry
                .get(&ActionId::new(id))
                .unwrap_or_else(|| panic!("{id} must be registered"));
            assert_eq!(
                action.default_shortcuts,
                vec![KeyChord {
                    key: function_key.to_owned(),
                    ctrl: true,
                    ..KeyChord::default()
                }],
                "{id} must bind Ctrl+{function_key}"
            );
        }
    }

    #[test]
    fn selection_only_actions_include_insert_and_numpad_slash() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        // Space is bound here rather than to `core.toggleSelection` (Total Commander parity: both
        // Insert and Space toggle the cursor row's selection *and* advance to the next row).
        assert_eq!(
            registry
                .get(&ActionId::new("core.toggleSelectionAndAdvance"))
                .expect("core.toggleSelectionAndAdvance must be registered")
                .default_shortcuts,
            vec![key("Insert"), key(" ")]
        );
        assert_eq!(
            registry
                .get(&ActionId::new("core.toggleSelection"))
                .expect("core.toggleSelection must be registered")
                .default_shortcuts,
            Vec::<KeyChord>::new(),
        );
        assert_eq!(
            registry
                .get(&ActionId::new("core.restoreSelection"))
                .expect("core.restoreSelection must be registered")
                .default_shortcuts,
            vec![key("/")]
        );
    }

    #[test]
    fn quit_uses_alt_f4_and_help_uses_f1() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        assert_eq!(
            registry
                .get(&ActionId::new("core.quit"))
                .expect("core.quit must be registered")
                .default_shortcuts,
            vec![KeyChord {
                key: "F4".to_owned(),
                alt: true,
                ..KeyChord::default()
            }]
        );
        assert_eq!(
            registry
                .get(&ActionId::new("core.showShortcutsHelp"))
                .expect("core.showShortcutsHelp must be registered")
                .default_shortcuts,
            vec![key("F1")]
        );
    }

    #[test]
    fn compare_directories_uses_shift_f2_and_needs_no_selection() {
        let registry = ActionRegistry::with_core_actions(PlatformCapabilities::empty());
        let action = registry
            .get(&ActionId::new("core.compareDirectories"))
            .expect("core.compareDirectories must be registered");
        assert_eq!(
            action.default_shortcuts,
            vec![KeyChord {
                key: "F2".to_owned(),
                shift: true,
                ..KeyChord::default()
            }],
            "Shift+F2 must stay free for Total Commander's Compare Directories shortcut"
        );
        assert_eq!(
            action.context_requirements,
            ActionContextRequirements::none()
        );
    }

    #[test]
    fn register_rejects_a_duplicate_id() {
        let mut registry = ActionRegistry::new();
        registry
            .register(sample_descriptor("test.sample"))
            .expect("first registration must succeed");

        let error = registry
            .register(sample_descriptor("test.sample"))
            .expect_err("duplicate id must be rejected");
        assert_eq!(error, DuplicateActionId(ActionId::new("test.sample")));
    }

    #[test]
    fn require_available_reports_unknown_actions_without_panicking() {
        let registry = ActionRegistry::new();
        let context = ActionInvocationContext::default();
        let error = registry
            .require_available(&ActionId::new("does.not.exist"), &context)
            .expect_err("an unregistered action must be reported, not panic");
        assert_eq!(
            error,
            ApplicationError::ActionNotFound(ActionId::new("does.not.exist"))
        );
    }

    #[test]
    fn require_available_re_validates_context_requirements() {
        let mut registry = ActionRegistry::new();
        registry
            .register(core_action(
                "test.needsSelection",
                "Needs Selection",
                "test",
                Vec::new(),
                ActionContextRequirements::selection(),
            ))
            .expect("registration must succeed");
        let action_id = ActionId::new("test.needsSelection");

        let empty_context = ActionInvocationContext::default();
        assert_eq!(
            registry
                .require_available(&action_id, &empty_context)
                .expect_err("no selection must be rejected"),
            ApplicationError::ActionUnavailable(action_id.clone())
        );

        let mut selected_context = ActionInvocationContext::default();
        selected_context
            .selected_entry_ids
            .push(fm_domain::EntryId::new());
        assert!(
            registry
                .require_available(&action_id, &selected_context)
                .is_ok()
        );
    }
}
