import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import {
  attachedChars,
  fileContentAttachment,
  folderListingAttachment,
  formatBytes,
  MAX_LISTED_ENTRIES,
  selectionAttachment,
} from './chat-context';

const file = (name: string, size: number, modifiedAt?: string): EntrySummary =>
  ({ name, kind: 'file', size, modifiedAt }) as EntrySummary;
const folder = (name: string): EntrySummary => ({ name, kind: 'directory' }) as EntrySummary;

describe('formatBytes', () => {
  it('uses binary units', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(4 * 1024 ** 3)).toBe('4.0 GB');
  });
});

describe('folder listing attachment', () => {
  it('lists names, kinds, sizes and dates, never contents', () => {
    const attachment = folderListingAttachment('a1', '/notes', [
      folder('archive'),
      file('todo.md', 1536, '2026-11-01T10:00:00Z'),
    ]);
    expect(attachment.label).toBe('Folder listing: /notes');
    expect(attachment.text).toBe(
      'Folder listing of /notes (2 entries):\n- archive/ (folder)\n- todo.md (1.5 KB, modified 2026-11-01)',
    );
  });

  it('counts the entries it does not list', () => {
    const entries = Array.from({ length: MAX_LISTED_ENTRIES + 3 }, (_, index) =>
      file(`f${index}.txt`, 1),
    );
    const { text } = folderListingAttachment('a1', '/big', entries);
    // Header, the listed entries, and the line that counts the rest.
    expect(text.split('\n')).toHaveLength(MAX_LISTED_ENTRIES + 2);
    expect(text.endsWith('- … and 3 more')).toBe(true);
  });
});

describe('selection attachment', () => {
  it('names the selection and its details', () => {
    const attachment = selectionAttachment('s1', '/notes', [file('a.txt', 10)]);
    expect(attachment.label).toBe('Selection: 1 in /notes');
    expect(attachment.text).toBe('Selected entries in /notes (1):\n- a.txt (10 B)');
  });
});

describe('file contents attachment', () => {
  it('carries the exact file text, labelled with its name and size', () => {
    const attachment = fileContentAttachment('f1', 'todo.md', 1536, '- buy milk\n');
    expect(attachment.label).toBe('File: todo.md');
    expect(attachment.text).toBe('Contents of todo.md (1.5 KB):\n- buy milk\n');
    expect(attachedChars([attachment, attachment])).toBe(attachment.text.length * 2);
  });
});
