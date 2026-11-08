import type { FileTemplate } from '../../models';

/** The templates the backend ships until the user edits the list (mirrors `fm-settings`). */
export const DEFAULT_FILE_TEMPLATES: readonly FileTemplate[] = [
  { id: 'text', label: 'Text file', fileName: 'New file.txt', body: '', enabled: true },
  {
    id: 'markdown',
    label: 'Markdown note',
    fileName: 'Note.md',
    body: '# {name}\n\n{date}\n\n',
    enabled: true,
  },
  {
    id: 'todo',
    label: 'To-do list',
    fileName: 'TODO.md',
    body: '# To do\n\n- [ ] \n- [ ] \n- [ ] \n',
    enabled: true,
  },
  {
    id: 'shell',
    label: 'Shell script',
    fileName: 'script.sh',
    body: '#!/usr/bin/env bash\nset -euo pipefail\n\n',
    enabled: true,
  },
  {
    id: 'html',
    label: 'HTML page',
    fileName: 'index.html',
    body: '<!doctype html>\n<html lang="en">\n<head>\n  <meta charset="utf-8">\n  <title>{name}</title>\n</head>\n<body>\n\n</body>\n</html>\n',
    enabled: true,
  },
  { id: 'json', label: 'JSON document', fileName: 'data.json', body: '{\n}\n', enabled: true },
];

/** The templates to offer: enabled ones, in their configured order. */
export function enabledTemplates(templates: readonly FileTemplate[]): readonly FileTemplate[] {
  return templates.filter((template) => template.enabled);
}

function isoDate(now: Date): string {
  const pad = (value: number): string => String(value).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** A file name without its last extension (dotfiles keep their whole name). */
function stem(fileName: string): string {
  const dot = fileName.lastIndexOf('.');
  return dot > 0 ? fileName.slice(0, dot) : fileName;
}

/**
 * Fills the optional placeholders of a template body: `{date}` (today, ISO) and `{name}` (the new
 * file's name without its extension). Text without placeholders is returned unchanged.
 */
export function expandTemplateBody(body: string, fileName: string, now = new Date()): string {
  return body.replaceAll('{date}', isoDate(now)).replaceAll('{name}', stem(fileName));
}

/** A new id for a user-added template, unique within `templates`. */
export function newTemplateId(templates: readonly FileTemplate[]): string {
  const taken = new Set(templates.map((template) => template.id));
  for (let index = templates.length + 1; ; index += 1) {
    const id = `custom-${index}`;
    if (!taken.has(id)) return id;
  }
}
