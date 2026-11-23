/**
 * Decides whether a command the assistant proposes is strictly read-only, so it can run without
 * asking (task 0336). The check is deliberately narrow: a plain pipeline of a few allow-listed
 * programs whose arguments stay inside the working folder. Anything it does not understand is
 * "not read-only" and falls back to the person's approval. It never decides that a command is
 * safe in general, only that this exact line cannot write, execute other programs, expand
 * variables or read outside the folder.
 */

const MAX_LENGTH = 500;
const MAX_STAGES = 5;

/** Characters that are refused anywhere in the line, quoted or not. */
// biome-ignore lint/suspicious/noControlCharactersInRegex: control bytes are refused on purpose
const FORBIDDEN_ANYWHERE = /[\u0000-\u001f\u007f$`\\!]/;
/** Characters that need quotes: they redirect, chain, group, expand or comment. */
const FORBIDDEN_UNQUOTED = /[;&<>(){}[\]~#]/;
/** File name patterns: allowed unquoted only for programs in {@link GLOB_SAFE}. */
const GLOB = /[*?]/;
/**
 * Programs that only print names, sizes or counts, so a pattern may expand to any file in the
 * folder: whatever it matches (a credential, or a name that starts with `-`) is not read or run.
 */
const GLOB_SAFE: ReadonlySet<string> = new Set(['ls', 'wc', 'file', 'stat', 'du']);
/** Hiding error messages writes nothing; allowed unquoted, as a word of its own. */
const DISCARD_ERRORS = '2>/dev/null';
/** Names that hold credentials; an argument naming one is never auto-run. */
const CREDENTIAL =
  /(^|\/)(\.env[^/]*|\.ssh|\.aws|\.gnupg|\.netrc|\.npmrc|\.pypirc|\.kube|\.docker|\.git-credentials|id_(rsa|ed25519|ecdsa|dsa)[^/]*|[^/]*\.(pem|key|p12|pfx|kdbx)|[^/]*(secret|credential|password|token)[^/]*)(\/|$)/i;

interface Stage {
  readonly program: string;
  readonly args: readonly string[];
  /** Whether an argument has an unquoted `*` or `?` that the shell expands to file names. */
  readonly globbed: boolean;
}

/** Splits a line into pipeline stages of words, or `undefined` when it uses anything beyond that. */
function parsePipeline(line: string): Stage[] | undefined {
  if (line.trim() === '' || line.length > MAX_LENGTH || FORBIDDEN_ANYWHERE.test(line)) {
    return undefined;
  }
  const stages: { words: string[]; globbed: boolean }[] = [{ words: [], globbed: false }];
  const current = () => stages[stages.length - 1] ?? { words: [], globbed: false };
  let word: string | undefined;
  let quote: '"' | "'" | undefined;
  const endWord = () => {
    if (word !== undefined) current().words.push(word);
    word = undefined;
  };
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index] ?? '';
    if (quote !== undefined) {
      if (character === quote) quote = undefined;
      else word = (word ?? '') + character;
      continue;
    }
    if (word === undefined && line.startsWith(DISCARD_ERRORS, index)) {
      const after = line[index + DISCARD_ERRORS.length];
      if (after === undefined || after === '|' || /\s/.test(after)) {
        index += DISCARD_ERRORS.length - 1;
        continue;
      }
    }
    if (character === '"' || character === "'") {
      quote = character;
      word = word ?? '';
    } else if (character === '|') {
      endWord();
      if (current().words.length === 0) return undefined;
      stages.push({ words: [], globbed: false });
    } else if (/\s/.test(character)) {
      endWord();
    } else if (FORBIDDEN_UNQUOTED.test(character)) {
      return undefined;
    } else {
      if (GLOB.test(character)) current().globbed = true;
      word = (word ?? '') + character;
    }
  }
  if (quote !== undefined) return undefined;
  endWord();
  if (stages.length > MAX_STAGES) return undefined;
  const result: Stage[] = [];
  for (const { words, globbed } of stages) {
    const [program, ...args] = words;
    if (program === undefined) return undefined;
    result.push({ program, args, globbed });
  }
  return result;
}

/** Whether an argument could name something outside the working folder or a credential. */
function reachesOutside(argument: string): boolean {
  const value = argument.includes('=') ? argument.slice(argument.indexOf('=') + 1) : argument;
  return (
    value.startsWith('/') ||
    value.startsWith('~') ||
    value.split('/').includes('..') ||
    CREDENTIAL.test(value)
  );
}

const optionsOf = (args: readonly string[]): string[] => args.filter((arg) => arg.startsWith('-'));
const positionalsOf = (args: readonly string[]): string[] =>
  args.filter((arg) => !arg.startsWith('-'));
const hasAny = (args: readonly string[], blocked: readonly string[]): boolean =>
  args.some(
    (arg) => blocked.includes(arg) || blocked.some((b) => b.length > 2 && arg.startsWith(`${b}=`)),
  );

/** `sed` may only print or delete lines or substitute; no files written, no commands run. */
function safeSedScript(script: string): boolean {
  if (/^(?:\d+|\$)?(?:,(?:\d+|\$))?[pd]$/.test(script)) return true;
  return /^(?:\d+|\$)?(?:,(?:\d+|\$))?s([^\w\s\\])(?:(?!\1).)*\1(?:(?!\1).)*\1[gIpi\d]*$/.test(
    script,
  );
}

interface Rule {
  /** Whether the program reads the terminal when it is given no file, which would hang. */
  readonly needs: number;
  readonly allowed: (args: readonly string[]) => boolean;
}

const anyArgs = () => true;

const RULES: Readonly<Record<string, Rule>> = {
  ls: { needs: 0, allowed: anyArgs },
  pwd: { needs: 0, allowed: anyArgs },
  cat: { needs: 1, allowed: anyArgs },
  head: { needs: 1, allowed: anyArgs },
  tail: {
    needs: 1,
    allowed: (args) => !hasAny(args, ['-f', '-F', '--follow']),
  },
  wc: { needs: 1, allowed: anyArgs },
  file: { needs: 1, allowed: anyArgs },
  stat: { needs: 1, allowed: anyArgs },
  du: { needs: 0, allowed: anyArgs },
  df: { needs: 0, allowed: anyArgs },
  cut: { needs: 1, allowed: anyArgs },
  tr: { needs: 0, allowed: anyArgs },
  sort: {
    needs: 1,
    allowed: (args) => !hasAny(args, ['-o', '--output', '-T', '--compress-program']),
  },
  uniq: { needs: 1, allowed: (args) => positionalsOf(args).length <= 1 },
  grep: { needs: 2, allowed: anyArgs },
  egrep: { needs: 2, allowed: anyArgs },
  fgrep: { needs: 2, allowed: anyArgs },
  rg: { needs: 1, allowed: (args) => !hasAny(args, ['--pre', '--hostname-bin']) },
  find: {
    needs: 0,
    allowed: (args) =>
      !hasAny(args, [
        '-exec',
        '-execdir',
        '-ok',
        '-okdir',
        '-delete',
        '-fprint',
        '-fprint0',
        '-fprintf',
        '-fls',
      ]),
  },
  sed: {
    needs: 2,
    allowed: (args) => {
      const options = optionsOf(args);
      if (!options.every((o) => ['-n', '-E', '-r', '--quiet', '--silent'].includes(o)))
        return false;
      const script = positionalsOf(args)[0];
      return script !== undefined && safeSedScript(script);
    },
  },
  pdftotext: {
    needs: 2,
    allowed: (args) => {
      const takesValue = ['-f', '-l', '-enc', '-eol'];
      const flags = ['-layout', '-raw', '-nopgbrk', '-q'];
      const positionals: string[] = [];
      for (let index = 0; index < args.length; index += 1) {
        const arg = args[index] ?? '';
        if (takesValue.includes(arg)) index += 1;
        else if (flags.includes(arg)) continue;
        else if (arg === '-') positionals.push(arg);
        else if (arg.startsWith('-')) return false;
        else positionals.push(arg);
      }
      // Output must be stdout: `pdftotext in.pdf -` (a second plain name would be a file written).
      return positionals.length === 2 && positionals[1] === '-';
    },
  },
};

/** Whether `line` is a pipeline of read-only programs that stays inside the working folder. */
export function isReadOnlyCommand(line: string): boolean {
  const stages = parsePipeline(line);
  if (stages === undefined) return false;
  return stages.every((stage, index) => {
    const rule = Object.hasOwn(RULES, stage.program) ? RULES[stage.program] : undefined;
    if (rule === undefined) return false;
    if (stage.globbed && !GLOB_SAFE.has(stage.program)) return false;
    if (stage.args.some(reachesOutside)) return false;
    if (!rule.allowed(stage.args)) return false;
    // Only the first stage reads the terminal when it has nothing to read; later stages read the pipe.
    if (index === 0 && stage.program !== 'pdftotext') {
      return positionalsOf(stage.args).length >= rule.needs;
    }
    return true;
  });
}
