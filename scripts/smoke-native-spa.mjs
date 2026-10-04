import { spawn } from 'node:child_process';
import {
  closeSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.env.CI !== 'true') {
  throw new Error('the native SPA smoke test is restricted to disposable CI environments');
}

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const executable = resolve(process.argv[2] ?? '');
const bundled = process.argv.includes('--bundled');
const root = mkdtempSync(join(tmpdir(), 'procyon-native-spa-'));
const home = join(root, 'home');
const appData = join(root, 'appdata');
const localAppData = join(root, 'localappdata');
const config = join(root, 'config');
const data = join(root, 'data');
const file = join(root, 'smoke.svg');
const marker = 'data-native-spa-smoke="acl-denied-and-saved"';
const stdout = openSync(join(root, 'stdout.log'), 'w');
const stderr = openSync(join(root, 'stderr.log'), 'w');
for (const directory of [home, appData, localAppData, config, data]) {
  mkdirSync(directory, { recursive: true });
}
writeFileSync(file, '<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>');

let child;
try {
  const env = {
    ...process.env,
    HOME: home,
    APPDATA: appData,
    LOCALAPPDATA: localAppData,
    XDG_CONFIG_HOME: config,
    XDG_DATA_HOME: data,
    FM_LOG_FILE: join(root, 'app.log'),
    PROCYON_NATIVE_SPA_SMOKE_FILE: file,
    HTTP_PROXY: 'http://127.0.0.1:9',
    HTTPS_PROXY: 'http://127.0.0.1:9',
    ALL_PROXY: 'http://127.0.0.1:9',
    NO_PROXY: '127.0.0.1,localhost',
  };
  if (bundled) delete env.PROCYON_NATIVE_SPA_SMOKE_PLUGINS;
  else env.PROCYON_NATIVE_SPA_SMOKE_PLUGINS = join(repoRoot, 'plugins');
  child = spawn(executable, [], {
    env,
    stdio: ['ignore', stdout, stderr],
  });
  let spawnError;
  child.once('error', (error) => {
    spawnError = error;
  });
  const deadline = Date.now() + 90_000;
  let lastStage = 'process not started';
  let saveSucceeded = false;
  let bundledAssetsSelected = false;
  while (Date.now() < deadline) {
    if (spawnError) throw spawnError;
    const stages = readFileSync(join(root, 'stderr.log'), 'utf8').matchAll(
      /native-spa-stage: ([^\r\n]+)/g,
    );
    for (const match of stages) {
      lastStage = match[1];
      if (lastStage.startsWith('trusted-page-started: http://127.0.0.1:5181')) {
        throw new Error(
          'native SPA smoke loaded the Vite dev URL instead of embedded release assets',
        );
      }
      if (
        lastStage.startsWith('child-open-failed:') ||
        lastStage.startsWith('child-load-failed:') ||
        lastStage.startsWith('script-injection-failed:') ||
        lastStage.startsWith('script-failed') ||
        lastStage === 'bridge-save-failed'
      ) {
        throw new Error(`native SPA smoke failed at ${lastStage}`);
      }
      if (lastStage === 'bridge-save-succeeded') saveSucceeded = true;
      if (lastStage === 'bundled-plugin-assets-selected') bundledAssetsSelected = true;
    }
    if (readFileSync(file, 'utf8').includes(marker) && saveSucceeded) break;
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(`native SPA app exited early (${child.exitCode ?? child.signalCode})`);
    }
    await new Promise((done) => setTimeout(done, 250));
  }
  if (!readFileSync(file, 'utf8').includes(marker)) {
    throw new Error(`native SPA smoke timed out after ${lastStage} (90 seconds)`);
  }
  if (!saveSucceeded) {
    throw new Error(
      `native SPA saved the SVG but did not confirm bridge success after ${lastStage}`,
    );
  }
  if (bundled && !bundledAssetsSelected) {
    throw new Error('native SPA saved without selecting installed SVGO assets');
  }
  console.log(
    `Native SPA ${bundled ? 'installed-package' : 'release-binary'} activation, updater denial, and revision-checked Save passed on ${process.platform}`,
  );
} catch (error) {
  for (const name of [
    ...readdirSync(root).filter((entry) => entry.startsWith('app.log')),
    'stdout.log',
    'stderr.log',
  ]) {
    try {
      console.error(`${name}:\n${readFileSync(join(root, name), 'utf8').slice(-6000)}`);
    } catch {
      // A failing app may not have created its log.
    }
  }
  throw error;
} finally {
  if (child && child.exitCode === null && child.signalCode === null) {
    child.kill();
    await new Promise((done) => child.once('exit', done));
  }
  closeSync(stdout);
  closeSync(stderr);
  rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
}
