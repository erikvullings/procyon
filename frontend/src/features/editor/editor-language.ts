import { javascript } from '@codemirror/lang-javascript';
import { json, jsonParseLinter } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import { php } from '@codemirror/lang-php';
import { xml } from '@codemirror/lang-xml';
import { StreamLanguage, type StreamParser } from '@codemirror/language';
import {
  c,
  cpp,
  csharp,
  dart,
  java,
  kotlin,
  objectiveC,
  objectiveCpp,
  scala,
} from '@codemirror/legacy-modes/mode/clike';
import { cmake } from '@codemirror/legacy-modes/mode/cmake';
import { css } from '@codemirror/legacy-modes/mode/css';
import { diff } from '@codemirror/legacy-modes/mode/diff';
import { dockerFile } from '@codemirror/legacy-modes/mode/dockerfile';
import { erlang } from '@codemirror/legacy-modes/mode/erlang';
import { go } from '@codemirror/legacy-modes/mode/go';
import { groovy } from '@codemirror/legacy-modes/mode/groovy';
import { haskell } from '@codemirror/legacy-modes/mode/haskell';
import { julia } from '@codemirror/legacy-modes/mode/julia';
import { lua } from '@codemirror/legacy-modes/mode/lua';
import { nginx } from '@codemirror/legacy-modes/mode/nginx';
import { pascal } from '@codemirror/legacy-modes/mode/pascal';
import { perl } from '@codemirror/legacy-modes/mode/perl';
import { powerShell } from '@codemirror/legacy-modes/mode/powershell';
import { properties } from '@codemirror/legacy-modes/mode/properties';
import { protobuf } from '@codemirror/legacy-modes/mode/protobuf';
import { python } from '@codemirror/legacy-modes/mode/python';
import { r } from '@codemirror/legacy-modes/mode/r';
import { ruby } from '@codemirror/legacy-modes/mode/ruby';
import { rust } from '@codemirror/legacy-modes/mode/rust';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { sql } from '@codemirror/legacy-modes/mode/sql';
import { swift } from '@codemirror/legacy-modes/mode/swift';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { vb } from '@codemirror/legacy-modes/mode/vb';
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
  | 'c'
  | 'cpp'
  | 'java'
  | 'csharp'
  | 'kotlin'
  | 'scala'
  | 'dart'
  | 'objectivec'
  | 'objectivecpp'
  | 'swift'
  | 'php'
  | 'lua'
  | 'perl'
  | 'powershell'
  | 'r'
  | 'haskell'
  | 'groovy'
  | 'julia'
  | 'erlang'
  | 'cmake'
  | 'protobuf'
  | 'diff'
  | 'vb'
  | 'pascal'
  | 'dockerfile'
  | 'nginx';

export function editableLanguageForExtension(
  extension: string | undefined,
  fileName = '',
): EditableLanguage {
  const lowerName = fileName.toLowerCase();
  if (lowerName === '.env' || lowerName.startsWith('.env.') || lowerName === '.editorconfig')
    return 'properties';
  if (lowerName === 'dockerfile') return 'dockerfile';
  if (lowerName === 'cmakelists.txt') return 'cmake';
  if (lowerName === 'nginx.conf') return 'nginx';
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
      return 'c';
    case 'cpp':
    case 'cc':
    case 'cxx':
    case 'hpp':
    case 'hh':
    case 'hxx':
      return 'cpp';
    case 'java':
      return 'java';
    case 'cs':
      return 'csharp';
    case 'kt':
    case 'kts':
      return 'kotlin';
    case 'scala':
    case 'sc':
      return 'scala';
    case 'dart':
      return 'dart';
    case 'm':
      return 'objectivec';
    case 'mm':
      return 'objectivecpp';
    case 'swift':
      return 'swift';
    case 'php':
    case 'phtml':
      return 'php';
    case 'lua':
      return 'lua';
    case 'pl':
    case 'pm':
      return 'perl';
    case 'ps1':
    case 'psm1':
      return 'powershell';
    case 'r':
      return 'r';
    case 'hs':
      return 'haskell';
    case 'groovy':
    case 'gradle':
      return 'groovy';
    case 'jl':
      return 'julia';
    case 'erl':
    case 'hrl':
      return 'erlang';
    case 'cmake':
      return 'cmake';
    case 'proto':
      return 'protobuf';
    case 'diff':
    case 'patch':
      return 'diff';
    case 'vb':
    case 'vbs':
      return 'vb';
    case 'pas':
    case 'pp':
      return 'pascal';
    default:
      return 'text';
  }
}

const STREAM_PARSERS = {
  c,
  cpp,
  java,
  csharp,
  kotlin,
  scala,
  dart,
  objectivec: objectiveC,
  objectivecpp: objectiveCpp,
  swift,
  lua,
  perl,
  powershell: powerShell,
  r,
  haskell,
  groovy,
  julia,
  erlang,
  cmake,
  protobuf,
  diff,
  vb,
  pascal,
  dockerfile: dockerFile,
  nginx,
} satisfies Partial<Record<EditableLanguage, StreamParser<unknown>>>;

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
  if (language === 'php') return php();
  if (language in STREAM_PARSERS) {
    return StreamLanguage.define(STREAM_PARSERS[language as keyof typeof STREAM_PARSERS]);
  }
  return [];
}

export { jsonParseLinter };
