import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const tauriRoot = join(repoRoot, 'apps', 'fm-desktop', 'src-tauri');

/** @typedef {{ name: string, version: string, metadata: { desktop?: DesktopMetadata } }} CargoPackage */
/** @typedef {{ packages: CargoPackage[] }} CargoMetadata */
/** @typedef {{ 'product-name': string, identifier: string, icons: string[] }} DesktopMetadata */

function fail(message) {
  throw new Error(`desktop packaging configuration: ${message}`);
}

/** @returns {CargoMetadata} */
function cargoMetadata() {
  const result = spawnSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    fail(result.stderr.trim() || 'cargo metadata failed');
  }
  return JSON.parse(result.stdout);
}

/** @returns {Record<string, unknown>} */
function desktopConfig() {
  const desktopPackage = cargoMetadata().packages.find(({ name }) => name === 'fm-desktop');
  if (!desktopPackage) fail('fm-desktop is absent from cargo metadata');

  const metadata = desktopPackage.metadata.desktop;
  if (!metadata) fail('package.metadata.desktop is absent');
  if (!metadata['product-name']) fail('product-name is empty');
  if (!metadata.identifier) fail('identifier is empty');
  if (!Array.isArray(metadata.icons) || metadata.icons.length === 0) fail('icons is empty');

  const windowsThumbprint = process.env.FM_WINDOWS_CERTIFICATE_THUMBPRINT?.trim();
  return {
    productName: metadata['product-name'],
    version: desktopPackage.version,
    identifier: metadata.identifier,
    bundle: {
      icon: metadata.icons,
      ...(windowsThumbprint
        ? {
            windows: {
              certificateThumbprint: windowsThumbprint,
              digestAlgorithm: 'sha256',
              timestampUrl: 'http://timestamp.digicert.com',
            },
          }
        : {}),
    },
  };
}

const config = desktopConfig();
if (process.argv.includes('--print-config')) {
  process.stdout.write(`${JSON.stringify(config, null, 2)}\n`);
} else {
  const generatedDir = join(repoRoot, 'target', '.tauri');
  const generatedConfig = join(generatedDir, 'desktop-config.json');
  mkdirSync(generatedDir, { recursive: true });
  writeFileSync(generatedConfig, `${JSON.stringify(config, null, 2)}\n`);

  const extraArgs = process.argv.slice(2);
  const result = spawnSync(
    'pnpm',
    ['exec', 'tauri', 'build', '--config', generatedConfig, ...extraArgs],
    { cwd: tauriRoot, stdio: 'inherit', shell: process.platform === 'win32' },
  );
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}
