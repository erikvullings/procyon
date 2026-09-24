import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function filesBelow(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? filesBelow(path) : [path];
  });
}

function oneArtifact(files, pattern, label) {
  const matches = files.filter((path) => pattern.test(basename(path)));
  if (matches.length !== 1) {
    throw new Error(`Expected one ${label} artifact, found ${matches.length}.`);
  }
  return matches[0];
}

function signatureFor(artifact) {
  const signature = readFileSync(`${artifact}.sig`, 'utf8').trim();
  if (signature.length === 0) throw new Error(`Updater signature is empty: ${artifact}.sig`);
  return signature;
}

function releaseUrl(repository, tag, artifact) {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(basename(artifact))}`;
}

export function generateUpdaterManifest({ assetsDirectory, repository, tag, version }) {
  const files = filesBelow(assetsDirectory);
  const macos = oneArtifact(files, /\.app\.tar\.gz$/, 'macOS updater');
  const linux = oneArtifact(files, /\.AppImage$/, 'Linux updater');
  const windows = oneArtifact(files, /-setup\.exe$/, 'Windows NSIS updater');
  const platform = (artifact) => ({
    url: releaseUrl(repository, tag, artifact),
    signature: signatureFor(artifact),
  });
  const macosPlatform = platform(macos);
  return {
    version,
    platforms: {
      'darwin-aarch64': macosPlatform,
      'darwin-x86_64': macosPlatform,
      'linux-x86_64': platform(linux),
      'windows-x86_64': platform(windows),
    },
  };
}

function argument(name) {
  const index = process.argv.indexOf(name);
  const value = index < 0 ? undefined : process.argv[index + 1];
  if (value === undefined) throw new Error(`Missing ${name}.`);
  return value;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const output = resolve(argument('--output'));
  const manifest = generateUpdaterManifest({
    assetsDirectory: resolve(argument('--assets')),
    repository: argument('--repository'),
    tag: argument('--tag'),
    version: argument('--version'),
  });
  writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
}
