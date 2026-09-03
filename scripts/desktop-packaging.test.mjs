import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { load } from 'js-yaml';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

function read(...segments) {
  return readFileSync(join(repoRoot, ...segments), 'utf8');
}

function workflow(name) {
  return load(read('.github', 'workflows', name));
}

function workflowText(name) {
  return read('.github', 'workflows', name);
}

test('desktop Cargo metadata and Tauri bootstrap config agree on product identity and icons', () => {
  const workspaceCargo = read('Cargo.toml');
  const cargo = read('apps', 'fm-desktop', 'src-tauri', 'Cargo.toml');
  const config = JSON.parse(read('apps', 'fm-desktop', 'src-tauri', 'tauri.conf.json'));
  const workspaceVersion = workspaceCargo.match(/^version = "([^"]+)"$/m)?.[1];

  assert.match(cargo, /version\.workspace = true/);
  assert.match(cargo, /\[package\.metadata\.desktop\]/);
  assert.match(cargo, /product-name = "Procyon"/);
  assert.match(cargo, /identifier = "nl\.erikvullings\.procyon"/);
  assert.match(cargo, /icons = \[/);
  assert.equal(config.productName, 'Procyon');
  assert.equal(config.mainBinaryName, 'Procyon');
  assert.equal(config.identifier, 'nl.erikvullings.procyon');

  const derived = JSON.parse(
    execFileSync('node', ['scripts/build-tauri.mjs', '--print-config'], {
      cwd: repoRoot,
      encoding: 'utf8',
    }),
  );
  assert.equal(derived.productName, config.productName);
  assert.equal(derived.version, workspaceVersion);
  assert.equal(derived.identifier, config.identifier);
  assert.deepEqual(derived.bundle.icon, config.bundle.icon);
});

test('the root Tauri build command uses the metadata-derived build wrapper', () => {
  const rootPackage = JSON.parse(read('package.json'));
  assert.equal(rootPackage.scripts['build:tauri'], 'node scripts/build-tauri.mjs');
});

test('Tauri targets installable macOS, Windows, and Linux bundle formats', () => {
  const config = JSON.parse(read('apps', 'fm-desktop', 'src-tauri', 'tauri.conf.json'));
  assert.deepEqual(config.bundle.targets, ['app', 'dmg', 'msi', 'nsis', 'deb', 'appimage']);
});

test('pull-request CI builds desktop bundles without any signing credentials', () => {
  const ciText = workflowText('ci.yml');
  const ci = workflow('ci.yml');
  assert.deepEqual([...ci.jobs.desktop.strategy.matrix.os].sort(), [
    'macos-latest',
    'windows-latest',
  ]);
  assert.match(JSON.stringify(ci.jobs.desktop), /build:tauri/);
  assert.doesNotMatch(ciText, /APPLE_|WINDOWS_|CERTIFICATE|SIGNING|notariz/i);
});

test('protected release workflow signs and notarizes macOS packages only', () => {
  const releaseText = workflowText('release-desktop.yml');
  const release = workflow('release-desktop.yml');
  assert.deepEqual(release.on.push.tags, ['v*']);
  assert.ok(release.on.workflow_dispatch);
  assert.equal(release.on.pull_request, undefined);

  for (const jobName of ['macos', 'windows', 'linux']) {
    assert.equal(release.jobs[jobName].environment, 'desktop-release');
  }

  assert.match(releaseText, /secrets\.APPLE_CERTIFICATE/);
  assert.match(releaseText, /secrets\.APPLE_CERTIFICATE_PASSWORD/);
  assert.match(releaseText, /secrets\.APPLE_API_ISSUER/);
  assert.match(releaseText, /secrets\.APPLE_API_KEY/);
  assert.match(releaseText, /secrets\.APPLE_API_KEY_P8/);
  assert.match(releaseText, /Developer ID Application/);
  assert.match(releaseText, /codesign --verify/);
  assert.match(releaseText, /spctl --assess/);
  assert.match(releaseText, /stapler validate/);
  assert.doesNotMatch(releaseText, /WINDOWS_CERTIFICATE|signtool/i);
});

test('release workflow publishes signed macOS and unsigned Windows and Linux packages', () => {
  const releaseText = workflowText('release-desktop.yml');
  const release = workflow('release-desktop.yml');
  const chocolateyText = workflowText('publish-chocolatey.yml');
  const chocolatey = workflow('publish-chocolatey.yml');

  assert.match(releaseText, /build:tauri --target universal-apple-darwin/);
  assert.equal(release.jobs.linux['runs-on'], 'ubuntu-22.04');
  assert.match(releaseText, /libwebkit2gtk-4\.1-dev/);
  assert.match(releaseText, /bundle\/deb\/\*\.deb/);
  assert.match(releaseText, /bundle\/appimage\/\*\.AppImage/);
  assert.deepEqual(release.jobs.homebrew.needs, ['macos']);
  assert.equal(release.jobs.homebrew.environment, 'desktop-release');
  assert.equal(release.jobs.chocolatey.needs, 'windows');
  assert.equal(release.jobs.chocolatey.uses, './.github/workflows/publish-chocolatey.yml');
  assert.equal(release.jobs.chocolatey.with.release_tag, `\${{ github.ref_name }}`);
  assert.equal(chocolatey.jobs.chocolatey.environment, 'desktop-release');
  assert.ok(chocolatey.on.workflow_dispatch);
  assert.ok(chocolatey.on.workflow_call);
  assert.match(releaseText, /vars\.HOMEBREW_TAP_REPOSITORY/);
  assert.match(releaseText, /secrets\.HOMEBREW_TAP_TOKEN/);
  assert.match(chocolateyText, /secrets\.CHOCOLATEY_API_KEY/);
  assert.match(chocolateyText, /choco pack/);
  assert.match(chocolateyText, /choco push/);
});

test('package-manager generator creates a Homebrew cask and Chocolatey installer package', () => {
  const outputRoot = mkdtempSync(join(tmpdir(), 'procyon-packages-'));
  const checksum = 'a'.repeat(64);
  const commonArgs = ['--version', '1.2.3', '--sha256', checksum, '--repository', 'example/fm'];

  const caskPath = join(outputRoot, 'Casks', 'procyon.rb');
  execFileSync(
    'node',
    [
      'scripts/generate-package-manager-files.mjs',
      'homebrew',
      ...commonArgs,
      '--asset',
      'Procyon_1.2.3_universal.dmg',
      '--output',
      caskPath,
    ],
    { cwd: repoRoot },
  );
  const cask = readFileSync(caskPath, 'utf8');
  assert.match(cask, /cask "procyon" do/);
  assert.match(cask, /version "1\.2\.3"/);
  assert.match(cask, new RegExp(`sha256 "${checksum}"`));
  assert.match(cask, /releases\/download\/v1\.2\.3\/Procyon_1\.2\.3_universal\.dmg/);
  assert.match(cask, /app "Procyon\.app"/);

  const chocolateyDir = join(outputRoot, 'chocolatey');
  execFileSync(
    'node',
    [
      'scripts/generate-package-manager-files.mjs',
      'chocolatey',
      ...commonArgs,
      '--asset',
      'Procyon_1.2.3_x64-setup.exe',
      '--output',
      chocolateyDir,
    ],
    { cwd: repoRoot },
  );
  const nuspec = readFileSync(join(chocolateyDir, 'procyon.nuspec'), 'utf8');
  const install = readFileSync(join(chocolateyDir, 'tools', 'chocolateyinstall.ps1'), 'utf8');
  assert.match(nuspec, /<id>procyon<\/id>/);
  assert.match(nuspec, /<version>1\.2\.3<\/version>/);
  assert.match(
    nuspec,
    /<licenseUrl>https:\/\/github\.com\/example\/fm\/blob\/main\/LICENSE<\/licenseUrl>/,
  );
  assert.doesNotMatch(nuspec, /<license(?:\s|>)/);
  assert.match(nuspec, /<iconUrl>.*icons\/icon\.png<\/iconUrl>/);
  assert.match(nuspec, /releases\/tag\/v1\.2\.3/);
  assert.match(install, /Install-ChocolateyPackage @packageArgs/);
  assert.match(install, /silentArgs\s*= '\/S'/);
  assert.match(install, new RegExp(`checksum64\\s+= '${checksum}'`));
});

test('Chocolatey nuspec uses dotted version identifiers, not our hyphenated pre-release suffix', () => {
  const outputRoot = mkdtempSync(join(tmpdir(), 'procyon-packages-'));
  const checksum = 'b'.repeat(64);
  const chocolateyDir = join(outputRoot, 'chocolatey');
  execFileSync(
    'node',
    [
      'scripts/generate-package-manager-files.mjs',
      'chocolatey',
      '--version',
      '0.1.0-6',
      '--sha256',
      checksum,
      '--repository',
      'example/fm',
      '--asset',
      'Procyon_0.1.0-6_x64-setup.exe',
      '--output',
      chocolateyDir,
    ],
    { cwd: repoRoot },
  );
  const nuspec = readFileSync(join(chocolateyDir, 'procyon.nuspec'), 'utf8');
  assert.match(nuspec, /<version>0\.1\.0\.6<\/version>/);
  assert.doesNotMatch(nuspec, /<version>0\.1\.0-6<\/version>/);
  // The GitHub release/tag/asset names keep the hyphenated version - only the nuspec's own
  // <version> (and therefore the resulting .nupkg filename) needs the dotted form.
  assert.match(nuspec, /releases\/tag\/v0\.1\.0-6/);
});

test('desktop CI runs platform packaging smoke tests after building', () => {
  const desktop = workflow('ci.yml').jobs.desktop;
  const commands = (desktop.steps ?? [])
    .map((step) => step.run)
    .filter((command) => typeof command === 'string');
  assert.ok(commands.some((command) => /smoke-desktop-package\.mjs/.test(command)));
});

test('README documents release versioning, package managers, smoke checks, and no auto-update', () => {
  const readme = read('README.md');
  assert.match(readme, /## Desktop releases/);
  assert.match(readme, /Cargo\.toml/);
  assert.match(readme, /v<version>/);
  assert.match(readme, /release notes/i);
  assert.match(readme, /Developer ID Application/i);
  assert.match(readme, /notariz/i);
  assert.match(readme, /SmartScreen/i);
  assert.match(readme, /APPLE_CERTIFICATE/);
  assert.match(readme, /APPLE_API_KEY_P8/);
  assert.doesNotMatch(readme, /APPLE_ID|APPLE_PASSWORD|APPLE_TEAM_ID|WINDOWS_CERTIFICATE/);
  assert.match(readme, /manual smoke/i);
  assert.match(readme, /brew install --cask/);
  assert.match(readme, /choco install procyon/);
  assert.match(readme, /HOMEBREW_TAP_TOKEN/);
  assert.match(readme, /CHOCOLATEY_API_KEY/);
  assert.match(readme, /auto-update is not included/i);
  assert.match(readme, /\.deb/);
  assert.match(readme, /AppImage/);
});
