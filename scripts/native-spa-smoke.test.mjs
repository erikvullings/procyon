import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

function stopFixtureWorker(pid) {
  if (!pid) return;
  try {
    process.kill(pid, 'SIGKILL');
  } catch (error) {
    if (error.code !== 'ESRCH') throw error;
  }
}

test('native SPA smoke stops cache-writing descendants before removing its isolated root', async () => {
  if (process.platform === 'win32') return;
  const fixture = mkdtempSync(join(tmpdir(), 'procyon-smoke-fixture-'));
  const app = join(fixture, 'app');
  const worker = join(fixture, 'worker.cjs');
  const preload = join(fixture, 'preload.cjs');
  const workerPidFile = join(fixture, 'worker.pid');
  const rootFile = join(fixture, 'root');
  let workerPid;
  try {
    writeFileSync(
      worker,
      `const fs = require('node:fs');
const path = require('node:path');
const cache = path.join(process.env.HOME, '.cache', 'mesa_shader_cache');
fs.writeFileSync(process.env.TEST_WORKER_PID_FILE, String(process.pid));
fs.writeFileSync(process.env.TEST_ROOT_FILE, path.dirname(process.env.HOME));
setInterval(() => {
  fs.mkdirSync(cache, { recursive: true });
  fs.writeFileSync(path.join(cache, 'shader'), 'active');
}, 1);
`,
    );
    writeFileSync(
      app,
      `#!${process.execPath}
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
spawn(process.execPath, [${JSON.stringify(worker)}], { stdio: 'ignore' });
const file = process.env.PROCYON_NATIVE_SPA_SMOKE_FILE;
const ready = setInterval(() => {
  if (!fs.existsSync(process.env.TEST_WORKER_PID_FILE)) return;
  clearInterval(ready);
  fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace('<rect ', '<rect data-native-spa-smoke="acl-denied-and-saved" '));
  if (!process.env.PROCYON_NATIVE_SPA_SMOKE_PLUGINS)
    console.error('native-spa-stage: bundled-plugin-assets-selected');
  console.error('native-spa-stage: bridge-save-succeeded');
  console.error('native-spa-stage: heartbeat-and-disable-teardown-succeeded');
}, 5);
setInterval(() => {}, 1000);
`,
    );
    chmodSync(app, 0o755);
    writeFileSync(
      preload,
      `const fs = require('node:fs');
const { syncBuiltinESMExports } = require('node:module');
const original = fs.rmSync;
fs.rmSync = (path, options) => {
  if (path.includes('procyon-native-spa-') &&
      fs.existsSync(${JSON.stringify(workerPidFile)})) {
    const pid = Number(fs.readFileSync(${JSON.stringify(workerPidFile)}, 'utf8'));
    try {
      process.kill(pid, 0);
      const zombie = process.platform === 'linux' &&
        fs.readFileSync('/proc/' + pid + '/stat', 'utf8').split(' ')[2] === 'Z';
      if (!zombie) {
        const error = new Error("ENOTEMPTY: directory not empty, rmdir '" +
          require('node:path').join(path, 'home', '.cache', 'mesa_shader_cache') + "'");
        error.code = 'ENOTEMPTY';
        throw error;
      }
    } catch (error) {
      if (error.code !== 'ESRCH' && error.code !== 'ENOENT') throw error;
    }
  }
  return original(path, options);
};
syncBuiltinESMExports();
`,
    );
    for (const bundled of [false, true]) {
      const result = spawnSync(
        process.execPath,
        ['scripts/smoke-native-spa.mjs', app, ...(bundled ? ['--bundled'] : [])],
        {
          cwd: fileURLToPath(new URL('..', import.meta.url)),
          env: {
            ...process.env,
            CI: 'true',
            TEST_WORKER_PID_FILE: workerPidFile,
            TEST_ROOT_FILE: rootFile,
            NODE_OPTIONS: `--require=${preload}`,
          },
          encoding: 'utf8',
          timeout: 15_000,
        },
      );
      if (existsSync(workerPidFile)) workerPid = Number(readFileSync(workerPidFile, 'utf8'));
      assert.equal(result.status, 0, result.stderr);
      assert.match(
        result.stdout,
        bundled ? /installed-package activation/ : /release-binary activation/,
      );
      await new Promise((done) => setTimeout(done, 75));
      assert.equal(existsSync(readFileSync(rootFile, 'utf8')), false);
      rmSync(workerPidFile, { force: true });
    }
  } finally {
    stopFixtureWorker(workerPid);
    if (existsSync(rootFile)) {
      rmSync(readFileSync(rootFile, 'utf8'), { recursive: true, force: true });
    }
    rmSync(fixture, { recursive: true, force: true });
  }
});

test('native SPA smoke reports both an app failure and a cleanup failure', () => {
  if (process.platform === 'win32') return;
  const fixture = mkdtempSync(join(tmpdir(), 'procyon-smoke-failures-'));
  const app = join(fixture, 'app');
  const preload = join(fixture, 'preload.cjs');
  const rootFile = join(fixture, 'root');
  try {
    writeFileSync(
      app,
      `#!${process.execPath}
console.error('native-spa-stage: bridge-save-failed');
setInterval(() => {}, 1000);
`,
    );
    chmodSync(app, 0o755);
    writeFileSync(
      preload,
      `const fs = require('node:fs');
const { syncBuiltinESMExports } = require('node:module');
const original = fs.rmSync;
fs.rmSync = (path, options) => {
  if (path.includes('procyon-native-spa-')) {
    fs.writeFileSync(${JSON.stringify(rootFile)}, path);
    const error = new Error('injected cleanup failure');
    error.code = 'EACCES';
    throw error;
  }
  return original(path, options);
};
syncBuiltinESMExports();
`,
    );
    const result = spawnSync(process.execPath, ['scripts/smoke-native-spa.mjs', app], {
      cwd: fileURLToPath(new URL('..', import.meta.url)),
      env: { ...process.env, CI: 'true', NODE_OPTIONS: `--require=${preload}` },
      encoding: 'utf8',
      timeout: 15_000,
    });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /bridge-save-failed/);
    assert.match(result.stderr, /injected cleanup failure/);
    assert.match(result.stderr, /native SPA smoke and cleanup failed/);
  } finally {
    if (existsSync(rootFile))
      rmSync(readFileSync(rootFile, 'utf8'), { recursive: true, force: true });
    rmSync(fixture, { recursive: true, force: true });
  }
});
