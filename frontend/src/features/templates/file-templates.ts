import type { FileTemplate, FileTemplateEntry } from '../../models';
import defaultIndex from './defaults/index.json' with { type: 'json' };

/** Every bundled body, keyed by its path (`./defaults/<id>.tmpl`), as raw text. */
const defaultBodies = import.meta.glob<string>('./defaults/*.tmpl', {
  query: '?raw',
  import: 'default',
  eager: true,
});

function defaultBody(id: string): string {
  const body = defaultBodies[`./defaults/${id}.tmpl`];
  if (body === undefined) throw new Error(`Missing default template body for "${id}"`);
  return body;
}

/**
 * The templates the backend ships until the user edits the list. `fm-settings` bundles the same
 * `defaults/` files, so the two cannot drift.
 */
export const DEFAULT_FILE_TEMPLATES: readonly FileTemplate[] = defaultIndex.map((entry) => ({
  ...entry,
  body: defaultBody(entry.id),
  enabled: true,
}));

/** The templates to offer: enabled ones, in their configured order. */
export function enabledTemplates(templates: readonly FileTemplate[]): readonly FileTemplate[] {
  return templates.filter((template) => template.enabled);
}

/** The enabled settings templates as catalogue entries from the app source. */
export function appTemplateEntries(templates: readonly FileTemplate[]): FileTemplateEntry[] {
  return enabledTemplates(templates).map((template) => ({
    id: template.id,
    label: template.label,
    fileName: template.fileName,
    body: template.body,
    source: 'app',
    sourceName: null,
  }));
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
