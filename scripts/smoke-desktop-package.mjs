import { spawn, spawnSync } from 'node:child_process';
import {
  closeSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.env.CI !== 'true') {
  throw new Error('the install-and-launch smoke test is restricted to disposable CI runners');
}

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const bundleRoot = join(repoRoot, 'target', 'release', 'bundle');
const evidenceRoot = resolve(
  process.env.PROCYON_QUALIFICATION_EVIDENCE_ROOT ??
    mkdtempSync(join(tmpdir(), 'procyon-package-evidence-')),
);
mkdirSync(evidenceRoot, { recursive: true });
const filenameCanary = process.env.PROCYON_QUALIFICATION_FILENAME_CANARY_FILE
  ? readFileSync(process.env.PROCYON_QUALIFICATION_FILENAME_CANARY_FILE, 'utf8').trim()
  : undefined;
if (filenameCanary && (!/^[a-zA-Z0-9._-]+$/u.test(filenameCanary) || filenameCanary.length > 180)) {
  throw new Error('qualification filename canary must be a bounded portable filename');
}
const expectedCatalogDirectory = process.env.PROCYON_QUALIFICATION_CATALOG_DIRECTORY
  ? resolve(process.env.PROCYON_QUALIFICATION_CATALOG_DIRECTORY)
  : undefined;

function filesBelow(root) {
  if (!existsSync(root)) return [];
  const paths = [];
  const pending = [root];
  while (pending.length > 0) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const candidate = join(directory, entry.name);
      paths.push(candidate);
      if (entry.isDirectory() && !entry.isSymbolicLink()) pending.push(candidate);
    }
  }
  return paths;
}

function requiredFile(extension) {
  const match = filesBelow(bundleRoot).find((path) => path.toLowerCase().endsWith(extension));
  if (!match) throw new Error(`expected a ${extension} artifact below ${bundleRoot}`);
  return match;
}

function assertEmbeddedCatalog(root) {
  if (!expectedCatalogDirectory) return;
  for (const name of ['catalog.json', 'catalog.sig']) {
    const matches = filesBelow(root).filter(
      (candidate) => basename(candidate) === name && basename(dirname(candidate)) === 'semantic',
    );
    if (matches.length !== 1) {
      throw new Error(`installed package must contain exactly one semantic/${name}`);
    }
    if (!readFileSync(matches[0]).equals(readFileSync(join(expectedCatalogDirectory, name)))) {
      throw new Error(`installed semantic/${name} differs from the signed qualification input`);
    }
  }
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${command} failed: ${result.stderr || result.stdout}`);
  }
  return result;
}

function isolatedEnvironment(root) {
  const home = join(root, 'home');
  const appData = join(root, 'app-data');
  const localAppData = join(root, 'local-app-data');
  const config = join(root, 'config');
  const data = join(root, 'data');
  for (const directory of [home, appData, localAppData, config, data]) {
    mkdirSync(directory, { recursive: true });
  }
  if (filenameCanary) writeFileSync(join(home, filenameCanary), 'qualification fixture');
  return {
    ...process.env,
    HOME: home,
    APPDATA: appData,
    LOCALAPPDATA: localAppData,
    XDG_CONFIG_HOME: config,
    XDG_DATA_HOME: data,
    FM_LOG_FILE: join(evidenceRoot, 'fm-desktop.log'),
    HTTP_PROXY: 'http://127.0.0.1:9',
    HTTPS_PROXY: 'http://127.0.0.1:9',
    ALL_PROXY: 'http://127.0.0.1:9',
    NO_PROXY: '',
  };
}

async function assertLaunches(command, args, root, label) {
  const stdout = openSync(join(evidenceRoot, `${label}-stdout.log`), 'w');
  const stderr = openSync(join(evidenceRoot, `${label}-stderr.log`), 'w');
  try {
    const child = spawn(command, args, {
      env: isolatedEnvironment(root),
      stdio: ['ignore', stdout, stderr],
    });
    let spawnError;
    child.once('error', (error) => {
      spawnError = error;
    });
    await new Promise((resolve) => setTimeout(resolve, 5_000));
    if (spawnError) throw spawnError;
    if (child.exitCode !== null) {
      throw new Error(`${command} exited during the launch smoke window (${child.exitCode})`);
    }
    child.kill();
    await new Promise((resolve) => child.once('exit', resolve));
    if (filesBelow(root).some((candidate) => basename(candidate) === 'worker.pid')) {
      throw new Error('first launch started an uninstalled semantic worker');
    }
  } finally {
    closeSync(stdout);
    closeSync(stderr);
  }
}

async function smokeMacos() {
  const dmg = requiredFile('.dmg');
  const mount = mkdtempSync(join(tmpdir(), 'procyon-dmg-'));
  const installRoot = mkdtempSync(join(tmpdir(), 'procyon-install-'));
  try {
    run('hdiutil', ['attach', '-readonly', '-nobrowse', '-mountpoint', mount, dmg]);
    const sourceApp = filesBelow(mount).find((candidate) => candidate.endsWith('.app'));
    if (!sourceApp) throw new Error('DMG did not contain a Procyon application');
    const installedApp = join(installRoot, basename(sourceApp));
    cpSync(sourceApp, installedApp, { recursive: true });
    assertEmbeddedCatalog(installedApp);
    await assertLaunches(
      join(installedApp, 'Contents', 'MacOS', 'Procyon'),
      [],
      installRoot,
      'macos-installed',
    );
  } finally {
    spawnSync('hdiutil', ['detach', mount], { encoding: 'utf8' });
    rmSync(mount, { recursive: true, force: true });
    rmSync(installRoot, { recursive: true, force: true });
  }
}

async function smokeWindows() {
  const msi = requiredFile('.msi');
  requiredFile('-setup.exe');
  const installRoot = mkdtempSync(join(tmpdir(), 'procyon-install-'));
  try {
    run('msiexec.exe', [
      '/i',
      msi,
      '/qn',
      `/L*v`,
      join(evidenceRoot, 'windows-msi-install.log'),
      `INSTALLDIR=${installRoot}`,
    ]);
    const executable = filesBelow(installRoot).find(
      (path) => basename(path).toLowerCase() === 'procyon.exe',
    );
    if (!executable) throw new Error('MSI did not install Procyon.exe');
    assertEmbeddedCatalog(installRoot);
    await assertLaunches(executable, [], installRoot, 'windows-installed');
    run('msiexec.exe', ['/x', msi, '/qn', '/L*v', join(evidenceRoot, 'windows-msi-uninstall.log')]);
  } finally {
    rmSync(installRoot, { recursive: true, force: true });
  }
}

async function smokeLinux() {
  const deb = requiredFile('.deb');
  const appImage = requiredFile('.appimage');
  const installRoot = mkdtempSync(join(tmpdir(), 'procyon-deb-install-'));
  const appImageRoot = mkdtempSync(join(tmpdir(), 'procyon-appimage-install-'));
  try {
    run('dpkg-deb', ['--extract', deb, installRoot]);
    const executable = filesBelow(installRoot).find((candidate) => {
      const name = basename(candidate).toLowerCase();
      return name === 'procyon' && !candidate.includes('/resources/');
    });
    if (!executable) throw new Error('DEB did not install a Procyon executable');
    assertEmbeddedCatalog(installRoot);
    await assertLaunches(
      'xvfb-run',
      ['--auto-servernum', executable],
      installRoot,
      'linux-deb-installed',
    );

    run(appImage, ['--appimage-extract'], { cwd: appImageRoot });
    const appRun = join(appImageRoot, 'squashfs-root', 'AppRun');
    if (!existsSync(appRun)) throw new Error('AppImage did not extract AppRun');
    assertEmbeddedCatalog(join(appImageRoot, 'squashfs-root'));
    await assertLaunches(
      'xvfb-run',
      ['--auto-servernum', appRun],
      appImageRoot,
      'linux-appimage-installed',
    );
  } finally {
    rmSync(installRoot, { recursive: true, force: true });
    rmSync(appImageRoot, { recursive: true, force: true });
  }
}

if (process.platform === 'darwin') await smokeMacos();
else if (process.platform === 'win32') await smokeWindows();
else if (process.platform === 'linux') await smokeLinux();
else throw new Error(`desktop packaging smoke test is unsupported on ${process.platform}`);
