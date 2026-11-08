import { describe, expect, it } from 'vitest';
import {
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

  it('mints unused ids', () => {
    expect(newTemplateId([])).toBe('custom-1');
    const taken = {
      ...DEFAULT_FILE_TEMPLATES[0],
      id: 'custom-2',
    } as (typeof DEFAULT_FILE_TEMPLATES)[number];
    expect(newTemplateId([taken])).toBe('custom-3');
  });
});
