import { describe, expect, it } from 'vitest';
import { editableLanguageForExtension, languageExtension } from './editor-language';

describe('editableLanguageForExtension', () => {
  it.each([
    ['txt', 'text'],
    ['md', 'markdown'],
    ['markdown', 'markdown'],
    ['xml', 'xml'],
    ['html', 'xml'],
    ['json', 'json'],
    ['geojson', 'json'],
    ['toml', 'toml'],
    ['yaml', 'yaml'],
    ['yml', 'yaml'],
    ['ini', 'properties'],
    ['properties', 'properties'],
    ['sh', 'shell'],
    ['ts', 'typescript'],
    ['tsx', 'typescript'],
    ['mts', 'typescript'],
    ['js', 'javascript'],
    ['jsx', 'javascript'],
    ['mjs', 'javascript'],
    ['py', 'python'],
    ['rs', 'rust'],
    ['css', 'css'],
    ['scss', 'css'],
    ['go', 'go'],
    ['rb', 'ruby'],
    ['sql', 'sql'],
    ['java', 'clike'],
    ['c', 'clike'],
    ['cpp', 'clike'],
  ])('maps %s', (extension, expected) =>
    expect(editableLanguageForExtension(extension)).toBe(expected),
  );
  it.each([
    [undefined, '.env', 'properties'],
    [undefined, '.env.local', 'properties'],
    [undefined, '.editorconfig', 'properties'],
    [undefined, '.gitignore', 'text'],
    [undefined, 'Dockerfile', 'shell'],
  ])('detects filename %s/%s', (extension, fileName, expected) =>
    expect(editableLanguageForExtension(extension, fileName)).toBe(expected),
  );

  it('treats an unknown extension as plain text so content validation decides editability', () =>
    expect(editableLanguageForExtension('unknown')).toBe('text'));
});

describe('languageExtension', () => {
  it.each([
    'text',
    'markdown',
    'xml',
    'json',
    'toml',
    'yaml',
    'properties',
    'shell',
    'javascript',
    'typescript',
    'python',
    'rust',
    'css',
    'go',
    'ruby',
    'sql',
    'clike',
  ] as const)('builds a CodeMirror extension for %s without throwing', (language) => {
    expect(() => languageExtension(language)).not.toThrow();
  });
});
