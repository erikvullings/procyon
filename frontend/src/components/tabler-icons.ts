import m from 'mithril';
import type { IconAttrs } from './icons';

/**
 * Vendored subset of Tabler Icons (task 0094), used for the workspace
 * toolbar's navigation/utility buttons.
 *
 * Vendored from https://github.com/tabler/tabler-icons (MIT licensed,
 * Copyright (c) 2020-2024 Paweł Kuna) — a curated subset of the
 * `icons/outline/*.svg` sources, reproduced verbatim below rather than
 * imported as an asset, since only a handful of glyphs are needed and the
 * codebase already follows this vendoring convention (see `./icons.ts`).
 */

/** Builds an icon renderer from one vendored icon's inner SVG markup (`<path>` elements). */
function trustedStrokeIcon(innerMarkup: string, extraClass: string) {
  return (attrs?: IconAttrs): m.Children => {
    const size = attrs?.size ?? 18;
    return m(
      `svg.fm-icon.fm-icon-tabler.${extraClass}${
        attrs?.className === undefined ? '' : `.${attrs.className}`
      }`,
      {
        'aria-hidden': 'true',
        viewBox: '0 0 24 24',
        width: size,
        height: size,
        fill: 'none',
        stroke: 'currentColor',
        'stroke-width': '2',
        'stroke-linecap': 'round',
        'stroke-linejoin': 'round',
      },
      // Safe: `innerMarkup` is a hardcoded constant vendored at build time, never user input.
      m.trust(innerMarkup),
    );
  };
}

/** "arrow-left" — back navigation. */
export const arrowLeftIcon = trustedStrokeIcon(
  '<path d="M5 12l14 0" /><path d="M5 12l6 6" /><path d="M5 12l6 -6" />',
  'fm-icon-arrow-left',
);

/** "arrow-right" — forward navigation. */
export const arrowRightIcon = trustedStrokeIcon(
  '<path d="M5 12l14 0" /><path d="M13 18l6 -6" /><path d="M13 6l6 6" />',
  'fm-icon-arrow-right',
);

/** "corner-left-up" — navigate to parent directory. */
export const cornerLeftUpIcon = trustedStrokeIcon(
  '<path d="M18 18h-6a3 3 0 0 1 -3 -3v-10l-4 4m8 0l-4 -4" />',
  'fm-icon-corner-left-up',
);

/** "search" — find files. */
export const searchIcon = trustedStrokeIcon(
  '<path d="M3 10a7 7 0 1 0 14 0a7 7 0 1 0 -14 0" /><path d="M21 21l-6 -6" />',
  'fm-icon-search',
);

/** "star" — an unpinned saved search. */
export const starIcon = trustedStrokeIcon(
  '<path d="M12 17.75l-6.172 3.245l1.179 -6.873l-4.993 -4.867l6.902 -1.003l3.086 -6.252l3.086 6.252l6.902 1.003l-4.993 4.867l1.179 6.873z" />',
  'fm-icon-star',
);

/** "star-filled" — a saved search pinned as a smart folder. */
export const starFilledIcon = trustedStrokeIcon(
  '<path fill="currentColor" stroke="none" d="M12 17.75l-6.172 3.245l1.179 -6.873l-4.993 -4.867l6.902 -1.003l3.086 -6.252l3.086 6.252l6.902 1.003l-4.993 4.867l1.179 6.873z" />',
  'fm-icon-star-filled',
);

/** "folder-open" — open a saved search in the current pane. */
export const folderOpenIcon = trustedStrokeIcon(
  '<path d="M5 19l14 0a2 2 0 0 0 2 -2l0 -8a2 2 0 0 0 -2 -2l-7 0l-2 -2l-5 0a2 2 0 0 0 -2 2l0 10a2 2 0 0 0 2 2z" /><path d="M3 11l18 0" />',
  'fm-icon-folder-open',
);

/** "columns" — open a saved search in the opposite pane. */
export const columnsIcon = trustedStrokeIcon(
  '<path d="M4 4m0 2a2 2 0 0 1 2 -2h12a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-12a2 2 0 0 1 -2 -2z" /><path d="M12 4l0 16" />',
  'fm-icon-columns',
);

/** "browser-plus" — open a saved search in a new tab. */
export const browserPlusIcon = trustedStrokeIcon(
  '<path d="M4 8h16" /><path d="M8 4v4" /><path d="M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-12a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2z" /><path d="M12 12v5" /><path d="M9.5 14.5h5" />',
  'fm-icon-browser-plus',
);

/** "file-search" — find text within files. */
export const contentSearchIcon = trustedStrokeIcon(
  '<path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M12 21h-5a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2h7l5 5v4.5" /><path d="M16.5 17a2.5 2.5 0 1 0 5 0a2.5 2.5 0 0 0 -5 0" /><path d="M21 21l-1.5 -1.5" />',
  'fm-icon-content-search',
);

/** "command" — command palette. */
export const commandIcon = trustedStrokeIcon(
  '<path d="M7 9a2 2 0 1 1 2 -2v10a2 2 0 1 1 -2 -2h10a2 2 0 1 1 -2 2v-10a2 2 0 1 1 2 2h-10" />',
  'fm-icon-command',
);

/** "switch-horizontal"-style compare glyph — compare the two panes' directories (task 0075). */
export const compareIcon = trustedStrokeIcon(
  '<path d="M3 8l4 -4l4 4" /><path d="M7 4l0 9" /><path d="M21 16l-4 4l-4 -4" /><path d="M17 20l0 -9" />',
  'fm-icon-compare',
);

/** "list" — plain table view (task 0134). */
export const listIcon = trustedStrokeIcon(
  '<path d="M9 6l11 0" /><path d="M9 12l11 0" /><path d="M9 18l11 0" /><path d="M5 6l0 .01" /><path d="M5 12l0 .01" /><path d="M5 18l0 .01" />',
  'fm-icon-list',
);

/** "menu-2" — compact document-navigation menu. */
export const menuIcon = trustedStrokeIcon(
  '<path d="M4 6l16 0" /><path d="M4 12l16 0" /><path d="M4 18l16 0" />',
  'fm-icon-menu',
);

/** "settings" — gear, for the settings button. */
export const settingsIcon = trustedStrokeIcon(
  '<path d="M10.325 4.317c.426 -1.756 2.924 -1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543 -.94 3.31 .826 2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756 .426 1.756 2.924 0 3.35a1.724 1.724 0 0 0 -1.066 2.573c.94 1.543 -.826 3.31 -2.37 2.37a1.724 1.724 0 0 0 -2.572 1.065c-.426 1.756 -2.924 1.756 -3.35 0a1.724 1.724 0 0 0 -2.573 -1.066c-1.543 .94 -3.31 -.826 -2.37 -2.37a1.724 1.724 0 0 0 -1.065 -2.572c-1.756 -.426 -1.756 -2.924 0 -3.35a1.724 1.724 0 0 0 1.066 -2.573c-.94 -1.543 .826 -3.31 2.37 -2.37c1 .608 2.296 .07 2.572 -1.065" /><path d="M9 12a3 3 0 1 0 6 0a3 3 0 0 0 -6 0" />',
  'fm-icon-settings',
);

/** "heart" — favourites/bookmarks. */
export const heartIcon = trustedStrokeIcon(
  '<path d="M19.5 12.572l-7.5 7.428l-7.5 -7.428a5 5 0 1 1 7.5 -6.566a5 5 0 1 1 7.5 6.572" />',
  'fm-icon-heart',
);

/** "plus" — add the current location to favourites. */
export const plusIcon = trustedStrokeIcon(
  '<path d="M12 5l0 14" /><path d="M5 12l14 0" />',
  'fm-icon-plus',
);

/** "plug" — connection management. */
export const plugIcon = trustedStrokeIcon(
  '<path d="M7 8h10v4a5 5 0 0 1 -10 0v-4z" />' +
    '<path d="M9 8v-5" /><path d="M15 8v-5" /><path d="M12 17v4" />',
  'fm-icon-plug',
);

/** "layout-grid" — workspace switcher. */
export const layoutGridIcon = trustedStrokeIcon(
  '<path d="M4 5a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1l0 -4" />' +
    '<path d="M14 5a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1l0 -4" />' +
    '<path d="M4 15a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1l0 -4" />' +
    '<path d="M14 15a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1l0 -4" />',
  'fm-icon-layout-grid',
);

/** "arrows-sort" — the grid view's sort menu toggle (task 0134). */
export const arrowsSortIcon = trustedStrokeIcon(
  '<path d="M3 9l4 -4l4 4" /><path d="M7 5l0 14" /><path d="M13 15l4 4l4 -4" /><path d="M17 19l0 -14" />',
  'fm-icon-arrows-sort',
);
/** "photo" — the grid view's photo-mode (day grouping) toggle (task 0134). */
export const photoIcon = trustedStrokeIcon(
  '<path d="M15 8h.01" />' +
    '<path d="M3 6a3 3 0 0 1 3 -3h12a3 3 0 0 1 3 3v12a3 3 0 0 1 -3 3h-12a3 3 0 0 1 -3 -3v-12z" />' +
    '<path d="M3 16l5 -5c.928 -.893 2.072 -.893 3 0l5 5" />' +
    '<path d="M14 14l1 -1c.928 -.893 2.072 -.893 3 0l3 3" />',
  'fm-icon-photo',
);

/** "grid-dots" — the pane's View toggle when in grid mode; kept visually distinct from
 * `layoutGridIcon` (used by the unrelated workspace switcher button) so the two aren't confused. */
export const gridDotsIcon = trustedStrokeIcon(
  '<path d="M5 8m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M12 8m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M19 8m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M5 12m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M12 12m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M19 12m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M5 16m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M12 16m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />' +
    '<path d="M19 16m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" />',
  'fm-icon-grid-dots',
);

/** "x" — close a dialog/disclosure panel. */
export const closeIcon = trustedStrokeIcon(
  '<path d="M18 6l-12 12" /><path d="M6 6l12 12" />',
  'fm-icon-close',
);

/** "chevron-right" — collapsed expand/collapse state (directory-tree sidebar, task 0139). */
export const chevronRightIcon = trustedStrokeIcon(
  '<path d="M9 6l6 6l-6 6" />',
  'fm-icon-chevron-right',
);

/** "chevron-down" — expanded expand/collapse state (directory-tree sidebar, task 0139). */
export const chevronDownIcon = trustedStrokeIcon(
  '<path d="M6 9l6 6l6 -6" />',
  'fm-icon-chevron-down',
);

/** "eye-off" — hidden-entry indicator in the directory table's name column. */
export const eyeOffIcon = trustedStrokeIcon(
  '<path d="M10.585 10.587a2 2 0 0 0 2.829 2.828" />' +
    '<path d="M16.681 16.673a8.717 8.717 0 0 1 -4.681 1.327c-3.6 0 -6.6 -2 -9 -6c1.272 -2.12 2.712 -3.678 4.32 -4.674m2.86 -1.146a9.055 9.055 0 0 1 1.82 -.18c3.6 0 6.6 2 9 6c-.666 1.11 -1.379 2.067 -2.138 2.87" />' +
    '<path d="M3 3l18 18" />',
  'fm-icon-eye-off',
);

/** "filter" — the pane's inline quick-filter box. */
export const filterIcon = trustedStrokeIcon(
  '<path d="M4 4h16v2.172a2 2 0 0 1 -.586 1.414l-4.414 4.414v7l-6 2v-8.5l-4.414 -4.414a2 2 0 0 1 -.586 -1.414v-2.172z" />',
  'fm-icon-filter',
);

/** "link" — symbolic-link indicator in the directory table's name column. */
export const linkIcon = trustedStrokeIcon(
  '<path d="M9 15l6 -6" />' +
    '<path d="M11 6l.463 -.536a5 5 0 0 1 7.071 7.072l-.534 .464" />' +
    '<path d="M13 18l-.397 .534a5.068 5.068 0 0 1 -7.127 0a4.972 4.972 0 0 1 0 -7.071l.523 -.461" />',
  'fm-icon-link',
);

/** "activity" — diagnostics / system health indicator. */
export const activityIcon = trustedStrokeIcon(
  '<path d="M3 12h3l3 -8l3 16l3 -8h3" />',
  'fm-icon-activity',
);

/** "copy" — copy text/image content or a metadata field to the clipboard. */
export const copyIcon = trustedStrokeIcon(
  '<path d="M9 3m0 2a2 2 0 0 1 2 -2h2a2 2 0 0 1 2 2v0a2 2 0 0 1 -2 2h-2a2 2 0 0 1 -2 -2z" />' +
    '<path d="M9 5h-2a2 2 0 0 0 -2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-12a2 2 0 0 0 -2 -2h-2" />',
  'fm-icon-copy',
);

/** "pencil" — rename a workspace. */
export const pencilIcon = trustedStrokeIcon(
  '<path d="M4 20h4l10.5 -10.5a2.828 2.828 0 1 0 -4 -4l-10.5 10.5v4" />' +
    '<path d="M13.5 6.5l4 4" />',
  'fm-icon-pencil',
);

/** "trash" — delete a workspace. */
export const trashIcon = trustedStrokeIcon(
  '<path d="M4 7l16 0" />' +
    '<path d="M10 11l0 6" />' +
    '<path d="M14 11l0 6" />' +
    '<path d="M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2 -2l1 -12" />' +
    '<path d="M9 7v-3a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v3" />',
  'fm-icon-trash',
);

/** "external-link" — open a workspace in its own OS window (task 0143). */
export const externalLinkIcon = trustedStrokeIcon(
  '<path d="M12 6h-6a2 2 0 0 0 -2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-6" />' +
    '<path d="M11 13l9 -9" />' +
    '<path d="M15 4h5v5" />',
  'fm-icon-external-link',
);

/** "info-circle" — the F3 viewer's metadata/properties sub-panel toggle. */
export const infoCircleIcon = trustedStrokeIcon(
  '<path d="M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0" /><path d="M12 9h.01" /><path d="M11 12h1v4h1" />',
  'fm-icon-info-circle',
);

/** "refresh" — the workspace switcher's per-row "Update" (resync) button. */
export const refreshIcon = trustedStrokeIcon(
  '<path d="M20 11a8.1 8.1 0 0 0 -15.5 -2m-.5 -4v4h4" />' +
    '<path d="M4 13a8.1 8.1 0 0 0 15.5 2m.5 4v-4h-4" />',
  'fm-icon-refresh',
);
