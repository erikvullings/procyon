import { spawn } from 'node:child_process';
import {
  closeSync,
  mkdirSync,
  mkdtempSync,
  openSync,
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
  child = spawn(executable, [], {
    env: {
      ...process.env,
      HOME: home,
      APPDATA: appData,
      LOCALAPPDATA: localAppData,
      XDG_CONFIG_HOME: config,
      XDG_DATA_HOME: data,
      FM_LOG_FILE: join(root, 'app.log'),
      PROCYON_NATIVE_SPA_SMOKE_FILE: file,
      PROCYON_NATIVE_SPA_SMOKE_PLUGINS: join(repoRoot, 'plugins'),
      HTTP_PROXY: 'http://127.0.0.1:9',
      HTTPS_PROXY: 'http://127.0.0.1:9',
      ALL_PROXY: 'http://127.0.0.1:9',
      NO_PROXY: '127.0.0.1,localhost',
    },
    stdio: ['ignore', stdout, stderr],
  });
  let spawnError;
  child.once('error', (error) => {
    spawnError = error;
  });
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    if (spawnError) throw spawnError;
    if (readFileSync(file, 'utf8').includes(marker)) break;
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(`native SPA app exited early (${child.exitCode ?? child.signalCode})`);
    }
    await new Promise((done) => setTimeout(done, 250));
  }
  if (!readFileSync(file, 'utf8').includes(marker)) {
    throw new Error('native child did not deny updater and save the SVG within 90 seconds');
  }
  console.log(
    `Native SPA activation, updater denial, and revision-checked Save passed on ${process.platform}`,
  );
} catch (error) {
  for (const name of ['app.log', 'stdout.log', 'stderr.log']) {
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
  rmSync(root, { recursive: true, force: true });
}
