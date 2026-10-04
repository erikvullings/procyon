import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

const html = readFileSync(join(import.meta.dirname, '../site/index.html'), 'utf8');
const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)];
const script = scripts.at(-1)?.[1];
assert.ok(script, 'Site interaction script is present');

function renderDownload(platform, userAgent) {
  const elements = Object.fromEntries(
    [
      'theme-toggle',
      'hero-download',
      'footer-download',
      'footer-alternatives',
      'hero-install',
      'footer-install',
    ].map((id) => [
      id,
      {
        textContent: '',
        href: '#platforms',
        hidden: true,
        setAttribute() {},
        addEventListener() {},
      },
    ]),
  );
  elements['hero-download'].textContent = 'Choose your download';
  elements['footer-download'].textContent = 'Choose your download';
  elements['footer-alternatives'].textContent = 'All install options';

  runInNewContext(script, {
    navigator: { platform, userAgent },
    document: {
      documentElement: { getAttribute: () => 'light', setAttribute() {} },
      getElementById: (id) => elements[id],
    },
    localStorage: { getItem: () => null },
    window: { matchMedia: () => ({ addEventListener() {} }) },
  });
  return elements;
}

test('site download CTAs match the visitor platform', () => {
  const cases = [
    [
      'MacIntel',
      'Mozilla/5.0 (Macintosh; Intel Mac OS X)',
      'Install with Homebrew',
      '#platform-macos',
      true,
    ],
    [
      'Linux x86_64',
      'Mozilla/5.0 (X11; Linux x86_64)',
      'Install with Homebrew',
      '#platform-linux',
      true,
    ],
    [
      'Win32',
      'Mozilla/5.0 (Windows NT 10.0)',
      'Download for Windows',
      /Procyon_0\.3\.1_x64_en-US\.msi$/,
      false,
    ],
    [
      'Linux armv8l',
      'Mozilla/5.0 (Linux; Android 14)',
      'Choose your download',
      '#platforms',
      false,
    ],
    [
      'Linux aarch64',
      'Mozilla/5.0 (X11; Linux aarch64)',
      'Choose your download',
      '#platforms',
      false,
    ],
    [
      'MacIntel',
      'Mozilla/5.0 (iPad; CPU OS 16_0) Mobile',
      'Choose your download',
      '#platforms',
      false,
    ],
    ['unknown', 'Mozilla/5.0', 'Choose your download', '#platforms', false],
  ];
  for (const [platform, userAgent, label, href, showsBrew] of cases) {
    const elements = renderDownload(platform, userAgent);
    for (const id of ['hero-download', 'footer-download']) {
      assert.equal(elements[id].textContent, label, `${platform}: ${id} label`);
      assert.match(elements[id].href, typeof href === 'string' ? new RegExp(`^${href}$`) : href);
    }
    assert.equal(elements['hero-install'].hidden, !showsBrew);
    assert.equal(elements['footer-install'].hidden, !showsBrew);
  }
});

test('Homebrew instructions are available for macOS and Linux', () => {
  const command = 'brew install --cask erikvullings/tap/procyon';
  assert.match(html, new RegExp(`<article[^>]+id="platform-macos"[\\s\\S]*?${command}`));
  assert.match(html, new RegExp(`<article[^>]+id="platform-linux"[\\s\\S]*?${command}`));
  assert.match(
    html,
    /id="hero-install"[^>]*>Run in Terminal: <code>brew install --cask erikvullings\/tap\/procyon<\/code>/,
  );
});
