//! Native menu bar content (task 0133).
//!
//! Plain, serializable data types describing the OS-level menu bar
//! (macOS `NSMenu`/Windows `HMENU`) so the frontend can compute the menu tree
//! from the action registry (task 0049) and the platform layer only has to
//! render whatever tree it is handed - it never re-derives menu content
//! itself. Mirrors `action.rs`'s split: the frontend hand-writes matching TS
//! types against this module's JSON shape, so field names/casing/tags here
//! are load-bearing and pinned by an explicit `serde_json::to_string`
//! assertion in the tests below, not just a round-trip check.

use serde::{Deserialize, Serialize};

use crate::action::KeyChord;

/// The entire native menu bar, top to bottom (spec §23, task 0133).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMenuSpec {
    /// Top-level menus, e.g. File/Edit/View/Go/Window/Help.
    pub menus: Vec<NativeMenu>,
}

/// One top-level menu (e.g. "File") and its items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMenu {
    /// The menu's title. On macOS, AppKit ignores this for the very first
    /// menu in the bar (it shows the process name instead) but the field is
    /// still required structurally.
    pub title: String,
    /// The menu's items, top to bottom.
    pub items: Vec<NativeMenuItem>,
}

/// One entry within a menu (or submenu).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NativeMenuItem {
    /// A visual divider between groups of items.
    Separator,
    /// An item that dispatches to an action-registry action id (task 0049)
    /// when clicked, so menu clicks and the matching keyboard shortcut share
    /// exactly one code path rather than diverging.
    Action {
        /// The action-registry id this item dispatches, e.g. `"core.copy"`.
        id: String,
        /// User-facing label.
        title: String,
        /// The keyboard shortcut shown alongside the label, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shortcut: Option<KeyChord>,
        /// Whether the item is currently clickable, mirroring the action
        /// registry's own availability evaluation (spec §18).
        #[serde(default = "default_true")]
        enabled: bool,
        /// Whether the item shows a checkmark/on-state (e.g. a toggle).
        #[serde(default)]
        checked: bool,
    },
    /// A nested menu, e.g. "Open Recent" inside "File".
    Submenu {
        /// The submenu's title.
        title: String,
        /// The submenu's items.
        items: Vec<NativeMenuItem>,
    },
    /// A standard OS-provided item with no application callback (e.g.
    /// "Quit", "Hide Others…") - the platform adapter wires these to the
    /// matching native selector rather than routing them through
    /// `on_action`.
    ///
    /// A struct variant (`{ role: NativeMenuRole }`), not a newtype
    /// (`Role(NativeMenuRole)`): serde's internally-tagged representation
    /// folds a newtype's own unit-variant serialization into a bare
    /// `{ <variantName>: null }` field merged next to `"kind"` (verified
    /// behaviour, not an assumption) rather than nesting it under a `role`
    /// key - which the frontend hand-writes matching TS types against, so
    /// the wire shape here is load-bearing.
    Role {
        /// Which standard OS role this item performs.
        role: NativeMenuRole,
    },
}

fn default_true() -> bool {
    true
}

/// A standard OS-provided menu role with no action-registry equivalent
/// (spec §23, task 0133). Kept as a closed set (unlike
/// [`crate::action::ActionSource`]'s open plugin extension point) because
/// these map 1:1 to a fixed list of native AppKit selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeMenuRole {
    /// "About <App>" - `orderFrontStandardAboutPanel:`.
    About,
    /// The "Services" submenu, populated by the OS.
    Services,
    /// "Hide <App>" - `hide:`.
    HideApp,
    /// "Hide Others" - `hideOtherApplications:`.
    HideOthers,
    /// "Show All" - `unhideAllApplications:`.
    ShowAll,
    /// "Quit <App>" - `terminate:`.
    Quit,
    /// "Minimize" - `performMiniaturize:`.
    Minimize,
    /// "Zoom" - `performZoom:`.
    Zoom,
    /// "Bring All to Front" - `arrangeInFront:`.
    BringAllToFront,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::KeyChord;

    fn full_spec() -> NativeMenuSpec {
        NativeMenuSpec {
            menus: vec![NativeMenu {
                title: "fm".to_owned(),
                items: vec![
                    NativeMenuItem::Role {
                        role: NativeMenuRole::About,
                    },
                    NativeMenuItem::Separator,
                    NativeMenuItem::Action {
                        id: "core.preferences".to_owned(),
                        title: "Preferences\u{2026}".to_owned(),
                        shortcut: Some(KeyChord {
                            key: ",".to_owned(),
                            meta: true,
                            ..KeyChord::default()
                        }),
                        enabled: true,
                        checked: false,
                    },
                    NativeMenuItem::Submenu {
                        title: "Open Recent".to_owned(),
                        items: vec![NativeMenuItem::Action {
                            id: "core.openRecent.0".to_owned(),
                            title: "Downloads".to_owned(),
                            shortcut: None,
                            enabled: true,
                            checked: false,
                        }],
                    },
                    NativeMenuItem::Role {
                        role: NativeMenuRole::Quit,
                    },
                ],
            }],
        }
    }

    #[test]
    fn native_menu_spec_round_trips_through_serde_json() {
        let spec = full_spec();
        let json = serde_json::to_string(&spec).expect("serialization must succeed");
        let parsed: NativeMenuSpec =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(spec, parsed);
    }

    #[test]
    fn native_menu_item_variants_serialize_with_the_exact_json_shape_the_frontend_expects() {
        assert_eq!(
            serde_json::to_string(&NativeMenuItem::Separator).expect("must serialize"),
            r#"{"kind":"separator"}"#
        );

        assert_eq!(
            serde_json::to_string(&NativeMenuItem::Action {
                id: "core.copy".to_owned(),
                title: "Copy".to_owned(),
                shortcut: Some(KeyChord {
                    key: "c".to_owned(),
                    meta: true,
                    ..KeyChord::default()
                }),
                enabled: true,
                checked: false,
            })
            .expect("must serialize"),
            concat!(
                r#"{"kind":"action","id":"core.copy","title":"Copy","#,
                r#""shortcut":{"key":"c","ctrl":false,"shift":false,"alt":false,"meta":true},"#,
                r#""enabled":true,"checked":false}"#
            )
        );

        assert_eq!(
            serde_json::to_string(&NativeMenuItem::Action {
                id: "core.undo".to_owned(),
                title: "Undo".to_owned(),
                shortcut: None,
                enabled: false,
                checked: false,
            })
            .expect("must serialize"),
            r#"{"kind":"action","id":"core.undo","title":"Undo","enabled":false,"checked":false}"#
        );

        assert_eq!(
            serde_json::to_string(&NativeMenuItem::Submenu {
                title: "Go".to_owned(),
                items: vec![NativeMenuItem::Separator],
            })
            .expect("must serialize"),
            r#"{"kind":"submenu","title":"Go","items":[{"kind":"separator"}]}"#
        );

        // `Role` is a struct variant (`{ role: NativeMenuRole }`), not a
        // newtype (`Role(NativeMenuRole)`): serde's internally-tagged
        // representation folds a newtype's own unit-variant serialization
        // into a bare `{ <variantName>: null }` field merged next to
        // `"kind"` rather than nesting it, which would silently disagree
        // with the frontend's hand-written TS types. Pinned explicitly here.
        assert_eq!(
            serde_json::to_string(&NativeMenuItem::Role {
                role: NativeMenuRole::Quit
            })
            .expect("must serialize"),
            r#"{"kind":"role","role":"quit"}"#
        );
        assert_eq!(
            serde_json::to_string(&NativeMenuItem::Role {
                role: NativeMenuRole::HideApp
            })
            .expect("must serialize"),
            r#"{"kind":"role","role":"hideApp"}"#
        );
        assert_eq!(
            serde_json::to_string(&NativeMenuItem::Role {
                role: NativeMenuRole::About
            })
            .expect("must serialize"),
            r#"{"kind":"role","role":"about"}"#
        );
    }

    #[test]
    fn native_menu_spec_serializes_with_camel_case_field_names() {
        let spec = NativeMenuSpec {
            menus: vec![NativeMenu {
                title: "File".to_owned(),
                items: vec![],
            }],
        };
        assert_eq!(
            serde_json::to_string(&spec).expect("must serialize"),
            r#"{"menus":[{"title":"File","items":[]}]}"#
        );
    }

    #[test]
    fn nested_submenus_round_trip() {
        let spec = NativeMenuSpec {
            menus: vec![NativeMenu {
                title: "Window".to_owned(),
                items: vec![NativeMenuItem::Submenu {
                    title: "Outer".to_owned(),
                    items: vec![NativeMenuItem::Submenu {
                        title: "Inner".to_owned(),
                        items: vec![NativeMenuItem::Role {
                            role: NativeMenuRole::Minimize,
                        }],
                    }],
                }],
            }],
        };
        let json = serde_json::to_string(&spec).expect("serialization must succeed");
        let parsed: NativeMenuSpec =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(spec, parsed);
    }

    #[test]
    fn empty_native_menu_spec_json_deserializes_to_default() {
        let spec: NativeMenuSpec =
            serde_json::from_str(r#"{"menus":[]}"#).expect("must deserialize");
        assert_eq!(spec, NativeMenuSpec::default());
    }
}
