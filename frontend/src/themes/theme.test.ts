import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const themeCss = readFileSync(join(process.cwd(), 'src/themes/theme.css'), 'utf8');
const materializedCss = readFileSync(
  join(process.cwd(), 'src/themes/mithril-materialized-procyon.css'),
  'utf8',
);
const directoryTableCss = readFileSync(
  join(process.cwd(), 'src/features/directory-table/directory-table.css'),
  'utf8',
);
const paneCss = readFileSync(join(process.cwd(), 'src/features/panes/pane.css'), 'utf8');
const fileViewerCss = readFileSync(
  join(process.cwd(), 'src/features/preview/file-viewer.css'),
  'utf8',
);

const REQUIRED_TOKENS = [
  '--fm-background',
  '--fm-surface',
  '--fm-surface-elevated',
  '--fm-text',
  '--fm-text-muted',
  '--fm-border',
  '--fm-accent',
  '--fm-selection',
  '--fm-selection-inactive',
  '--fm-hover',
  '--fm-error',
  '--fm-warning',
  '--fm-success',
  '--fm-row-height',
  '--fm-font-family',
  '--fm-font-size',
  '--fm-radius',
  '--fm-shadow',
] as const;

function themeBlock(selector: RegExp): string {
  const block = themeCss.match(selector)?.[1];
  if (block === undefined) {
    throw new Error(`theme block ${selector.source} was not found`);
  }
  return block;
}

function tokenValue(block: string, token: string): string {
  const value = block.match(new RegExp(`${token}:\\s*(#[\\da-f]{6})`, 'i'))?.[1];
  if (value === undefined) {
    throw new Error(`${token} is not a six-digit hex colour`);
  }
  return value;
}

function relativeLuminance(hex: string): number {
  if (!/^#[\da-f]{6}$/i.test(hex)) {
    throw new Error(`invalid colour ${hex}`);
  }
  const linearChannel = (offset: number): number => {
    const channel = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linearChannel(1) + 0.7152 * linearChannel(3) + 0.0722 * linearChannel(5);
}

function contrastRatio(foreground: string, background: string): number {
  const foregroundLuminance = relativeLuminance(foreground);
  const backgroundLuminance = relativeLuminance(background);
  const lighter = Math.max(foregroundLuminance, backgroundLuminance);
  const darker = Math.min(foregroundLuminance, backgroundLuminance);
  return (lighter + 0.05) / (darker + 0.05);
}

describe('theme stylesheet', () => {
  it('contains long operation source previews within their card', () => {
    expect(themeBlock(/\.fm-operation\s*\{([^}]*)\}/)).toContain('flex-direction: column');
    expect(themeBlock(/\.fm-operation\s*\{([^}]*)\}/)).toContain('overflow: hidden');
    expect(themeBlock(/\.fm-operation-summary\s*\{([^}]*)\}/)).toContain('flex-wrap: wrap');
    expect(themeBlock(/\.fm-operation-source-preview\s*\{([^}]*)\}/)).toContain('min-width: 0');
    expect(themeBlock(/\.fm-operation-source-preview\s*\{([^}]*)\}/)).toContain(
      'overflow-wrap: anywhere',
    );
  });

  it('aligns operation card headers and controls across uneven content', () => {
    expect(themeBlock(/\.fm-operation-list\s*\{([^}]*)\}/)).toContain('align-items: stretch');
    expect(themeBlock(/\.fm-operation-controls\s*\{([^}]*margin-top[^}]*)\}/)).toContain(
      'margin-top: auto',
    );
  });

  it('keeps the Operations Centre close control fixed at its top-right edge', () => {
    expect(themeBlock(/\.fm-operation-centre\s*\{([^}]*border-top[^}]*)\}/)).toContain(
      'position: relative',
    );
    const sharedControls = themeBlock(
      /\.fm-operation-centre-close,\s*\.fm-operation-centre-show-all\s*\{([^}]*)\}/,
    );
    expect(sharedControls).toContain('position: absolute');
    expect(sharedControls).toContain('right: 0.25rem');
    const close = themeBlock(/\.fm-operation-centre-close\s*\{([^}]*)\}/);
    expect(close).toContain('top: 0.25rem');
  });

  it('defines every file-manager design token', () => {
    for (const token of REQUIRED_TOKENS) {
      expect(themeCss).toContain(`${token}:`);
    }
  });

  it('provides explicit light and dark themes plus a system-dark fallback', () => {
    expect(themeCss).toMatch(/:root,\s*\[data-theme=["']light["']\]/);
    expect(themeCss).toMatch(/\[data-theme=["']dark["']\]/);
    expect(themeCss).toMatch(
      /@media \(prefers-color-scheme: dark\)[\s\S]*:root:not\(\[data-theme\]\)/,
    );
  });

  it('maps mithril-materialized theme variables to file-manager tokens', () => {
    const mappings = [
      '--mm-primary-color: var(--fm-accent)',
      '--mm-background-color: var(--fm-background)',
      '--mm-surface-color: var(--fm-surface)',
      '--mm-modal-background: var(--fm-surface-elevated)',
      '--mm-text-primary: var(--fm-text)',
      '--mm-text-secondary: var(--fm-text-muted)',
      '--mm-border-color: var(--fm-border)',
      '--mm-error-color: var(--fm-error)',
    ] as const;

    for (const mapping of mappings) {
      expect(themeCss).toContain(mapping);
    }
  });

  it('keeps delete-workspace modal controls inside its compact surface', () => {
    expect(themeCss).toMatch(
      /\.modal\.fm-delete-workspace-modal\s*>\s*button\.mm-modal-close-button\s*\{[^}]*top:\s*0\.25rem[^}]*right:\s*0\.25rem/s,
    );
    expect(themeCss).toMatch(
      /\.modal\.fm-delete-workspace-modal\s+\.modal-footer\s*\{[^}]*box-sizing:\s*border-box[^}]*display:\s*flex[^}]*justify-content:\s*flex-end[^}]*gap:\s*0\.5rem/s,
    );
  });

  it('shows the keyboard-focused permanent-delete action in the error colour', () => {
    expect(themeCss).toMatch(
      /\.fm-permanent-delete-modal\s+\.fm-permanent-delete-confirm:focus\s*\{[^}]*color:\s*var\(--fm-error\)/s,
    );
    expect(themeCss).not.toMatch(
      /\.fm-permanent-delete-modal\s+\.fm-permanent-delete-confirm:focus\s*\{[^}]*outline:\s*2px/s,
    );
  });

  it('densifies the mithril-materialized controls used by the application', () => {
    for (const selector of [
      '.modal',
      '.btn',
      '.btn-flat',
      '.input-field',
      '.switch',
      '.select-wrapper',
    ]) {
      expect(materializedCss).toContain(selector);
    }
    expect(materializedCss).toContain('var(--fm-row-height)');
  });

  it('tunes mithril-materialized form chrome to match the Procyon layout', () => {
    expect(materializedCss).toContain('.input-field > label');
    expect(materializedCss).toContain('input[type="number"]::-webkit-inner-spin-button');
    expect(materializedCss).toContain('.switch label span:not(.lever)');
    expect(materializedCss).toContain('.modal.fm-find-files-modal > button.modal-close');
    expect(materializedCss).toContain('.dropdown-content li.selected');
    expect(materializedCss).toContain(
      '[data-theme="dark"] .fm-app-shell .select-wrapper .dropdown-content li.active',
    );
    expect(materializedCss).toContain('position: static');
    expect(materializedCss).toContain('input[type="number"]::-webkit-inner-spin-button');
  });

  it('removes transitions and animations when reduced motion is requested', () => {
    expect(themeCss).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*transition-duration:\s*0\.01ms/,
    );
    expect(themeCss).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*animation-duration:\s*0\.01ms/,
    );
  });

  it('meets WCAG AA for text on surfaces and both selection states', () => {
    const themes = [
      themeBlock(/:root,\s*\[data-theme=["']light["']\]\s*\{([^}]*)\}/),
      themeBlock(/\[data-theme=["']dark["']\]\s*\{([^}]*)\}/),
    ];

    for (const theme of themes) {
      const text = tokenValue(theme, '--fm-text');
      for (const backgroundToken of ['--fm-surface', '--fm-selection', '--fm-selection-inactive']) {
        expect(contrastRatio(text, tokenValue(theme, backgroundToken))).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('uses subtle selection backgrounds and a distinct brighter cursor highlight', () => {
    expect(themeCss).not.toMatch(/(?:^|\n)\.fm-selected-row\s*\{/);
    expect(themeCss).not.toMatch(/(?:^|\n)\.fm-cursor-row\s*\{/);
    // The filled/tinted treatment additionally requires `:focus-within` (task 0139 follow-up):
    // an "active" pane that lost real DOM focus to the directory-tree sidebar falls through to
    // the unconditional box-shadow-only rule below instead.
    expect(themeCss).toMatch(
      /\.fm-pane\[data-active="true"\]:focus-within\s+\.fm-selected-row\s*\{[^}]*color:\s*var\(--fm-selected-row-text\)/s,
    );
    expect(themeCss).toMatch(
      /\.fm-pane\[data-active="true"\]:focus-within\s+\.fm-selected-row\s*\{[^}]*background:[^}]*18%/s,
    );
    expect(themeCss).toMatch(
      /\.fm-pane\[data-active="true"\]:focus-within\s+\.fm-cursor-row:not\(\.fm-selected-row\)\s*\{[^}]*background-color:[^}]*48%[^}]*color:\s*var\(--fm-cursor-row-text\)/s,
    );
    // The cursor row keeps a distinctive outline even when it's also marked, so the mark's amber
    // text color (above) isn't washed out by the cursor's own background/text override - and,
    // unlike the fill rules above, this one is deliberately not gated on `:focus-within`, so the
    // cursor position stays visible in a pane that lost focus to the tree sidebar.
    expect(themeCss).toMatch(
      /\.fm-pane\[data-active="true"\]\s+\.fm-cursor-row\s*\{[^}]*box-shadow:[^}]*var\(--fm-accent\)/s,
    );
    expect(themeCss).toMatch(
      /\[data-theme="dark"\][^}]*\.fm-pane\[data-active="true"\]:focus-within\s+\.fm-selected-row\s*\{[^}]*background:/s,
    );
  });

  it('does not highlight directory rows on mouse hover', () => {
    expect(directoryTableCss).not.toMatch(/\.fm-directory-row:hover/);
  });

  it('keeps the arrow cursor over directory rows', () => {
    expect(directoryTableCss).toMatch(/\.fm-directory-row\s*[,{][^}]*cursor:\s*default/s);
  });

  it('keeps directory and viewer content inside its pane grid track', () => {
    expect(paneCss).toMatch(/\.fm-pane\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)/s);
    expect(fileViewerCss).toMatch(/\.fm-file-viewer\s*\{[^}]*min-width:\s*0/s);
  });

  it('keeps the favourites menu width stable when row actions appear', () => {
    const menuRule = paneCss.match(/\.fm-favourites-menu\s*\{([^}]*)\}/s)?.[1];

    expect(menuRule).toContain('width: min(18rem, calc(100vw - 0.5rem))');
    expect(menuRule).not.toMatch(/\bmin-width:/);
    expect(menuRule).not.toMatch(/\bmax-width:/);
  });

  it('allows a longer favourites list before scrolling', () => {
    const menuRule = paneCss.match(/\.fm-favourites-menu\s*\{([^}]*)\}/s)?.[1];

    expect(menuRule).toContain(
      'max-height: min(32rem, calc(100vh - var(--fm-row-height) * 2 - 0.75rem))',
    );
  });

  it('aligns server and OneDrive connection statuses at the right edge', () => {
    const connectionRowRule = paneCss.match(
      /\.fm-favourites-recents > button\.fm-server-item,\s*\.fm-favourites-recents > button\.fm-cloud-item\s*\{([^}]*)\}/s,
    )?.[1];

    expect(connectionRowRule).toContain('display: flex');
    expect(connectionRowRule).toContain('justify-content: space-between');
  });

  it('keeps Markdown body and headings on the compact application scale', () => {
    expect(fileViewerCss).toMatch(
      /\.fm-file-viewer-markdown\s*\{[^}]*font-size:\s*var\(--fm-font-size\)/s,
    );
    expect(fileViewerCss).toMatch(/\.fm-file-viewer-markdown h1\s*\{\s*font-size:\s*1\.5em;/);
    expect(fileViewerCss).toMatch(/\.fm-file-viewer-markdown h6\s*\{\s*font-size:\s*0\.92em;/);
    expect(fileViewerCss).toMatch(/\.fm-file-viewer-markdown code\s*\{[^}]*font-size:\s*0\.92em/s);
    expect(fileViewerCss).toMatch(
      /\.fm-file-viewer-markdown sub,[^}]*\{[^}]*vertical-align:\s*baseline/s,
    );
    expect(fileViewerCss).toMatch(
      /\.fm-file-viewer-markdown ul[^}]*\{[^}]*list-style-type:\s*disc !important/s,
    );
  });
});
