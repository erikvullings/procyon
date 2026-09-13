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
    fontSize: "0.82rem"
    fontWeight: 400
    lineHeight: 1.15
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, Liberation Mono, monospace"
    fontSize: "0.85rem"
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
    padding: "2px 4px"
    height: "20px"
  button-flat:
    backgroundColor: "transparent"
    textColor: "{colors.day-text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "2px 4px"
    height: "20px"
  input-compact:
    backgroundColor: "{colors.day-surface}"
    textColor: "{colors.day-text}"
    typography: "{typography.body}"
    rounded: "{rounded.control}"
    padding: "0 0.55rem"
    height: "20px"
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
- **Mono** (400, 0.85rem, 1.35): Terminal content, source text, hashes, and technical identifiers.

### Named Rules

**The No Hero Type Rule.** Application typography should never consume working area for visual drama. Hierarchy comes from weight, alignment, and tonal contrast before size.

## Layout

The desktop workspace is a full-viewport operational grid anchored by two equal panes. Pane boundaries, tab strips, breadcrumbs, table headers, status bars, optional drawers, and the function-key bar form a continuous stack with no ornamental gaps. Splitters may have a generous invisible hit area, but their visible rule remains hairline-thin.

The base density unit is the 20px file row. Structural headers are 24px, compact controls normally resolve to 20px, and most internal padding uses 2px, 4px, 8px, 12px, or 16px. Forms may breathe more than file lists, but should still align labels and controls to this compact rhythm. Dialog content scrolls within its working region while titles and action footers remain available.

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

Compact and explicit, with no ornamental lift.

- **Shape:** 2px radius, 1px border, minimum 20px height, and 2px 4px padding.
- **Primary:** Instrument Blue fill with high-contrast text; reserve it for the decisive action in a dialog or compact action group.
- **Hover / Focus:** Use a tonal hover change. Do not add a shadow or transform; keyboard focus must remain accessible through the component's semantic state and surrounding context.
- **Flat:** Transparent with normal text; hover uses the theme hover wash and no border emphasis.

### Inputs / Fields

Inputs are compact instruments, not floating decorative fields.

- **Style:** Surface background, 1px divider border, 2px radius, 20px height, and `0 0.55rem` horizontal padding.
- **Labels:** Static, above the field, muted, and close to the control.
- **Focus:** Preserve the neutral border with no blue glow, thick ring, or animated floating-label movement. Selection and caret behavior provide the text-entry cue.
- **Error / Disabled:** Use semantic color or muted opacity in addition to accessible text; never rely on color alone.

### Navigation

Tabs, breadcrumbs, toolbars, and the function bar form one continuous control frame.

- **Tabs:** Compact 24px geometry, square joins, muted inactive state, and stronger active text/surface contrast.
- **Breadcrumbs:** Single-line, low-chrome path navigation with truncation rather than wrapping.
- **Function Bar:** Full-width bottom command strip with evenly distributed actions and visible function-key labels. It remains visually available while the workspace scrolls internally.

### Menus and Command Palette

Menus use the raised surface, fine border, overlay shadow, and compact rows. Hover and keyboard focus use the hover/selection washes. Shortcut chips use a bordered inactive-selection fill so key labels remain readable in dark mode.

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

## Do's and Don'ts

### Do:
- **Do** preserve the 20px row and 24px header rhythm for core file-management surfaces.
- **Do** use borders and tonal layers to make pane ownership and hierarchy immediately scannable.
- **Do** keep the cursor row visually stronger than selection, hover, striping, and search marks.
- **Do** keep primary actions scarce and explicit; secondary actions should remain flat.
- **Do** maintain equivalent visual behavior across browser and Tauri hosts.
- **Do** keep focus, selection, disabled, error, and progress states accessible without depending on color alone.

### Don't:
- **Don't** turn Procyon into a spacious card dashboard or add whitespace that reduces simultaneous file context.
- **Don't** use gradients, glass effects, glow, or ambient shadows on permanent workspace regions.
- **Don't** round every container; rows, panes, tabs, and structural bars should remain rectilinear.
- **Don't** introduce oversized headings, decorative typography, or icon-only actions without accessible names.
- **Don't** let modals, drawers, or toolbars make their primary action scroll off-screen.
- **Don't** create host-specific visual behavior that breaks browser/Tauri parity.
