import { spawn, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.env.CI !== 'true') {
  throw new Error('the install-and-launch smoke test is restricted to disposable CI runners');
}

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const bundleRoot = join(repoRoot, 'target', 'release', 'bundle');

function filesBelow(root) {
  if (!existsSync(root)) return [];
  return readdirSync(root, { recursive: true, encoding: 'utf8' }).map((path) => join(root, path));
}

function requiredFile(extension) {
  const match = filesBelow(bundleRoot).find((path) => path.toLowerCase().endsWith(extension));
  if (!match) throw new Error(`expected a ${extension} artifact below ${bundleRoot}`);
  return match;
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.status !== 0) {
    throw new Error(`${command} failed: ${result.stderr || result.stdout}`);
  }
}

async function assertLaunches(executable) {
  const child = spawn(executable, [], { stdio: 'ignore' });
  await new Promise((resolve) => setTimeout(resolve, 5_000));
  if (child.exitCode !== null) {
    throw new Error(`${executable} exited during the launch smoke window (${child.exitCode})`);
  }
  child.kill();
}

async function smokeMacos() {
  requiredFile('.dmg');
  const sourceApp = filesBelow(join(bundleRoot, 'macos')).find((path) => path.endsWith('.app'));
  if (!sourceApp) throw new Error('expected a .app artifact below target/release/bundle/macos');

  const installRoot = mkdtempSync(join(tmpdir(), 'procyon-install-'));
  const installedApp = join(installRoot, basename(sourceApp));
  try {
    cpSync(sourceApp, installedApp, { recursive: true });
    await assertLaunches(join(installedApp, 'Contents', 'MacOS', 'Procyon'));
  } finally {
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
      join(installRoot, 'install.log'),
      `INSTALLDIR=${installRoot}`,
    ]);
    const executable = filesBelow(installRoot).find(
      (path) => basename(path).toLowerCase() === 'procyon.exe',
    );
    if (!executable) throw new Error('MSI did not install Procyon.exe');
    await assertLaunches(executable);
    run('msiexec.exe', ['/x', msi, '/qn']);
  } finally {
    rmSync(installRoot, { recursive: true, force: true });
  }
}

if (process.platform === 'darwin') await smokeMacos();
else if (process.platform === 'win32') await smokeWindows();
else throw new Error(`desktop packaging smoke test is unsupported on ${process.platform}`);
