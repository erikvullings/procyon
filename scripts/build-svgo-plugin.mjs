import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const source = join(root, 'plugin-sources', 'svgo');
const output = join(source, 'dist', 'procyon');
const destination = join(root, 'plugins', 'svgo', 'dist');

function run(args) {
  const result = spawnSync('pnpm', args, {
    cwd: source,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`SVGO plugin ${args.join(' ')} failed (exit ${result.status})`);
  }
}

run(['install', '--frozen-lockfile']);
run(['run', 'build:procyon']);

const index = join(output, 'index.html');
if (!existsSync(index)) throw new Error(`SVGO plugin build did not produce ${index}`);
const html = readFileSync(index, 'utf8');
const entry = html.match(/src="\.\/(assets\/[^"]+\.js)"/)?.[1];
if (!entry || !existsSync(join(output, entry))) {
  throw new Error('SVGO plugin build did not produce its JavaScript entrypoint');
}

rmSync(destination, { recursive: true, force: true });
cpSync(output, destination, { recursive: true });
