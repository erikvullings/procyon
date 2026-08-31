import { describe, expect, it } from 'vitest';
import { sanitizeSvgMarkup } from './svg-sanitizer';

describe('sanitizeSvgMarkup', () => {
  it('keeps an allow-listed path with allow-listed attributes', () => {
    const result = sanitizeSvgMarkup(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="none" stroke="#fff" d="M1 1h2v2z" /></svg>',
    );
    expect(result.viewBox).toBe('0 0 16 16');
    expect(result.innerMarkup).toContain('<path');
    expect(result.innerMarkup).toContain('stroke="#fff"');
    expect(result.innerMarkup).toContain('d="M1 1h2v2z"');
  });

  it('strips <script> elements anywhere in the tree', () => {
    const result = sanitizeSvgMarkup(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g><script>alert(1)</script><path d="M0 0" /></g></svg>',
    );
    expect(result.innerMarkup).not.toContain('script');
    expect(result.innerMarkup).not.toContain('alert');
    expect(result.innerMarkup).toContain('<path');
  });

  it('strips <foreignObject> elements', () => {
    const result = sanitizeSvgMarkup(
      '<svg xmlns="http://www.w3.org/2000/svg"><foreignObject><div>hi</div></foreignObject><path d="M0 0" /></svg>',
    );
    expect(result.innerMarkup).not.toContain('foreignObject');
    expect(result.innerMarkup).toContain('<path');
  });

  it('strips event-handler attributes', () => {
    const result = sanitizeSvgMarkup(
      '<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" onclick="alert(1)" onload="alert(2)" /></svg>',
    );
    expect(result.innerMarkup).not.toContain('onclick');
    expect(result.innerMarkup).not.toContain('onload');
    expect(result.innerMarkup).toContain('d="M0 0"');
  });

  it('strips disallowed attributes such as href', () => {
    const result = sanitizeSvgMarkup(
      '<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0" href="javascript:alert(1)" /></svg>',
    );
    expect(result.innerMarkup).not.toContain('href');
  });

  it('strips disallowed elements such as <use>', () => {
    const result = sanitizeSvgMarkup(
      '<svg xmlns="http://www.w3.org/2000/svg"><use href="#evil" /><path d="M0 0" /></svg>',
    );
    expect(result.innerMarkup).not.toContain('<use');
    expect(result.innerMarkup).toContain('<path');
  });

  it('returns a blank result for unparsable markup', () => {
    const result = sanitizeSvgMarkup('<svg><path d="M0 0"</svg>');
    expect(result.innerMarkup).toBe('');
  });

  it('returns a blank result when the root element is not <svg>', () => {
    const result = sanitizeSvgMarkup('<div>not an svg</div>');
    expect(result.innerMarkup).toBe('');
  });
});
