import type { KeyChord } from './action';

/**
 * The native OS menu bar's content (macOS `NSMenu`, task 0133). Mirrors
 * `fm_domain::menu::NativeMenuSpec` and its siblings 1:1 - field
 * names/casing/tags here are load-bearing and must match the Rust
 * `#[serde(rename_all = "camelCase")]` shape exactly, same convention as the
 * hand-written `ActionDescriptor`/`KeyChord` types in `./action.ts`.
 */
export interface NativeMenuSpec {
  readonly menus: readonly NativeMenu[];
}

/** One top-level menu (e.g. "File") and its items. */
export interface NativeMenu {
  readonly title: string;
  readonly items: readonly NativeMenuItem[];
}

/** One entry within a menu (or submenu). */
export type NativeMenuItem =
  | { readonly kind: 'separator' }
  | {
      readonly kind: 'action';
      readonly id: string;
      readonly title: string;
      readonly shortcut?: KeyChord;
      readonly enabled: boolean;
      readonly checked: boolean;
    }
  | { readonly kind: 'submenu'; readonly title: string; readonly items: readonly NativeMenuItem[] }
  | { readonly kind: 'role'; readonly role: NativeMenuRole };

/**
 * A standard OS-provided menu role with no action-registry equivalent.
 * Mirrors `fm_domain::menu::NativeMenuItem::Role`, a struct variant
 * (`{ role: NativeMenuRole }`) rather than a newtype - serde's
 * internally-tagged representation can't nest a newtype's own unit-variant
 * serialization under a field, so the struct-variant shape
 * (`{ kind: 'role', role: '<value>' }`) is what actually serializes.
 */
export type NativeMenuRole =
  | 'about'
  | 'services'
  | 'hideApp'
  | 'hideOthers'
  | 'showAll'
  | 'quit'
  | 'minimize'
  | 'zoom'
  | 'bringAllToFront';
