/**
 * Decides whether a command is too dangerous to run without asking, even in auto-approve mode
 * (task 0356). The opposite of {@link isReadOnlyCommand}: that check proves a line harmless; this
 * one only stops lines that can wreck a machine, so ordinary work (deleting a file in the working
 * folder, clearing entries under `/tmp`) runs without a question.
 *
 * Very dangerous means: deleting, moving or recursively changing the filesystem root, the home
 * folder or one of its ancestors, the session's working folder itself, or anything in a system
 * folder; writing to a disk device; formatting or partitioning; running as another user; turning
 * the machine off; a fork bomb; or running a downloaded script. A target the check cannot work
 * out (a variable, a command substitution, `~user`) counts as dangerous when it would be deleted.
 */

/** Why a command still asks in auto-approve mode. */
export type DangerKind =
  /** It deletes, moves or recursively changes the root, the home folder or an ancestor of it. */
  | 'home'
  /** It changes something in a system folder. */
  | 'system'
  /** It deletes or moves the working folder itself. */
  | 'workingFolder'
  /** It changes a path that cannot be worked out before it runs. */
  | 'unresolved'
  /** It writes to a disk device, or formats or partitions a disk. */
  | 'disk'
  /** It runs as another user (`sudo`, `doas`, `su`). */
  | 'privilege'
  /** It shuts down or restarts the machine. */
  | 'power'
  /** It pipes a download into a shell or interpreter. */
  | 'remoteScript'
  /** It is a fork bomb. */
  | 'forkBomb'
  /** The line could not be read as shell words (an unclosed quote, for example). */
  | 'unparsed';

/** Where the command runs. Both are native paths; an unknown one makes the check stricter. */
export interface CommandSite {
  /** The shell's working folder. */
  readonly cwd?: string;
  /** The person's home folder. */
  readonly home?: string;
}

const MAX_LENGTH = 4000;

/** Folders whose contents belong to the system; changing anything in them asks. */
const SYSTEM_FOLDERS = [
  '/System',
  '/usr',
  '/bin',
  '/sbin',
  '/etc',
  '/var',
  '/private',
  '/Library',
  '/Applications',
  '/opt',
  '/dev',
  '/boot',
  '/lib',
  '/lib32',
  '/lib64',
  '/proc',
  '/sys',
  '/nix',
  '/snap',
  '/srv',
  '/cores',
];
/** Inside a system folder, but meant for scratch files: entries may be changed freely. */
const SCRATCH_FOLDERS = [
  '/tmp',
  '/var/tmp',
  '/var/folders',
  '/private/tmp',
  '/private/var/tmp',
  '/private/var/folders',
];
/** Folders whose direct entries are mount points or homes: the entries themselves ask. */
const MOUNT_PARENTS = ['/Volumes', '/mnt', '/media', '/Users', '/home'];
/** Windows system folders, lower case with `/` separators after the drive. */
const WINDOWS_SYSTEM = ['windows', 'program files', 'program files (x86)', 'programdata'];

/** Programs whose every operand is a path they delete or move. */
export const REMOVERS = new Set([
  'rm',
  'rmdir',
  'unlink',
  'shred',
  'srm',
  'trash',
  'mv',
  'del',
  'erase',
  'rd',
  'remove-item',
  'ri',
  'move-item',
]);
/** Programs that change ownership or permissions; dangerous when recursive or in a system folder. */
const PERMISSIONS = new Set(['chmod', 'chown', 'chgrp', 'chflags', 'setfacl', 'icacls', 'takeown']);
/** Programs that format, partition or wipe disks. */
const DISK_TOOLS = new Set([
  'mkfs',
  'fdisk',
  'sfdisk',
  'cfdisk',
  'gdisk',
  'sgdisk',
  'parted',
  'wipefs',
  'format',
  'diskpart',
  'newfs',
  'newfs_apfs',
  'newfs_hfs',
  'clear-disk',
  'format-volume',
]);
/** `diskutil` verbs that destroy data. */
const DISKUTIL_DESTRUCTIVE =
  /^(erase\w*|partitiondisk|zerodisk|randomdisk|secureerase|reformat|deletevolume|deletecontainer|splitpartition|mergepartitions|resizevolume)$/i;
const POWER = new Set([
  'shutdown',
  'reboot',
  'halt',
  'poweroff',
  'stop-computer',
  'restart-computer',
]);
export const PRIVILEGE = new Set(['sudo', 'doas', 'su', 'pkexec', 'runas']);
/** Programs that run another program given as their arguments. */
const WRAPPERS = new Set([
  'command',
  'builtin',
  'exec',
  'nice',
  'nohup',
  'time',
  'env',
  'timeout',
  'caffeinate',
]);
export const DOWNLOADERS = new Set([
  'curl',
  'wget',
  'fetch',
  'iwr',
  'invoke-webrequest',
  'irm',
  'invoke-restmethod',
]);
const INTERPRETERS = new Set([
  'sh',
  'bash',
  'zsh',
  'dash',
  'ksh',
  'fish',
  'python',
  'python3',
  'perl',
  'ruby',
  'node',
  'pwsh',
  'powershell',
  'iex',
  'invoke-expression',
]);
export const SHELLS_WITH_C = new Set(['sh', 'bash', 'zsh', 'dash', 'ksh', 'fish']);
/** Redirection targets that write nowhere. */
export const HARMLESS_DEVICES = new Set([
  '/dev/null',
  '/dev/stdout',
  '/dev/stderr',
  '/dev/tty',
  '/dev/zero',
]);

/** One shell word: its text, and whether it held something only the shell can resolve. */
export interface Word {
  readonly text: string;
  /** A variable other than `$HOME`, a command substitution, or a backtick. */
  readonly dynamic: boolean;
  /** An unquoted `*`, `?` or `[`. */
  readonly glob: boolean;
}

/** A simple command, with the redirections written in it. */
export interface Simple {
  readonly words: Word[];
  readonly redirects: Word[];
  /** Whether its output is piped into the next command. */
  readonly pipedOut: boolean;
}

const FORK_BOMB = /(\w+|:)\s*\(\s*\)\s*\{[^}]*\1\s*\|\s*\1\s*&[^}]*\}\s*;?\s*\1/;

/**
 * Splits `line` into simple commands, or `undefined` when a quote is left open. `$HOME`,
 * `${HOME}` and a leading `~` become `home` when it is known.
 */
export function parse(line: string, home: string | undefined): Simple[] | undefined {
  const commands: Simple[] = [];
  let words: Word[] = [];
  let redirects: Word[] = [];
  let text: string | undefined;
  let dynamic = false;
  let glob = false;
  let quote: '"' | "'" | undefined;
  let redirectNext = false;
  const endWord = () => {
    if (text === undefined) return;
    const word = { text, dynamic, glob };
    if (redirectNext) redirects.push(word);
    else words.push(word);
    redirectNext = false;
    text = undefined;
    dynamic = false;
    glob = false;
  };
  const endCommand = (pipedOut: boolean) => {
    endWord();
    if (words.length > 0 || redirects.length > 0) commands.push({ words, redirects, pipedOut });
    words = [];
    redirects = [];
  };
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index] ?? '';
    const rest = line.slice(index);
    if (quote === "'") {
      if (character === "'") quote = undefined;
      else text = (text ?? '') + character;
      continue;
    }
    // A Windows path keeps its backslashes (`C:\Users`); elsewhere a backslash escapes.
    if (character === '\\' && text !== undefined && /^[A-Za-z]:/.test(text)) {
      text += character;
      continue;
    }
    if (character === '\\' && quote === undefined) {
      text = (text ?? '') + (line[index + 1] ?? '');
      index += 1;
      continue;
    }
    if (character === '$' || character === '`') {
      const variable = /^\$(?:HOME\b|\{HOME\})/.exec(rest);
      if (variable !== null && home !== undefined) {
        text = (text ?? '') + home;
        index += variable[0].length - 1;
      } else {
        dynamic = true;
        text = (text ?? '') + character;
      }
      continue;
    }
    if (quote === '"') {
      if (character === '"') quote = undefined;
      else text = (text ?? '') + character;
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
      text = text ?? '';
      continue;
    }
    if (character === '~' && text === undefined) {
      const user = /^~([A-Za-z_][\w.-]*)/.exec(rest);
      if (user !== null) {
        dynamic = true;
        text = user[0];
        index += user[0].length - 1;
      } else if (home !== undefined) {
        text = home;
      } else {
        dynamic = true;
        text = '~';
      }
      continue;
    }
    if (/\s/.test(character)) {
      endWord();
    } else if (character === '|') {
      endCommand(rest[1] !== '|');
      if (rest[1] === '|') index += 1;
    } else if (character === ';' || character === '&' || character === '(' || character === ')') {
      endCommand(false);
      if (character === '&' && rest[1] === '&') index += 1;
    } else if ((character === '{' || character === '}') && text === undefined) {
      endCommand(false);
    } else if (character === '>' || character === '<') {
      // `2>file`, `&>file`, `>>file`, `>|file`: the next word is where it goes.
      if (text !== undefined && /^\d+$/.test(text)) text = undefined;
      endWord();
      while (line[index + 1] === '>' || line[index + 1] === '|' || line[index + 1] === '&') {
        index += 1;
      }
      if (character === '>') redirectNext = true;
    } else {
      if (character === '*' || character === '?' || character === '[') glob = true;
      text = (text ?? '') + character;
    }
  }
  if (quote !== undefined) return undefined;
  endCommand(false);
  return commands;
}

/** `path` with `.` and `..` resolved, no trailing `/`, `\` read as `/` on a Windows drive. */
export function normalize(path: string): string {
  const windows = /^[A-Za-z]:[\\/]?/.exec(path);
  const unified = windows === null ? path : path.replace(/\\/g, '/');
  const drive = windows === null ? '' : unified.slice(0, 2);
  const parts: string[] = [];
  for (const part of unified.slice(drive.length).split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') parts.pop();
    else parts.push(part);
  }
  return `${drive}/${parts.join('/')}`;
}

const isInside = (path: string, folder: string): boolean =>
  path === folder || path.startsWith(folder === '/' ? '/' : `${folder}/`);

const isAncestorOrSelf = (folder: string, path: string): boolean => isInside(path, folder);

/** Why changing `path` (already normalised) is dangerous, if it is. */
function protectedPath(
  path: string,
  site: CommandSite,
  recursive: boolean,
): DangerKind | undefined {
  if (path === '/' || /^[A-Za-z]:\/$/.test(path)) return 'home';
  const home = site.home === undefined ? undefined : normalize(site.home);
  if (home !== undefined && isAncestorOrSelf(path, home)) return 'home';
  const cwd = site.cwd === undefined ? undefined : normalize(site.cwd);
  if (cwd !== undefined && path === cwd) return 'workingFolder';
  // Inside the known home folder (not the folder itself) is the person's own work.
  if (home !== undefined && isInside(path, home)) return undefined;
  if (MOUNT_PARENTS.some((parent) => path === parent || dirname(path) === parent)) return 'home';
  if (path === '/root') return 'home';
  if (SCRATCH_FOLDERS.some((scratch) => path !== scratch && isInside(path, scratch))) {
    return undefined;
  }
  if (SYSTEM_FOLDERS.some((folder) => isInside(path, folder))) return 'system';
  const windows = /^[A-Za-z]:\/([^/]+)(\/[^/]+)?/.exec(path);
  if (windows !== null) {
    const top = (windows[1] ?? '').toLowerCase();
    if (WINDOWS_SYSTEM.includes(top)) return 'system';
    if (top === 'users' && windows[2] === undefined) return 'home';
    if (top === 'users' && path.split('/').length === 3) return 'home';
  }
  // A recursive change at the top of a disk is as bad as one at the root.
  if (recursive && path.split('/').length === 2) return 'system';
  return undefined;
}

function dirname(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut <= 0 ? '/' : path.slice(0, cut);
}

/**
 * Why changing `word` is dangerous. A glob, like the start of a `find -delete`, stands for the
 * entries of its folder: `rm -rf ~/*` empties the home folder as surely as `rm -rf ~` removes it,
 * while `rm -rf /tmp/*` only clears scratch files.
 */
function targetDanger(
  word: Word,
  site: CommandSite,
  recursive: boolean,
  entries = false,
): DangerKind | undefined {
  if (word.dynamic) return 'unresolved';
  let path = word.text;
  if (path === '') return undefined;
  let ofEntries = entries;
  if (word.glob) {
    const first = path.search(/[*?[]/);
    const folder = path.slice(0, first);
    path = folder.includes('/') ? folder.slice(0, folder.lastIndexOf('/') + 1) || '/' : '.';
    ofEntries = true;
  }
  const absolute = path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path);
  if (!absolute) {
    if (site.cwd === undefined) {
      // Without a known folder, only names that stay below it are known not to be the folder.
      const parts = path.split('/').filter((part) => part !== '' && part !== '.');
      if (parts.length === 0 || parts.includes('..')) return 'unresolved';
      return undefined;
    }
    path = `${site.cwd}/${path}`;
  }
  const resolved = normalize(path);
  if (!ofEntries) return protectedPath(resolved, site, recursive);
  const home = site.home === undefined ? undefined : normalize(site.home);
  if (
    resolved === '/' ||
    /^[A-Za-z]:\/$/.test(resolved) ||
    (home !== undefined && isAncestorOrSelf(resolved, home)) ||
    MOUNT_PARENTS.includes(resolved)
  ) {
    return 'home';
  }
  // An entry of the folder: inside the working folder or a scratch folder is fine.
  return protectedPath(`${resolved === '/' ? '' : resolved}/*`, site, recursive);
}

const lower = (word: Word | undefined): string => (word?.text ?? '').toLowerCase();
export const programName = (word: Word | undefined): string => {
  const name = lower(word);
  return name.slice(name.lastIndexOf('/') + 1).replace(/\.exe$/, '');
};

/** The words after a wrapper such as `nice -n 5` or `env FOO=1`, starting at the real program. */
export function unwrap(words: Word[]): Word[] {
  let index = 0;
  while (index < words.length) {
    const word = words[index];
    const name = programName(word);
    if (/^[A-Za-z_]\w*=/.test(word?.text ?? '')) {
      index += 1;
    } else if (WRAPPERS.has(name)) {
      index += 1;
      // Options of the wrapper, and `timeout`'s duration.
      while (index < words.length && (words[index]?.text.startsWith('-') ?? false)) index += 1;
      // A value such as `nice -n 5` or `timeout 10s`.
      if (/^\d+[smhd]?$/.test(words[index]?.text ?? '')) index += 1;
    } else {
      break;
    }
  }
  return words.slice(index);
}

/** Splits arguments into options and operands; everything after `--` is an operand. */
export function operandsOf(args: Word[]): { options: string[]; operands: Word[] } {
  const options: string[] = [];
  const operands: Word[] = [];
  let ended = false;
  for (const arg of args) {
    if (!ended && arg.text === '--') ended = true;
    else if (
      !ended &&
      arg.text.length > 1 &&
      (arg.text.startsWith('-') || /^\/[a-z?]$/i.test(arg.text))
    )
      options.push(arg.text);
    else operands.push(arg);
  }
  return { options, operands };
}

const isRecursive = (options: string[]): boolean =>
  options.some(
    (option) =>
      option === '--recursive' ||
      /^-recurse$/i.test(option) ||
      /^\/s$/i.test(option) ||
      (/^-[A-Za-z]+$/.test(option) && /[rR]/.test(option)),
  );

/** The danger of one simple command, given the working folder it runs in. */
function commandDanger(command: Simple, site: CommandSite, depth: number): DangerKind | undefined {
  for (const target of command.redirects) {
    const path = target.text;
    if (HARMLESS_DEVICES.has(path)) continue;
    if (path.startsWith('/dev/')) return 'disk';
    const danger = targetDanger(target, site, false);
    // Writing to a file named by a variable is ordinary; overwriting a system file is not.
    if (danger !== undefined && danger !== 'unresolved' && danger !== 'workingFolder')
      return danger;
  }
  const words = unwrap(command.words);
  const name = programName(words[0]);
  const args = words.slice(1);
  if (name === '') return undefined;
  if (PRIVILEGE.has(name)) return 'privilege';
  if (POWER.has(name) || (name === 'init' && /^[06]$/.test(args[0]?.text ?? ''))) return 'power';
  if (DISK_TOOLS.has(name) || name.startsWith('mkfs.')) return 'disk';
  if (name === 'diskutil' && args.some((arg) => DISKUTIL_DESTRUCTIVE.test(arg.text))) return 'disk';
  if (name === 'dd') {
    const output = args.find((arg) => arg.text.startsWith('of='));
    if (output === undefined) return undefined;
    const path = output.text.slice(3);
    if (path.startsWith('/dev/') && !HARMLESS_DEVICES.has(path)) return 'disk';
    return targetDanger({ ...output, text: path }, site, false);
  }
  if (name === 'xargs') {
    // `xargs rm` deletes names it reads at run time.
    const inner = unwrap(args.filter((arg) => !arg.text.startsWith('-')));
    return REMOVERS.has(programName(inner[0])) ? 'unresolved' : undefined;
  }
  if ((SHELLS_WITH_C.has(name) || name === 'eval') && depth < 3) {
    const script =
      name === 'eval' ? args : args.slice(args.findIndex((arg) => arg.text === '-c') + 1);
    if (name === 'eval' || args.some((arg) => arg.text === '-c')) {
      const inner = script[0];
      if (inner === undefined) return undefined;
      if (inner.dynamic) return 'unresolved';
      return lineDanger(
        name === 'eval' ? script.map((w) => w.text).join(' ') : inner.text,
        site,
        depth + 1,
      );
    }
  }
  const { options, operands } = operandsOf(args);
  if (REMOVERS.has(name)) {
    const recursive = isRecursive(options) || name === 'mv' || name === 'move-item';
    for (const operand of operands) {
      const danger = targetDanger(operand, site, recursive);
      if (danger !== undefined) return danger;
    }
    return undefined;
  }
  if (PERMISSIONS.has(name)) {
    const recursive = isRecursive(options);
    // The first operand of chmod/chown is the mode or owner, not a path.
    const paths = name === 'icacls' || name === 'takeown' ? operands : operands.slice(1);
    for (const path of paths) {
      const danger = targetDanger(path, site, recursive);
      if (danger === 'system' || (recursive && danger !== undefined)) return danger;
    }
    return undefined;
  }
  if (name === 'find' && args.some((arg) => ['-delete', '-exec', '-execdir'].includes(arg.text))) {
    const deletes =
      args.some((arg) => arg.text === '-delete') ||
      args.some((arg) => REMOVERS.has(programName(arg)));
    if (!deletes) return undefined;
    const starts = args.slice(
      0,
      args.findIndex((arg) => arg.text.startsWith('-')),
    );
    for (const start of starts.length === 0
      ? [{ text: '.', dynamic: false, glob: false }]
      : starts) {
      const danger = targetDanger(start, site, true, true);
      if (danger !== undefined) return danger;
    }
    return undefined;
  }
  if (name === 'rsync' && options.some((option) => option.startsWith('--delete'))) {
    const destination = operands[operands.length - 1];
    return destination === undefined ? undefined : targetDanger(destination, site, true);
  }
  if (name === 'cp' || name === 'ln' || name === 'install' || name === 'copy-item') {
    const destination = operands[operands.length - 1];
    if (destination === undefined || operands.length < 2) return undefined;
    const danger = targetDanger(destination, site, false);
    return danger === 'system' ? danger : undefined;
  }
  return undefined;
}

function lineDanger(line: string, site: CommandSite, depth: number): DangerKind | undefined {
  if (FORK_BOMB.test(line)) return 'forkBomb';
  const commands = parse(line, site.home);
  if (commands === undefined) return 'unparsed';
  let cwd = site.cwd;
  for (const [index, command] of commands.entries()) {
    const words = unwrap(command.words);
    const name = programName(words[0]);
    if (command.pipedOut && DOWNLOADERS.has(name)) {
      const next = commands[index + 1];
      const nextWords = next === undefined ? [] : unwrap(next.words);
      const runner = PRIVILEGE.has(programName(nextWords[0]))
        ? unwrap(nextWords.slice(1))
        : nextWords;
      if (INTERPRETERS.has(programName(runner[0]))) {
        return 'remoteScript';
      }
    }
    if ((name === 'cd' || name === 'pushd' || name === 'set-location') && depth === 0) {
      cwd = changedFolder(words[1], cwd, site.home);
      continue;
    }
    const here: CommandSite = {
      ...(cwd === undefined ? {} : { cwd }),
      ...(site.home === undefined ? {} : { home: site.home }),
    };
    const danger = commandDanger(command, here, depth);
    if (danger !== undefined) return danger;
  }
  return undefined;
}

/** The working folder after `cd target`; unknown when the target cannot be worked out. */
function changedFolder(
  target: Word | undefined,
  cwd: string | undefined,
  home: string | undefined,
): string | undefined {
  if (target === undefined) return home;
  if (target.dynamic || target.glob || target.text === '-') return undefined;
  if (target.text.startsWith('/') || /^[A-Za-z]:[\\/]/.test(target.text)) {
    return normalize(target.text);
  }
  return cwd === undefined ? undefined : normalize(`${cwd}/${target.text}`);
}

/**
 * Why `line` must still be approved in auto-approve mode, or `undefined` when it may run.
 * `site` gives the working and home folders; without them, relative targets that could be
 * either count as dangerous.
 */
export function dangerOf(line: string, site: CommandSite): DangerKind | undefined {
  if (line.length > MAX_LENGTH) return 'unparsed';
  return lineDanger(line, site, 0);
}
