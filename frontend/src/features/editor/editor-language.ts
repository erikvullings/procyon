import { javascript } from '@codemirror/lang-javascript';
import { json, jsonParseLinter } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import { xml } from '@codemirror/lang-xml';
import { StreamLanguage } from '@codemirror/language';
import { clike } from '@codemirror/legacy-modes/mode/clike';
import { css } from '@codemirror/legacy-modes/mode/css';
import { go } from '@codemirror/legacy-modes/mode/go';
import { properties } from '@codemirror/legacy-modes/mode/properties';
import { python } from '@codemirror/legacy-modes/mode/python';
import { ruby } from '@codemirror/legacy-modes/mode/ruby';
import { rust } from '@codemirror/legacy-modes/mode/rust';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { sql } from '@codemirror/legacy-modes/mode/sql';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { yaml } from '@codemirror/legacy-modes/mode/yaml';
import type { Extension } from '@codemirror/state';

export type EditableLanguage =
  | 'text'
  | 'markdown'
  | 'xml'
  | 'json'
  | 'toml'
  | 'yaml'
  | 'properties'
  | 'shell'
  | 'javascript'
  | 'typescript'
  | 'python'
  | 'rust'
  | 'css'
  | 'go'
  | 'ruby'
  | 'sql'
  | 'clike';

export function editableLanguageForExtension(
  extension: string | undefined,
  fileName = '',
): EditableLanguage {
  const lowerName = fileName.toLowerCase();
  if (lowerName === '.env' || lowerName.startsWith('.env.') || lowerName === '.editorconfig')
    return 'properties';
  if (lowerName === 'dockerfile') return 'shell';
  switch (extension?.toLowerCase()) {
    case 'txt':
      return 'text';
    case 'md':
    case 'markdown':
      return 'markdown';
    case 'xml':
    case 'html':
    case 'htm':
    case 'svg':
      return 'xml';
    case 'json':
    case 'geojson':
    case 'jsonc':
      return 'json';
    case 'toml':
      return 'toml';
    case 'yaml':
    case 'yml':
      return 'yaml';
    case 'ini':
    case 'cfg':
    case 'conf':
    case 'properties':
      return 'properties';
    case 'sh':
    case 'bash':
    case 'zsh':
      return 'shell';
    case 'js':
    case 'mjs':
    case 'cjs':
    case 'jsx':
      return 'javascript';
    case 'ts':
    case 'mts':
    case 'cts':
    case 'tsx':
      return 'typescript';
    case 'py':
    case 'pyw':
      return 'python';
    case 'rs':
      return 'rust';
    case 'css':
    case 'scss':
    case 'less':
      return 'css';
    case 'go':
      return 'go';
    case 'rb':
      return 'ruby';
    case 'sql':
      return 'sql';
    case 'c':
    case 'h':
    case 'cpp':
    case 'cc':
    case 'cxx':
    case 'hpp':
    case 'java':
    case 'cs':
    case 'kt':
    case 'kts':
    case 'swift':
      return 'clike';
    default:
      return 'text';
  }
}

export function languageExtension(language: EditableLanguage): Extension {
  if (language === 'json') return json();
  if (language === 'markdown') return markdown();
  if (language === 'xml') return xml();
  if (language === 'javascript') return javascript({ jsx: true });
  if (language === 'typescript') return javascript({ jsx: true, typescript: true });
  if (language === 'toml') return StreamLanguage.define(toml);
  if (language === 'yaml') return StreamLanguage.define(yaml);
  if (language === 'properties') return StreamLanguage.define(properties);
  if (language === 'shell') return StreamLanguage.define(shell);
  if (language === 'python') return StreamLanguage.define(python);
  if (language === 'rust') return StreamLanguage.define(rust);
  if (language === 'css') return StreamLanguage.define(css);
  if (language === 'go') return StreamLanguage.define(go);
  if (language === 'ruby') return StreamLanguage.define(ruby);
  if (language === 'sql') return StreamLanguage.define(sql({}));
  if (language === 'clike') return StreamLanguage.define(clike({ name: 'clike' }));
  return [];
}

export { jsonParseLinter };
