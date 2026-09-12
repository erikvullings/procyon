import { appendFileSync, writeFileSync } from 'node:fs';

const [environmentFile, publicKeyFile, signingKeyFile] = process.argv.slice(2);
if (!environmentFile) {
  throw new Error(
    'usage: export-semantic-verifying-key.mjs <github-env-file> [raw-public-key-file] [raw-signing-key-file]',
  );
}

const encodedKey = process.env.VERIFYING_KEY_BASE64;
if (!encodedKey) {
  throw new Error('SEMANTIC_CATALOG_VERIFYING_KEY_BASE64 is required');
}

const key = Buffer.from(encodedKey, 'base64');
if (
  key.length !== 32 ||
  key.toString('base64').replace(/=+$/u, '') !== encodedKey.replace(/=+$/u, '')
) {
  throw new Error('semantic catalog verifying key must be exactly 32 canonical base64 bytes');
}

appendFileSync(
  environmentFile,
  `PROCYON_SEMANTIC_CATALOG_VERIFYING_KEY_HEX=${key.toString('hex')}\n`,
);
if (publicKeyFile) writeFileSync(publicKeyFile, key, { mode: 0o644, flag: 'wx' });
if (signingKeyFile) {
  const encodedSigningKey = process.env.SIGNING_KEY_BASE64;
  if (!encodedSigningKey) {
    throw new Error('SEMANTIC_CATALOG_SIGNING_KEY_BASE64 is required');
  }
  const signingKey = Buffer.from(encodedSigningKey, 'base64');
  if (
    signingKey.length !== 32 ||
    signingKey.toString('base64').replace(/=+$/u, '') !== encodedSigningKey.replace(/=+$/u, '')
  ) {
    throw new Error('semantic catalog signing key must be exactly 32 canonical base64 bytes');
  }
  writeFileSync(signingKeyFile, signingKey, { mode: 0o600, flag: 'wx' });
}
