// Verifies the root dev scripts fail loudly (never silently no-op) while
// their underlying features are not implemented yet, per TASKS/0003.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

/** On Windows `bash` on PATH is usually WSL, a different machine with none of
 * this toolchain installed, so use the Git Bash shipped alongside git itself. */
function bashCommand() {
  if (process.platform !== 'win32') {
    return 'bash';
  }
  const gitExecPath = execFileSync('git', ['--exec-path'], { encoding: 'utf8' }).trim();
  return join(gitExecPath, '..', '..', '..', 'bin', 'bash.exe');
}

/** Runs one of the Node scripts the root package.json exposes. */
function runNodeScript(scriptName, args = []) {
  const result = spawnSync(process.execPath, [join('scripts', scriptName), ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  return {
    exitCode: result.status ?? 1,
    stdout: result.stdout ?? '',
    stderr: result.stderr ?? '',
  };
}

/** Maps every file under `dir` to its contents, for before/after diffing. */
function snapshotDir(dir) {
  const entries = readdirSync(dir, {
    recursive: true,
    encoding: 'utf8',
  }).filter((entry) => statSync(join(dir, entry)).isFile());
  return Object.fromEntries(entries.sort().map((entry) => [entry, readFileSync(join(dir, entry))]));
}

test('scripts/not-implemented.sh is executable', () => {
  // Windows has no execute bit, so assert the mode git has recorded - that is
  // what actually makes the script runnable once checked out on macOS/Linux.
  const entry = execFileSync('git', ['ls-files', '-s', 'scripts/not-implemented.sh'], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  assert.match(entry, /^100755 /, 'not-implemented.sh should be committed executable');
});

test('scripts/export-openapi.mjs writes a deterministic OpenAPI document (task 0009)', () => {
  const dir = mkdtempSync(join(tmpdir(), 'fm-export-openapi-'));
  const outputPath = join(dir, 'openapi.json');

  try {
    const first = runNodeScript('export-openapi.mjs', [outputPath]);
    assert.equal(first.exitCode, 0, first.stderr);
    const firstBytes = readFileSync(outputPath);

    const second = runNodeScript('export-openapi.mjs', [outputPath]);
    assert.equal(second.exitCode, 0, second.stderr);
    const secondBytes = readFileSync(outputPath);

    assert.deepEqual(firstBytes, secondBytes, 're-running the export must produce no diff');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('scripts/generate-api.mjs regenerates a byte-identical client (task 0010)', () => {
  const generatedDir = join(repoRoot, 'frontend', 'src', 'api', 'generated');
  const before = snapshotDir(generatedDir);

  const result = runNodeScript('generate-api.mjs');
  assert.equal(result.exitCode, 0, result.stderr);

  const after = snapshotDir(generatedDir);
  assert.deepEqual(after, before, 're-running the generator must produce no diff');
});

test('scripts/not-implemented.sh reports the script name and task number', () => {
  let stderr = '';
  let exitCode = 0;
  try {
    execFileSync(bashCommand(), ['scripts/not-implemented.sh', 'dev:tauri', '0015'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
  } catch (error) {
    exitCode = error.status;
    stderr = error.stderr;
  }
  assert.notEqual(exitCode, 0);
  assert.match(stderr, /dev:tauri/);
  assert.match(stderr, /0015/);
});

test('Tauri lifecycle commands select its transport and resolve the frontend from their working directory', () => {
  const config = JSON.parse(
    readFileSync(join(repoRoot, 'apps', 'fm-desktop', 'src-tauri', 'tauri.conf.json'), 'utf8'),
  );

  assert.equal(config.build.devUrl, 'http://127.0.0.1:5181');
  assert.match(config.build.beforeDevCommand, /VITE_RUNTIME=tauri/);
  assert.match(config.build.beforeDevCommand, /--dir \.\.\/\.\.\/frontend exec vite --port 5181$/);
  assert.match(config.build.beforeBuildCommand, /VITE_RUNTIME=tauri/);
  assert.match(config.build.beforeBuildCommand, /--dir \.\.\/\.\.\/frontend build$/);
});
