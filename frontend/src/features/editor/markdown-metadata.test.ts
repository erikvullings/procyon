import { describe, expect, it } from 'vitest';
import { metadataTableHtml, splitFrontMatter, splitHeaderFields } from './markdown-metadata';
import { safeMarkdownHtml } from './markdown-preview';

const NOTE = `---
title: "Replace nginx cert"
created: 2026-11-21
tags: [nginx, tls, "a, b"]
hosts:
  - web-01
  - 'web-02'
---
# Replace nginx cert

Body`;

const TASK = `# 0337 Notes

Status: in_progress
Priority: medium
Depends on: 0336

## Context
Why.`;

describe('splitFrontMatter', () => {
  it('reads scalars, flow lists with quoted commas and block lists', () => {
    const split = splitFrontMatter(NOTE);
    expect(split?.fields).toEqual([
      { key: 'title', values: ['Replace nginx cert'], list: false },
      { key: 'created', values: ['2026-11-21'], list: false },
      { key: 'tags', values: ['nginx', 'tls', 'a, b'], list: true },
      { key: 'hosts', values: ['web-01', 'web-02'], list: true },
    ]);
    expect(split?.after.trim()).toBe('# Replace nginx cert\n\nBody');
  });

  it('ignores a leading rule that is not closed or is not YAML', () => {
    expect(splitFrontMatter('---\nno close\n')).toBeUndefined();
    expect(splitFrontMatter('---\njust a sentence here\n---\n')).toBeUndefined();
    expect(splitFrontMatter('# Title\n---\na: b\n---')).toBeUndefined();
  });
});

describe('splitFrontMatter for SKILL.md files', () => {
  it('reads folded and literal block scalars, nested maps and unindented lists', () => {
    const skill = [
      '---',
      'name: mem-setup',
      'description: >-',
      '  Use this when the user asks',
      '  to "set up".',
      '',
      '  Second paragraph.',
      'notes: |',
      '  line one',
      '  line two',
      'allowed-tools:',
      '- Bash',
      '- Read',
      'metadata:',
      '  version: "0.1.0"',
      '  owner:',
      '    team: docs',
      'summary:',
      '  "Starts on the next line',
      '  and continues."',
      '---',
      '# Body',
    ].join('\n');
    expect(splitFrontMatter(skill)?.fields).toEqual([
      { key: 'name', values: ['mem-setup'], list: false },
      {
        key: 'description',
        values: ['Use this when the user asks to "set up".\nSecond paragraph.'],
        list: false,
      },
      { key: 'notes', values: ['line one\nline two'], list: false, multiline: true },
      { key: 'allowed-tools', values: ['Bash', 'Read'], list: true },
      {
        key: 'metadata',
        values: [],
        list: false,
        children: [
          { key: 'version', values: ['0.1.0'], list: false },
          {
            key: 'owner',
            values: [],
            list: false,
            children: [{ key: 'team', values: ['docs'], list: false }],
          },
        ],
      },
      { key: 'summary', values: ['Starts on the next line and continues.'], list: false },
    ]);
  });

  it('renders nested maps as an inner table and keeps literal line breaks', () => {
    const html = safeMarkdownHtml('---\nnotes: |\n  a\n  b\nmetadata:\n  version: 1\n---\nx');
    expect(html).toContain('a<br>b');
    expect(html).toContain('<table class="fm-md-meta fm-md-meta-nested">');
  });
});

describe('splitHeaderFields', () => {
  it('takes the Key: value paragraph after the title', () => {
    const split = splitHeaderFields(TASK);
    expect(split?.before).toBe('# 0337 Notes');
    expect(split?.fields.map((field) => field.key)).toEqual(['Status', 'Priority', 'Depends on']);
    expect(split?.after.trim()).toBe('## Context\nWhy.');
  });

  it('leaves prose, single lines and documents without a title alone', () => {
    expect(splitHeaderFields('# T\n\nNote: one line only')).toBeUndefined();
    expect(splitHeaderFields('# T\n\nStatus: done\nand then prose')).toBeUndefined();
    expect(splitHeaderFields('Status: done\nPriority: high')).toBeUndefined();
  });
});

describe('metadataTableHtml', () => {
  it('escapes keys and values and badges the status', () => {
    const html = metadataTableHtml([
      { key: 'Status', values: ['in_progress'], list: false },
      { key: 'x<y', values: ['<b>'], list: false },
    ]);
    expect(html).toContain('fm-md-meta-status-in-progress');
    expect(html).toContain('x&lt;y');
    expect(html).toContain('&lt;b&gt;');
  });
});

describe('safeMarkdownHtml with metadata', () => {
  it('renders front matter as a table before the body', () => {
    const html = safeMarkdownHtml(NOTE);
    expect(html.indexOf('<table class="fm-md-meta">')).toBeLessThan(html.indexOf('<h1>'));
    expect(html).toContain('<span class="fm-md-meta-chip">tls</span>');
    expect(html).not.toContain('<hr');
  });

  it('renders a task header as a table under the title', () => {
    const html = safeMarkdownHtml(TASK);
    expect(html.indexOf('<h1>0337 Notes</h1>')).toBeLessThan(html.indexOf('fm-md-meta'));
    expect(html).toContain(
      '<th scope="row">Depends on</th><td><a href="#" class="fm-md-task-link" data-fm-task="0336">0336</a></td>',
    );
    expect(html).toContain('<h2>Context</h2>');
  });
});
