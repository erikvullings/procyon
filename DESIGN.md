---
name: Procyon
description: A calm, dense, keyboard-first interface for exact file operations.
colors:
  day-canvas: "#f4f6f8"
  day-surface: "#ffffff"
  day-text: "#344054"
  day-muted: "#667085"
  day-border: "#c8ced8"
  day-accent: "#075ea8"
  day-selection: "#c9e2ff"
  day-hover: "#e9eef4"
  night-canvas: "#11151a"
  night-surface: "#1b2129"
  night-elevated: "#252d38"
  night-text: "#d3dae3"
  night-muted: "#98a3b0"
  night-border: "#485463"
  night-accent: "#79b8ff"
  night-selection: "#244f78"
  night-hover: "#29333f"
  cursor-blue: "#285fa8"
  cursor-text: "#ffffff"
  danger: "#b42318"
  warning: "#8a4b08"
  success: "#18723c"
typography:
  title:
    fontFamily: "Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    lineHeight: 1.2
  body:
    fontFamily: "Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.35
  label:
    fontFamily: "Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "max(0.92rem, 12px)"
    fontWeight: 400
    lineHeight: 1.15
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, Liberation Mono, monospace"
    fontSize: "max(0.92rem, 12px)"
    fontWeight: 400
    lineHeight: 1.35
rounded:
  control: "2px"
  surface: "4px"
  pill: "999px"
spacing:
  hairline: "1px"
  compact: "2px"
  control: "4px"
  cluster: "8px"
  section: "12px"
  panel: "16px"
components:
  button-primary:
    backgroundColor: "{colors.day-accent}"
    textColor: "{colors.cursor-text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 10px"
    height: "24px"
  button-flat:
    backgroundColor: "transparent"
    textColor: "{colors.day-text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 10px"
    height: "24px"
  button-chrome:
    backgroundColor: "transparent"
    textColor: "{colors.day-text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "2px 4px"
    height: "20px"
  input-form:
    backgroundColor: "{colors.day-surface}"
    textColor: "{colors.day-text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 8px"
    height: "24px"
  directory-row:
    backgroundColor: "{colors.night-surface}"
    textColor: "{colors.night-text}"
    typography: "{typography.body}"
    rounded: "0"
    padding: "0 4px"
    height: "20px"
  directory-row-cursor:
    backgroundColor: "{colors.cursor-blue}"
    textColor: "{colors.cursor-text}"
    typography: "{typography.body}"
    rounded: "0"
    padding: "0 4px"
    height: "20px"
---

# Design System: Procyon

## Overview

**Creative North Star: "The Quiet Operations Deck"**

Procyon is an operations surface, not a presentation surface. It should feel like a calm command deck built for sustained, high-volume file work: dense enough to keep two directory contexts visible, exact enough to support keyboard habits, and quiet enough that selection, conflict, and progress states remain immediately legible. The visual system favors information capacity and stable geometry over decorative whitespace.

The interface uses flat tonal layers, fine separators, compact controls, and a restrained blue signal color. Brand character comes from disciplined alignment, decisive cursor states, dual-pane symmetry, and the function-key rhythm rather than ornament. It should not drift into a spacious consumer dashboard, a glowing sci-fi terminal, or a generic rounded-card application.

**Key Characteristics:**
- Dense 20px operational rows and 24px structural headers.
- Calm blue-gray neutrals in both light and dark themes.
- One strong cursor blue for immediate location and action context.
- Borders and tonal shifts define structure; shadows belong to overlays.
- Keyboard-first behavior with visible, compact command affordances.

## Colors

The palette is a low-chroma blue-gray field with precise blue interaction signals and semantic colors reserved for meaning.

### Primary
- **Instrument Blue** (#075ea8 light / #79b8ff dark): Interactive actions, links, active controls, and accessible emphasis. The dark value is deliberately brighter to retain contrast on the night surfaces.
- **Cursor Blue** (#285fa8): The authoritative current-row state in both themes. It uses white text and should remain visually stronger than ordinary selection.

### Neutral
- **Day Canvas** (#f4f6f8): Light-theme application background and inactive tab field.
- **Day Surface** (#ffffff): Light-theme panes, controls, and elevated surfaces.
- **Day Ink** (#344054): Primary light-theme text.
- **Day Muted Ink** (#667085): Metadata, labels, and secondary text.
- **Day Divider** (#c8ced8): Light-theme borders, split lines, and table rules.
- **Night Canvas** (#11151a): Dark-theme application background.
- **Night Surface** (#1b2129): Dark-theme pane and control field.
- **Night Raised Surface** (#252d38): Dialogs, menus, and temporarily raised dark surfaces.
- **Night Ink** (#d3dae3): Primary dark-theme text.
- **Night Muted Ink** (#98a3b0): Secondary dark-theme text.
- **Night Divider** (#485463): Dark-theme borders, split lines, and table rules.

### State
- **Selection Wash** (#c9e2ff light / #244f78 dark): Multi-selection and selected menu items, subordinate to the cursor row.
- **Hover Wash** (#e9eef4 light / #29333f dark): Quiet hover feedback without apparent elevation.
- **Danger** (#b42318 light / #ff8a80 dark): Destructive actions and errors only.
- **Warning** (#8a4b08 light / #ffc46b dark): Caution and attention states.
- **Success** (#18723c light / #72d69a dark): Confirmed completion and healthy states.

### Named Rules

**The Signal Rarity Rule.** Blue communicates location, focus, selection, or action. Do not use it as decorative fill across large passive regions.

**The Cursor Supremacy Rule.** The cursor row must be the strongest state in a file list; multi-selection, hover, zebra striping, and search highlighting must not compete with it.

## Typography

**Display Font:** Inter (with system sans-serif fallbacks)
**Body Font:** Inter (with system sans-serif fallbacks)
**Label/Mono Font:** ui-monospace, SFMono-Regular, Menlo, Consolas, Liberation Mono, monospace

**Character:** The primary type is compact, neutral, and highly legible at small sizes. Monospace is functional rather than decorative: use it for terminals, code, checksums, and aligned technical values, not for general application chrome.

### Hierarchy
- **Headline** (600, `var(--fm-type-heading)` / 1.05rem, 1.2): Dialog titles and the highest local heading. Procyon has no oversized application display tier.
- **Title** (600, `var(--fm-type-title)` / 1rem, 1.2): Active tabs, pane titles, table headings, and compact section titles.
- **Body** (400, `var(--fm-type-body)` / 1rem, 1.35): Default controls, file metadata, settings copy, and operational messages.
- **Label** (400, `var(--fm-type-label)` / max(0.92rem, 12px), 1.15): Static field labels and secondary control text, using muted ink. The 12px floor keeps functional text legible under the compact root font setting.
- **Mono** (400, `var(--fm-type-label)` in chrome / `var(--fm-type-mono)` inline, 1.35): Terminal content, source text, hashes, and technical identifiers.

### Named Rules

**The No Hero Type Rule.** Application typography should never consume working area for visual drama. Hierarchy comes from weight, alignment, and tonal contrast before size.

**The Four Steps Rule.** Chrome text is exactly one of `--fm-type-heading`, `--fm-type-title`, `--fm-type-body`, or `--fm-type-label`. A raw `font-size` in `rem` or `px` in a feature stylesheet is a defect (`theme.test.ts` fails on sub-1rem literals). Relative `em` sizes are reserved for rendered document content (Markdown, previews), never for controls.

**The 12px Floor Rule.** Nothing a user must read renders below 12px. `--fm-type-label` carries the floor, so a smaller root font setting cannot push labels, help text, or field values under it.

**The Own Your Type Rule.** Overlays (dialogs, settings, drawers) set `--fm-type-body` on their root instead of inheriting from wherever they are mounted; the same dialog must not change size because it was opened from the toolbar.

## Layout

The desktop workspace is a full-viewport operational grid anchored by two equal panes. Pane boundaries, tab strips, breadcrumbs, table headers, status bars, optional drawers, and the function-key bar form a continuous stack with no ornamental gaps. Splitters may have a generous invisible hit area, but their visible rule remains hairline-thin.

The base density unit is the 20px file row. Structural headers are 24px, and internal padding uses the spacing scale only: 2px, 4px, 8px, 12px, 16px (`--fm-space-1` … `--fm-space-5`). Dialog content scrolls within its working region while titles and action footers remain available.

There are exactly two control densities, and a surface uses one of them throughout:

| Density | Token | Height | Where |
| --- | --- | --- | --- |
| **Chrome** | `--fm-control-height-dense` (= `--fm-row-height`) | 20px | Panes, tab strips, breadcrumbs, toolbars, filter bars, the function-key bar, inline rename |
| **Form** | `--fm-control-height` (= `--fm-header-height`) | 24px | Dialogs, settings, drawers, the find-files form, any decision row |

Within one density every control in a row (text field, select, segmented choice, button) has the same height, the same body type, and the same 2px radius, so they align without per-feature tuning. Marta's calm comes from this sameness, not from extra whitespace.

At narrow widths, paired form controls and help-dialog controls stack rather than compressing below usable width. Existing responsive thresholds include 640px/40rem for compact forms and 44rem for broader layout changes. Preserve feature access across widths; do not create a separate simplified product.

## Elevation & Depth

Procyon is flat by default. Adjacent regions are distinguished through canvas, surface, and raised-surface tones plus 1px borders. Resting buttons, panes, tabs, tables, and cards do not cast shadows. The single standard shadow, `0 8px 24px rgb(23 32 51 / 16%)` in light mode and `0 8px 24px rgb(0 0 0 / 45%)` in dark mode, is reserved for dialogs, dropdowns, command palettes, and other temporary overlays.

### Shadow Vocabulary
- **Light Overlay** (`0 8px 24px rgb(23 32 51 / 16%)`): Separates a temporary decision or navigation layer from the light workspace.
- **Dark Overlay** (`0 8px 24px rgb(0 0 0 / 45%)`): Separates a temporary decision or navigation layer from the dark workspace.

### Named Rules

**The Flat-at-Rest Rule.** Permanent workspace structure uses tone and borders. A shadow means that a temporary layer is above the working plane.

## Shapes

The form language is compact and rectilinear. Standard surfaces use a 4px radius, dense controls use 2px, and table rows, tab strips, pane boundaries, and splitters remain square. Pills and circles are reserved for genuinely circular indicators, badges, tags, and icon geometry; they are not a default container treatment.

Borders are normally 1px and use the theme divider color. Clipping should preserve dense alignment, especially in tab labels and directory cells. Controls should not grow rounded corners or padding merely to appear more modern.

## Components

### Buttons

Compact and explicit, with no ornamental lift. Every `<button>` in the shell is flat by default; the host OS's bevelled button must never appear.

- **Shape:** 2px radius and a 1px border (transparent when flat). Chrome buttons are 20px high with 2px 4px padding; form buttons are 24px high with 0 10px padding and a 4.5rem minimum width so "OK" and "Cancel" are equally easy to hit.
- **One primary per decision.** The confirming action is last (rightmost), outlined in Instrument Blue at rest and filled once it holds focus. Enter triggers it; Escape cancels. Destructive confirmations use the danger color in the same outline/fill pattern.
- **Secondary actions** are flat with normal text. A third, non-deciding action (e.g. "Remove all", "Save as preset…") sits at the leading edge, separated from the decision pair.
- **Decision rows** use the modal footer or, outside a modal, `.fm-form-actions` with `.fm-primary-action`. Do not hand-style a new action row.
- **Decision actions come in exactly two formats.** *Text* in dialog and settings footers (Cancel, then the confirming verb last; the verb names the outcome: Save, Delete, Overwrite). *Icons* for an edit made inline in a row (rename a workspace, name a checksum file): a check that submits and an x that cancels, 20px chrome density, each with an accessible name and tooltip, Enter and Escape always equivalent. Build inline pairs with `inlineEditActions`; never mix text and icon decisions in one row, and never put icon-only decisions in a footer.
- **Close buttons** are one size everywhere a panel, dialog, or disclosure can be dismissed: a 1.25rem (20px) hit box, 0.15rem padding, a 0.75rem x icon, muted at rest, hover wash, top trailing corner. A panel never sizes its own close button.
- **Hover / Focus:** Tonal hover only. No shadow or transform.
- **Disabled:** Muted text, no border emphasis. A busy action keeps normal contrast and shows progress instead of looking unavailable.
- **Motion:** Keep icon-button click waves; limit hover/focus surface changes to 120ms (`--fm-duration`). Never animate position, scale, text color, opacity, or enabled/disabled state.
- **Icon-only buttons** always carry an accessible name and a tooltip of the form `Label (Shortcut)`.

### Inputs / Fields

Inputs are compact instruments, not floating decorative fields. There is one text field; features do not restyle their own.

- **Style:** Surface background, 1px divider border, 2px radius, body type, 8px inline padding, at the density's height (24px in forms, 20px in chrome). The canonical rule lives in `mithril-materialized-procyon.css`; a plain `<input>` inside a dialog or settings gets it automatically.
- **Composite fields** (an input plus inline toggles or buttons) draw the border on the wrapper at the same height and mark the inner input `.browser-default` so it stays borderless.
- **Labels:** Static, above the field, muted, label type, close to the control. Never a floating or animated label.
- **Help and errors:** Label type directly under the field; errors use the danger color plus text that names the problem.
- **Focus:** The border darkens to the muted text colour; the caret is Instrument Blue. No accent border, glow, underline, thicker ring, or layout shift. A field inside a row must be able to shrink (`min-width: 0`) so its focus edge never collides with neighbouring actions.
- **Selects, switches, segmented choices:** Same height and border as a text field. Checked/active state uses Instrument Blue or the selection wash, never the component library's default teal.
- **Browser surfaces:** Caret, native checkbox/radio/range accent, and text selection are themed from the palette (`caret-color`, `accent-color`, `::selection`).
- **Error / Disabled:** Use semantic color or muted opacity in addition to accessible text; never rely on color alone.

### Navigation

Tabs, breadcrumbs, toolbars, and the function bar form one continuous control frame.

- **Tabs:** Compact 24px geometry, square joins, muted inactive state, and stronger active text/surface contrast.
- **Breadcrumbs:** Single-line, low-chrome path navigation with truncation rather than wrapping.
- **Function Bar:** Full-width bottom command strip with evenly distributed actions and visible function-key labels. It remains visually available while the workspace scrolls internally.

### Menus and Command Palette

Menus (context menu, toolbar overflow, native-style menu bar, select dropdowns) use the raised surface, fine border, the single overlay shadow, a 4px radius, and 24px rows with a 2px item radius. Hover and keyboard focus use the hover/selection washes.

A menu row is `icon · label · shortcut`: the shortcut is right-aligned in muted label type and is never appended to the label text. Key caps (`<kbd>`) — bordered inactive-selection fill — are for the command palette, the shortcut help table, and walkthroughs, where the key itself is the subject.

### Dialogs

Dialogs are temporary operational layers with fixed title and action regions where content can overflow. Use the 4px surface radius, raised surface, divider borders, and overlay shadow. Escape closes dismissible dialogs, and initial focus moves into the dialog immediately.

### Directory Rows

The directory row is the signature component of the system.

- **Geometry:** Exactly 20px high with narrow horizontal cell padding and a fixed 24px table header.
- **Default:** Flat surface with subtle alternating or tonal row differentiation only when needed.
- **Hover:** Quiet hover wash.
- **Selection:** Theme selection wash.
- **Cursor:** Cursor Blue with white text, remaining authoritative when the row is also selected.
- **Content:** Icons, names, extensions, sizes, and timestamps align predictably; truncation never changes row height.

### Tags and Status Indicators

Use compact bordered chips or small indicators. Finder tag colors retain their recognizable swatches across themes. Semantic status colors must be paired with a label, tooltip, or shape so meaning survives low vision and color-vision differences.

### Skeletons and Progress

Skeletons preserve the exact geometry of the content they replace. Progress indicators use the accent sparingly and must not cause pane or row reflow. Respect `prefers-reduced-motion`; operational clarity is more important than continuous animation.

## Keyboard and Shortcuts

The keyboard model is Total Commander's, with the host platform's conventions layered on top, so a Total Commander or Marta user finds their habits intact.

- **Function keys are the spine.** F1 Help, F2 Rename, F3 View, F4 Edit, F5 Copy, F6 Move, F7 New folder, F8 Delete, always visible in the function-key bar. Holding a modifier relabels the bar with that layer (Shift+F4 New file, Alt+F5 Pack, Alt+F7 Find files). Anything the bar advertises must work when pressed and when clicked.
- **Commander navigation.** Tab switches pane, Backspace goes up, Enter opens, Insert/Space mark, typing starts quick search.
- **Platform conventions for everything else.** The primary modifier is Command on macOS and Control elsewhere: Cmd/Ctrl+P command palette (Marta's entry point), +T/+W tabs, +L location, +F quick filter, +R refresh. Where Total Commander has a second well-known chord (Shift+F6 rename, Ctrl+PageUp parent) it is kept as an alias rather than replacing the platform one.
- **One display form.** Shortcuts are rendered only through `features/commands/shortcut-label.ts`: `Cmd`/`Option` on macOS, `Ctrl`/`Alt` elsewhere, `Shift`, then the key — single characters upper-case, named keys capitalised (`Enter`, `Backspace`, `Up`, `Space`, `Esc`), joined with `+`. Never show a stored binding string (`CTRL+ENTER`) or bake a shortcut into a translation.
- **Where shortcuts appear.** Tooltips: `Label (Shortcut)`. Menus: right-aligned. Palette and help: key caps. Settings: the same display form, editable.
- **F3 on a folder** calculates its size in place (the same as Alt+Space / Ctrl+.), identically in the browser and the desktop app. It never hands the folder to the OS file manager; Enter and Alt+F3 do that.
- **Dialogs.** Initial focus lands on the first field (or the safe action for destructive confirmations), Enter confirms, Escape cancels, Tab order follows reading order.

## Progressive Disclosure

A first-time user sees two panes, a five-control toolbar, and the function-key bar — enough to navigate, copy, move, and delete without reading anything. Power is one step away, never on screen by default.

- **Three tiers.** (1) Always visible: navigation, find, the function-key bar. (2) One click or keystroke: the overflow menu, context menus, the command palette (every action, searchable, with its shortcut). (3) Opt-in: optional panels, semantic features, plugins, and anything configured in Settings.
- **New capability goes to the palette first.** A feature earns a permanent toolbar or status-bar position only when it is used every session; otherwise it lives in the palette, a menu, and the shortcut table.
- **Teach in place.** Tooltips, menus, and the palette show the shortcut next to the action so mouse use trains keyboard use. Empty and unavailable states say what to do next instead of only reporting absence.
- **Defaults are quiet.** Optional chrome (tree, operation centre, extra columns, previews) is off or collapsed until asked for, and remembers the user's choice per workspace.

## Do's and Don'ts

### Do:
- **Do** preserve the 20px row and 24px header rhythm for core file-management surfaces.
- **Do** use borders and tonal layers to make pane ownership and hierarchy immediately scannable.
- **Do** keep the cursor row visually stronger than selection, hover, striping, and search marks.
- **Do** keep primary actions scarce and explicit; secondary actions should remain flat.
- **Do** maintain equivalent visual behavior across browser and Tauri hosts.
- **Do** keep focus, selection, disabled, error, and progress states accessible without depending on color alone.
- **Do** size and round controls with the tokens (`--fm-control-height`, `--fm-control-height-dense`, `--fm-radius-control`, `--fm-radius`, `--fm-space-*`, `--fm-type-*`); map component-library variables onto them rather than overriding individual rules.
- **Do** render every shortcut through the shared formatter and show it beside its action.

### Don't:
- **Don't** turn Procyon into a spacious card dashboard or add whitespace that reduces simultaneous file context.
- **Don't** use gradients, glass effects, glow, or ambient shadows on permanent workspace regions.
- **Don't** round every container; rows, panes, tabs, and structural bars should remain rectilinear.
- **Don't** introduce oversized headings, decorative typography, or icon-only actions without accessible names.
- **Don't** let modals, drawers, or toolbars make their primary action scroll off-screen.
- **Don't** create host-specific visual behavior that breaks browser/Tauri parity.
- **Don't** write raw font sizes, radii, or one-off shadows in a feature stylesheet, or restyle a text field, button, or menu locally.
- **Don't** mix the two densities on one surface, or let a control render below the 12px type floor.
- **Don't** add a permanently visible control for an occasional action, or advertise a shortcut that the current host cannot dispatch.
