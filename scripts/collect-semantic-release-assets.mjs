import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

function digest(file) {
  return createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

export function collectSemanticReleaseAssets(payloadRoot, catalogRoot, output) {
  fs.rmSync(output, { recursive: true, force: true });
  fs.mkdirSync(output, { recursive: true });
  const payloadDirectories = fs
    .readdirSync(payloadRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .sort((left, right) => left.name.localeCompare(right.name));
  const seen = new Map();
  for (const directory of payloadDirectories) {
    const payload = path.join(payloadRoot, directory.name);
    const artifacts = path.join(payload, 'artifacts');
    for (const name of fs.readdirSync(artifacts).sort()) {
      const source = path.join(artifacts, name);
      const checksum = digest(source);
      const previous = seen.get(name);
      if (previous && previous !== checksum) {
        throw new Error(`semantic release payload ${name} has conflicting bytes`);
      }
      if (!previous) {
        fs.copyFileSync(source, path.join(output, name));
        seen.set(name, checksum);
      }
    }
    const target = directory.name.replace(/^semantic-payloads-/u, '');
    fs.copyFileSync(
      path.join(payload, 'zvec-runtime-qualification.json'),
      path.join(output, `zvec-runtime-qualification-${target}.json`),
    );
    const onnxQualification = path.join(payload, 'onnx-runtime-qualification.json');
    if (fs.existsSync(onnxQualification)) {
      fs.copyFileSync(
        onnxQualification,
        path.join(output, `onnx-runtime-qualification-${target}.json`),
      );
    }
  }

  const catalogDirectories = fs
    .readdirSync(catalogRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.startsWith('semantic-catalog-'))
    .sort((left, right) => left.name.localeCompare(right.name));
  for (const directory of catalogDirectories) {
    const target = directory.name.slice('semantic-catalog-'.length);
    for (const [sourceName, extension] of [
      ['catalog.json', 'json'],
      ['catalog.sig', 'sig'],
    ]) {
      fs.copyFileSync(
        path.join(catalogRoot, directory.name, sourceName),
        path.join(output, `semantic-catalog-${target}.${extension}`),
      );
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , payloadRoot, catalogRoot, output] = process.argv;
  if (!payloadRoot || !catalogRoot || !output) {
    throw new Error(
      'Usage: collect-semantic-release-assets.mjs <payload-root> <catalog-root> <output>',
    );
  }
  collectSemanticReleaseAssets(payloadRoot, catalogRoot, output);
}
