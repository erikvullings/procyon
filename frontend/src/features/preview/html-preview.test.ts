import { describe, expect, it } from 'vitest';
import { isHtmlPreviewExtension, sandboxedHtmlDocument } from './html-preview';

describe('sandboxedHtmlDocument', () => {
  it('puts the policy before the file content and blocks scripts and network loads', () => {
    const doc = sandboxedHtmlDocument('<h1>Hi</h1><script src="https://x/y.js"></script>');
    expect(doc.startsWith('<meta http-equiv="Content-Security-Policy"')).toBe(true);
    expect(doc).toContain("default-src 'none'");
    expect(doc).not.toMatch(/script-src/);
    expect(doc.endsWith('</script>')).toBe(true);
  });
});

describe('isHtmlPreviewExtension', () => {
  it('matches html and htm in any case only', () => {
    expect(isHtmlPreviewExtension('HTML')).toBe(true);
    expect(isHtmlPreviewExtension('htm')).toBe(true);
    expect(isHtmlPreviewExtension('xml')).toBe(false);
    expect(isHtmlPreviewExtension(undefined)).toBe(false);
  });
});
