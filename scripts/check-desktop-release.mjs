import { execFileSync } from 'node:child_process';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const tag = process.argv[2];
if (!tag) throw new Error('usage: node scripts/check-desktop-release.mjs v<version>');

const metadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], {
    cwd: repoRoot,
    encoding: 'utf8',
  }),
);
const desktopPackage = metadata.packages.find(({ name }) => name === 'fm-desktop');
if (!desktopPackage) throw new Error('fm-desktop is absent from cargo metadata');

const expectedTag = `v${desktopPackage.version}`;
if (tag !== expectedTag) {
  throw new Error(`release tag ${tag} does not match desktop crate version ${expectedTag}`);
}
