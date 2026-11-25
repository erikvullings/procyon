/**
 * What a command the assistant proposes does, worked out by Procyon itself (task 0364), so the
 * approval does not rest on the model's own explanation: the programs it runs, what it writes or
 * deletes, whether it uses the network or another user's rights, and for inline code and script
 * files a scan of the code. The scan is a heuristic that only ever widens what asks: code it
 * cannot read safely is "unknown", never "read-only".
 */

import {
  type CommandSite,
  DOWNLOADERS,
  HARMLESS_DEVICES,
  normalize,
  operandsOf,
  PRIVILEGE,
  parse,
  programName,
  REMOVERS,
  SHELLS_WITH_C,
  type Simple,
  unwrap,
  type Word,
} from './dangerous-command';

/** A script file the command runs, as the host read it from the working folder. */
export interface ScriptText {
  readonly path: string;
  readonly text: string;
  readonly truncated: boolean;
}

/** What one piece of code (inline or a file) does, as far as a scan can tell. */
export interface ScriptEffects {
  readonly language: 'python' | 'node';
  /** The file it was read from; absent for inline code. */
  readonly path?: string;
  readonly lines: number;
  /** Whether the whole text was available to scan. */
  readonly known: boolean;
  readonly writes: boolean;
  readonly deletes: boolean;
  readonly network: boolean;
  /** It starts other programs. */
  readonly subprocess: boolean;
  /** It builds or loads code at run time (`eval`, `exec`, dynamic imports): it cannot be checked. */
  readonly dynamicCode: boolean;
  /** It names paths outside the working folder, the home folder or the environment. */
  readonly outsideFolder: boolean;
  /** Modules it imports that are not known to only read. */
  readonly modules: readonly string[];
}

/** What a whole command line does. */
export interface CommandEffects {
  /** Whether the line could be read as shell words. */
  readonly readable: boolean;
  readonly programs: readonly string[];
  readonly writes: readonly string[];
  readonly deletes: readonly string[];
  readonly network: boolean;
  readonly privilege: boolean;
  readonly scripts: readonly ScriptEffects[];
}

const PYTHONS = /^python(3(\.\d+)?)?$/;
const NODES = new Set(['node', 'nodejs']);
/** Interpreter options that change nothing about what the code may do. */
const PYTHON_OPTIONS = new Set(['-u', '-B', '-I', '-E', '-s', '-S', '-O', '-OO', '-q']);
const NODE_OPTIONS = new Set(['--no-warnings', '--no-deprecation']);

/** Programs that use the network whatever their arguments. */
const NETWORK_PROGRAMS = new Set([
  ...DOWNLOADERS,
  'ssh',
  'scp',
  'sftp',
  'ftp',
  'nc',
  'netcat',
  'telnet',
  'ping',
  'dig',
  'nslookup',
  'gh',
]);
/** Programs whose listed subcommands use the network. */
const NETWORK_SUBCOMMANDS: Readonly<Record<string, readonly string[]>> = {
  git: ['clone', 'fetch', 'pull', 'push', 'ls-remote', 'submodule'],
  pip: ['install', 'download'],
  pip3: ['install', 'download'],
  npm: ['install', 'i', 'ci', 'add', 'update', 'publish'],
  pnpm: ['install', 'i', 'add', 'update', 'publish', 'dlx'],
  yarn: ['install', 'add', 'upgrade', 'publish', 'dlx'],
  bun: ['install', 'add', 'update', 'x'],
  npx: [''],
  brew: ['install', 'update', 'upgrade', 'fetch', 'reinstall'],
  apt: ['install', 'update', 'upgrade'],
  'apt-get': ['install', 'update', 'upgrade'],
  dnf: ['install', 'update', 'upgrade'],
  cargo: ['install', 'fetch', 'update', 'add', 'publish', 'search'],
  uv: ['add', 'sync', 'pip', 'lock', 'tool', 'run'],
  docker: ['pull', 'push', 'login', 'run', 'build'],
};
/** Programs that write their operands (or the last one). */
const WRITE_OPERANDS = new Set(['touch', 'mkdir', 'truncate', 'tee']);
const WRITE_DESTINATION = new Set(['cp', 'install', 'ln', 'rsync', 'copy-item', 'mv', 'move-item']);

/** Python modules that only compute or read; anything else may do more. */
const PYTHON_READ_ONLY_MODULES = new Set([
  'os',
  'sys',
  're',
  'json',
  'pathlib',
  'collections',
  'itertools',
  'functools',
  'math',
  'statistics',
  'csv',
  'datetime',
  'time',
  'glob',
  'fnmatch',
  'hashlib',
  'textwrap',
  'string',
  'operator',
  'typing',
  'dataclasses',
  'pprint',
  'struct',
  'base64',
  'unicodedata',
  'decimal',
  'fractions',
  'random',
  'enum',
  'heapq',
  'bisect',
  'difflib',
  'argparse',
  'io',
  'codecs',
  'calendar',
  'tomllib',
  'configparser',
  'uuid',
  'shlex',
  'html',
  'mimetypes',
  'copy',
  'contextlib',
  'abc',
  'array',
  'numbers',
  'zlib',
  'gzip',
  'bz2',
  'lzma',
  'zipfile',
  'tarfile',
  'xml',
  'yaml',
  'numpy',
  'pandas',
]);
/** Node modules that only compute or read; anything else may do more. */
const NODE_READ_ONLY_MODULES = new Set([
  'fs',
  'fs/promises',
  'path',
  'os',
  'util',
  'url',
  'crypto',
  'readline',
  'assert',
  'events',
  'string_decoder',
  'zlib',
  'buffer',
]);

const PYTHON_DYNAMIC =
  /\b(eval|exec|compile|__import__|getattr|setattr|delattr|globals|locals|vars|breakpoint)\s*\(|\bimportlib\b|__builtins__|\b(pickle|marshal|shelve|ctypes|cffi)\b/;
const PYTHON_SUBPROCESS =
  /\bsubprocess\b|\bos\.(system|popen|exec\w*|spawn\w*|fork\w*|kill\w*|startfile)\b|\bpty\b|\bmultiprocessing\b|create_subprocess/;
const PYTHON_NETWORK =
  /\b(socket|ssl|requests|urllib\d?|httpx|aiohttp|ftplib|smtplib|poplib|imaplib|telnetlib|paramiko|websockets?|xmlrpc|webbrowser)\b|\bhttp\.(client|server)\b/;
const PYTHON_DELETES =
  /\bos\.(remove|unlink|rmdir|removedirs)\b|\bshutil\.(rmtree|move)\b|\bsend2trash\b|\.(unlink|rmdir)\s*\(/;
const PYTHON_WRITES = [
  // `open(path, 'w')` and friends, a mode given as a variable, or `mode=`.
  /\bopen\s*\([^)]*?,\s*(mode\s*=\s*)?['"][^'"]*[wax+][^'"]*['"]/,
  /\bopen\s*\(\s*[^,)]+,\s*(?!['"]|encoding|errors|newline|buffering|closefd)[A-Za-z_]/,
  /\bmode\s*=\s*(?!['"]r?b?t?['"])/,
  /\.(write_text|write_bytes|touch|mkdir|rename|replace|symlink_to|hardlink_to|chmod|lchmod)\s*\(/,
  /\bos\.(rename|renames|replace|mkdir|makedirs|chmod|chown|lchown|symlink|link|truncate|utime|mkfifo|mknod)\b/,
  /\bshutil\.(copy\w*|chown|make_archive|unpack_archive)\b/,
  /\.to_(csv|excel|parquet|json|pickle|sql|feather|hdf|html|latex|markdown|xml|stata|clipboard)\s*\(/,
  /\.(savefig|save|tofile|extractall|extract)\s*\(/,
  /\bnp\.save\w*\s*\(/,
];
const NODE_DYNAMIC =
  /\beval\s*\(|\bnew\s+Function\b|\bFunction\s*\(|\brequire\s*\(\s*[^'"\s)]|\bimport\s*\(\s*[^'"\s)]|\bvm\b/;
const NODE_SUBPROCESS =
  /\bchild_process\b|\bworker_threads\b|\bprocess\.(kill|dlopen|binding)\b|\bDeno\.(run|Command)\b|\bBun\.spawn/;
const NODE_NETWORK =
  /\bfetch\s*\(|\bWebSocket\b|\baxios\b|\bundici\b|XMLHttpRequest|['"](node:)?(https?|net|dgram|tls|http2|dns)['"]/;
const NODE_DELETES = /\b(rm|rmSync|rmdir|rmdirSync|unlink|unlinkSync)\s*\(|\brimraf\b/;
const NODE_WRITES =
  /\b(writeFile|writeFileSync|appendFile|appendFileSync|copyFile|copyFileSync|cp|cpSync|rename|renameSync|mkdir|mkdirSync|mkdtemp\w*|chmod\w*|chown\w*|symlink\w*|link|linkSync|truncate\w*|utimes\w*|createWriteStream|open|openSync)\s*\(/;
/** A `.write(` that is not printing to standard output or error. */
const PYTHON_WRITE_CALL = /(?<!sys\.std(out|err))\.write\s*\(/;
const NODE_WRITE_CALL = /(?<!process\.std(out|err))\.write\s*\(/;
/** A string that names a path outside the working folder: absolute, home or a parent. */
const OUTSIDE_LITERAL =
  /['"`](\/[^'"`\s]+|~[^'"`]*|[A-Za-z]:\\[^'"`]*|(\.\.\/|[^'"`]*\/\.\.\/)[^'"`]*|\.\.)['"`]/g;
const PYTHON_ENVIRONMENT = /\b(os\.environ|getenv|expanduser|Path\.home|home\s*\(\s*\))/;
const NODE_ENVIRONMENT = /\bprocess\.env\b|\bhomedir\s*\(/;
/** Names that hold credentials. */
const CREDENTIAL =
  /(^|\/)(\.env|\.ssh|\.aws|\.gnupg|\.netrc|\.npmrc|\.pypirc|\.kube|\.docker|id_(rsa|ed25519|ecdsa|dsa)|[^/]*\.(pem|key|p12|pfx|kdbx)|[^/]*(secret|credential|password|token))/i;

function pythonModules(code: string): string[] {
  const modules: string[] = [];
  for (const match of code.matchAll(
    /(?:^|[;\n])\s*(?:from\s+([\w.]+)\s+import\b|import\s+([\w.]+(?:\s+as\s+\w+)?(?:\s*,\s*[\w.]+(?:\s+as\s+\w+)?)*))/g,
  )) {
    const names = match[1] ?? match[2] ?? '';
    for (const name of names.split(',')) {
      const root = name.trim().split(/\s+/)[0]?.split('.')[0];
      if (root !== undefined && root !== '') modules.push(root);
    }
  }
  return modules;
}

function nodeModules(code: string): string[] {
  const modules: string[] = [];
  for (const match of code.matchAll(
    /\brequire\s*\(\s*['"]([^'"]+)['"]\s*\)|\bfrom\s+['"]([^'"]+)['"]|\bimport\s+['"]([^'"]+)['"]|\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
  )) {
    const name = (match[1] ?? match[2] ?? match[3] ?? match[4] ?? '').replace(/^node:/, '');
    if (name !== '') modules.push(name);
  }
  return modules;
}

/** Whether a string in the code names a path outside `cwd`. */
function namesOutside(code: string, cwd: string | undefined): boolean {
  for (const match of code.matchAll(OUTSIDE_LITERAL)) {
    const path = match[1] ?? '';
    if (cwd !== undefined && path.startsWith('/') && normalize(path).startsWith(`${cwd}/`)) {
      continue;
    }
    if (path === '/dev/null' || path === '/dev/stdout' || path === '/dev/stderr') continue;
    return true;
  }
  return CREDENTIAL.test(code.replace(/["'`]/g, ' ').replace(/\s+/g, '/'));
}

/** Scans one piece of code. */
export function scanScript(
  language: 'python' | 'node',
  code: string,
  options: { readonly path?: string; readonly known: boolean; readonly cwd?: string | undefined },
): ScriptEffects {
  const python = language === 'python';
  const modules = (python ? pythonModules(code) : nodeModules(code)).filter(
    (module) =>
      !(python ? PYTHON_READ_ONLY_MODULES : NODE_READ_ONLY_MODULES).has(module) &&
      !(python ? PYTHON_NETWORK : NODE_NETWORK).test(python ? module : `'${module}'`),
  );
  const writes = python
    ? PYTHON_WRITES.some((pattern) => pattern.test(code)) || PYTHON_WRITE_CALL.test(code)
    : NODE_WRITES.test(code) || NODE_WRITE_CALL.test(code);
  return {
    language,
    ...(options.path === undefined ? {} : { path: options.path }),
    lines: code.split('\n').length,
    known: options.known,
    writes,
    deletes: (python ? PYTHON_DELETES : NODE_DELETES).test(code),
    network: (python ? PYTHON_NETWORK : NODE_NETWORK).test(code),
    subprocess: (python ? PYTHON_SUBPROCESS : NODE_SUBPROCESS).test(code),
    dynamicCode: (python ? PYTHON_DYNAMIC : NODE_DYNAMIC).test(code),
    outsideFolder:
      namesOutside(code, options.cwd) ||
      (python ? PYTHON_ENVIRONMENT : NODE_ENVIRONMENT).test(code),
    modules: [...new Set(modules)],
  };
}

/** Whether a scanned script only reads inside the working folder. */
export function scriptReadsOnly(script: ScriptEffects): boolean {
  return (
    script.known &&
    !script.writes &&
    !script.deletes &&
    !script.network &&
    !script.subprocess &&
    !script.dynamicCode &&
    !script.outsideFolder &&
    script.modules.length === 0
  );
}

const languageOf = (name: string): 'python' | 'node' | undefined =>
  PYTHONS.test(name) ? 'python' : NODES.has(name) ? 'node' : undefined;

/** The code a `python`/`node` command runs: inline (`-c`/`-e`) or the script file it names. */
function commandScript(
  words: Word[],
  scripts: readonly ScriptText[],
  cwd: string | undefined,
): { script: ScriptEffects; rest: Word[] } | undefined {
  const language = languageOf(programName(words[0]));
  if (language === undefined) return undefined;
  const harmless = language === 'python' ? PYTHON_OPTIONS : NODE_OPTIONS;
  let index = 1;
  while (harmless.has(words[index]?.text ?? '')) index += 1;
  const option = words[index]?.text ?? '';
  if (option === (language === 'python' ? '-c' : '-e')) {
    const code = words[index + 1];
    if (code === undefined || code.dynamic) return undefined;
    return {
      script: scanScript(language, code.text, { known: true, cwd }),
      rest: words.slice(index + 2),
    };
  }
  const operand = words[index];
  if (operand === undefined || operand.text.startsWith('-')) return undefined;
  const file = scripts.find(
    (script) => script.path === operand.text || `./${script.path}` === operand.text,
  );
  const known =
    file !== undefined &&
    !file.truncated &&
    !operand.dynamic &&
    !operand.glob &&
    !/^(\/|~)|(^|\/)\.\.(\/|$)/.test(operand.text);
  return {
    script: scanScript(language, file?.text ?? '', {
      path: operand.text,
      known,
      cwd,
    }),
    rest: words.slice(index + 1),
  };
}

function subcommand(words: Word[]): string {
  return words.slice(1).find((word) => !word.text.startsWith('-'))?.text ?? '';
}

function addCommand(
  command: Simple,
  site: CommandSite,
  scripts: readonly ScriptText[],
  into: {
    programs: Set<string>;
    writes: string[];
    deletes: string[];
    network: boolean;
    privilege: boolean;
    scripts: ScriptEffects[];
    readable: boolean;
  },
  depth: number,
): void {
  for (const target of command.redirects) {
    if (!HARMLESS_DEVICES.has(target.text)) into.writes.push(target.text);
  }
  let words = command.words;
  if (PRIVILEGE.has(programName(words[0]))) {
    into.privilege = true;
    words = words.slice(1);
  }
  words = unwrap(words);
  const name = programName(words[0]);
  if (name === '') return;
  into.programs.add(name);
  const args = words.slice(1);
  const { options, operands } = operandsOf(args);
  if (NETWORK_PROGRAMS.has(name)) into.network = true;
  const verbs = NETWORK_SUBCOMMANDS[name];
  if (verbs !== undefined && (verbs.includes('') || verbs.includes(subcommand(words)))) {
    into.network = true;
  }
  if (name === 'rsync' && operands.some((operand) => /^[^/]*:/.test(operand.text))) {
    into.network = true;
  }
  if (REMOVERS.has(name) && name !== 'mv' && name !== 'move-item') {
    into.deletes.push(...operands.map((operand) => operand.text));
  }
  if (name === 'mv' || name === 'move-item') {
    into.deletes.push(...operands.slice(0, -1).map((operand) => operand.text));
  }
  if (WRITE_DESTINATION.has(name) && operands.length >= 2) {
    into.writes.push(operands[operands.length - 1]?.text ?? '');
  }
  if (WRITE_OPERANDS.has(name)) into.writes.push(...operands.map((operand) => operand.text));
  if (name === 'sed' && options.some((option) => option.startsWith('-i'))) {
    into.writes.push(...operands.slice(1).map((operand) => operand.text));
  }
  if (name === 'find' && args.some((arg) => arg.text === '-delete')) {
    into.deletes.push(operands[0]?.text ?? '.');
  }
  if (name === 'git' && subcommand(words) === 'clean') into.deletes.push('.');
  if (
    (name === 'curl' && args.some((arg) => /^(-o|--output|-O|--remote-name)/.test(arg.text))) ||
    (name === 'wget' && !args.some((arg) => arg.text === '-O-' || arg.text === '-'))
  ) {
    into.writes.push('(a downloaded file)');
  }
  if (SHELLS_WITH_C.has(name) && depth < 3) {
    const at = args.findIndex((arg) => arg.text === '-c');
    const inner = at < 0 ? undefined : args[at + 1];
    if (inner !== undefined) {
      if (inner.dynamic) into.readable = false;
      else addLine(inner.text, site, scripts, into, depth + 1);
    }
  }
  const script = commandScript(words, scripts, site.cwd);
  if (script !== undefined) into.scripts.push(script.script);
}

function addLine(
  line: string,
  site: CommandSite,
  scripts: readonly ScriptText[],
  into: Parameters<typeof addCommand>[3],
  depth: number,
): void {
  const commands = parse(line, site.home);
  if (commands === undefined) {
    into.readable = false;
    return;
  }
  for (const command of commands) addCommand(command, site, scripts, into, depth);
}

/** What `line` does, with the text of the script files it runs when the host could read them. */
export function effectsOf(
  line: string,
  site: CommandSite,
  scripts: readonly ScriptText[] = [],
): CommandEffects {
  const into = {
    programs: new Set<string>(),
    writes: [] as string[],
    deletes: [] as string[],
    network: false,
    privilege: false,
    scripts: [] as ScriptEffects[],
    readable: true,
  };
  addLine(line, site, scripts, into, 0);
  return {
    readable: into.readable,
    programs: [...into.programs],
    writes: [...new Set(into.writes)],
    deletes: [...new Set(into.deletes)],
    network: into.network,
    privilege: into.privilege,
    scripts: into.scripts,
  };
}

/**
 * Whether `line` is one `python` or `node` command whose code (inline, or a script file the host
 * read in full) only reads inside the working folder, so it may run without asking like other
 * read-only commands. One command, no pipes or redirections, plain arguments.
 */
export function isReadOnlyScriptCommand(
  line: string,
  site: CommandSite,
  scripts: readonly ScriptText[] = [],
): boolean {
  const commands = parse(line, site.home);
  if (commands === undefined || commands.length !== 1) return false;
  const [command] = commands;
  if (command === undefined || command.pipedOut) return false;
  if (command.redirects.some((target) => target.text !== '/dev/null')) return false;
  const found = commandScript(command.words, scripts, site.cwd);
  if (found === undefined || !scriptReadsOnly(found.script)) return false;
  return found.rest.every(
    (arg) =>
      !arg.dynamic &&
      !arg.glob &&
      !/^(\/|~)|(^|\/)\.\.(\/|$)/.test(arg.text) &&
      !CREDENTIAL.test(arg.text),
  );
}
