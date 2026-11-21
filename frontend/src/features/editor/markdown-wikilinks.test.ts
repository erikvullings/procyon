import { describe, expect, it } from 'vitest';
import { safeMarkdownHtml } from './markdown-preview';
import { wikilinksToHtml } from './markdown-wikilinks';

describe('wikilinksToHtml', () => {
  it('turns name, heading, alias and embed forms into anchors', () => {
    expect(wikilinksToHtml('See [[My Note]].')).toBe(
      'See <a href="#" class="fm-md-wikilink" data-fm-wiki="My Note">My Note</a>.',
    );
    expect(wikilinksToHtml('[[Note#Setup|the setup]]')).toContain(
      'data-fm-wiki="Note#Setup">the setup</a>',
    );
    expect(wikilinksToHtml('[[Note#Setup]]')).toContain('>Note › Setup</a>');
    expect(wikilinksToHtml('![[diagram.png]]')).toContain('data-fm-wiki="diagram.png"');
  });

  it('escapes the target and label', () => {
    expect(wikilinksToHtml('[[a "b" <c>]]')).toContain(
      'data-fm-wiki="a &quot;b&quot; &lt;c&gt;">a &quot;b&quot; &lt;c&gt;</a>',
    );
  });

  it('leaves code, fenced blocks and shell tests alone', () => {
    const markdown = [
      'Use `[[inline]]` here, [[real]] there.',
      '```bash',
      'if [[ -f x ]]; then [[note]]; fi',
      '```',
      'Plain [[ -d dir ]] test.',
    ].join('\n');
    const out = wikilinksToHtml(markdown).split('\n');
    expect(out[0]).toBe(
      'Use `[[inline]]` here, <a href="#" class="fm-md-wikilink" data-fm-wiki="real">real</a> there.',
    );
    expect(out.slice(1)).toEqual(markdown.split('\n').slice(1));
  });

  it('survives sanitising in the rendered preview', () => {
    expect(safeMarkdownHtml('Go to [[Runbook]]')).toContain(
      '<a href="#" class="fm-md-wikilink" data-fm-wiki="Runbook">Runbook</a>',
    );
  });
});
