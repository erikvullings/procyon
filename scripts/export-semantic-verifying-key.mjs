import { appendFileSync } from 'node:fs';

const [environmentFile] = process.argv.slice(2);
if (!environmentFile) {
  throw new Error('usage: export-semantic-verifying-key.mjs <github-env-file>');
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
