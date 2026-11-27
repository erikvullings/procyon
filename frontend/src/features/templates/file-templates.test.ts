import { describe, expect, it } from 'vitest';
import {
  appTemplateEntries,
  DEFAULT_FILE_TEMPLATES,
  enabledTemplates,
  expandTemplateBody,
  newTemplateId,
} from './file-templates';

describe('file templates', () => {
  it('fills {date} and {name}, leaving other text alone', () => {
    const now = new Date(2026, 10, 8);
    expect(expandTemplateBody('# {name}\n{date}\n{name}.{date} {other}', 'Notes.v2.md', now)).toBe(
      '# Notes.v2\n2026-11-08\nNotes.v2.2026-11-08 {other}',
    );
    expect(expandTemplateBody('plain', 'x.txt', now)).toBe('plain');
    expect(expandTemplateBody('{name}', '.env', now)).toBe('.env');
  });

  it('offers only enabled templates, in order', () => {
    const templates = DEFAULT_FILE_TEMPLATES.map((template, index) => ({
      ...template,
      enabled: index !== 1,
    }));
    expect(enabledTemplates(templates).map((template) => template.id)).not.toContain('markdown');
    expect(enabledTemplates(templates)[0]?.id).toBe('text');
  });

  it('lists enabled settings templates as app entries', () => {
    const templates = DEFAULT_FILE_TEMPLATES.map((template, index) => ({
      ...template,
      enabled: index !== 0,
    }));
    const entries = appTemplateEntries(templates);
    expect(entries).toHaveLength(templates.length - 1);
    expect(entries.every((entry) => entry.source === 'app' && entry.sourceName === null)).toBe(
      true,
    );
    expect(entries[0]?.id).toBe('markdown');
    expect(entries[0]?.openInEditor).toBe(true);
  });

  it('bundles an empty file template that stays closed in the editor', () => {
    const empty = DEFAULT_FILE_TEMPLATES.find((template) => template.id === 'empty');
    expect(empty).toMatchObject({ body: '', openInEditor: false });
    const markdown = DEFAULT_FILE_TEMPLATES.find((template) => template.id === 'markdown');
    expect(markdown?.openInEditor).toBe(true);
    expect(expandTemplateBody(markdown?.body ?? '', 'Idea.md', new Date(2026, 10, 27))).toBe(
      '---\ntitle: Idea\ndate: 2026-11-27\ntags: []\n---\n\n# Idea\n\n',
    );
  });

  it('mints unused ids', () => {
    expect(newTemplateId([])).toBe('custom-1');
    const taken = {
      ...DEFAULT_FILE_TEMPLATES[0],
      id: 'custom-2',
    } as (typeof DEFAULT_FILE_TEMPLATES)[number];
    expect(newTemplateId([taken])).toBe('custom-3');
  });
});
