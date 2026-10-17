// Samples the semantic worker process tree for manual Gemma qualification (task 0247).
//
// Records only numbers: resident memory, the byte size of temporary files the worker tree holds
// open (including anonymous, already-unlinked PDF spools), and free space on the temp volume.
// It never reads, copies, or records document content or temporary file names.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const USAGE = `Usage: node scripts/measure-semantic-worker-resources.mjs [options]

  --runtime-dir <dir>  Worker runtime directory containing worker.pid
                       (default: the installed desktop app's semantic/worker-runtime)
  --pid <pid>          Sample this process tree instead of reading worker.pid
  --match <text>       Sample every process whose command contains <text>, plus descendants
  --temp-dir <dir>     Temp directory holding worker spools (repeatable; default: OS temp dir)
  --interval <ms>      Sampling interval in milliseconds (default: 1000)
  --duration <s>       Stop after this many seconds (default: until Ctrl+C)
  --out <dir>          Output directory (default: target/semantic-resource-measurements/<time>)
`;

export function defaultRuntimeDirectory(platform, env, home) {
  const join = platform === 'win32' ? path.win32.join : path.posix.join;
  let config;
  if (platform === 'darwin') config = join(home, 'Library', 'Application Support');
  else if (platform === 'win32') config = env.APPDATA ?? join(home, 'AppData', 'Roaming');
  else config = env.XDG_CONFIG_HOME ?? join(home, '.config');
  return join(config, 'procyon', 'semantic', 'worker-runtime');
}

export function parseArguments(argv) {
  const options = { interval: 1000, tempDirs: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--') continue;
    if (flag === '--help' || flag === '-h') return { ...options, help: true };
    const value = argv[index + 1];
    if (value === undefined || value.startsWith('--')) throw new Error(`${flag} requires a value`);
    index += 1;
    switch (flag) {
      case '--runtime-dir':
        options.runtimeDir = value;
        break;
      case '--pid':
        options.pid = positiveInteger(flag, value);
        break;
      case '--match':
        options.match = value;
        break;
      case '--temp-dir':
        options.tempDirs.push(value);
        break;
      case '--interval':
        options.interval = positiveInteger(flag, value);
        break;
      case '--duration':
        options.duration = positiveInteger(flag, value);
        break;
      case '--out':
        options.out = value;
        break;
      default:
        throw new Error(`unknown option ${flag}`);
    }
  }
  if (options.pid !== undefined && options.match !== undefined) {
    throw new Error('--pid and --match are mutually exclusive');
  }
  return options;
}

function positiveInteger(flag, value) {
  const number = Number(value);
  if (!Number.isInteger(number) || number <= 0)
    throw new Error(`${flag} must be a positive integer`);
  return number;
}

/** Parses `ps -axo pid=,ppid=,rss=,command=` output (rss in KiB). */
export function parsePsTable(text) {
  const rows = [];
  for (const line of text.split(/\r?\n/)) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s?(.*)$/.exec(line);
    if (match) {
      rows.push({
        pid: Number(match[1]),
        ppid: Number(match[2]),
        rssBytes: Number(match[3]) * 1024,
        command: match[4].trim(),
      });
    }
  }
  return rows;
}

/** Parses PowerShell `ConvertTo-Csv` output of ProcessId, ParentProcessId, WorkingSetSize, CommandLine. */
export function parseWindowsProcessCsv(text) {
  const rows = [];
  for (const line of text.split(/\r?\n/)) {
    const match = /^"(\d+)","(\d+)","(\d+)","?(.*?)"?$/.exec(line.trim());
    if (match) {
      rows.push({
        pid: Number(match[1]),
        ppid: Number(match[2]),
        rssBytes: Number(match[3]),
        command: match[4].replaceAll('""', '"'),
      });
    }
  }
  return rows;
}

/** Returns the rows for `rootPids` and all their descendants. */
export function processTree(rows, rootPids) {
  const children = new Map();
  for (const row of rows) {
    if (!children.has(row.ppid)) children.set(row.ppid, []);
    children.get(row.ppid).push(row);
  }
  const byPid = new Map(rows.map((row) => [row.pid, row]));
  const selected = new Map();
  const pending = rootPids.filter((pid) => byPid.has(pid));
  while (pending.length > 0) {
    const pid = pending.pop();
    if (selected.has(pid)) continue;
    selected.set(pid, byPid.get(pid));
    for (const child of children.get(pid) ?? []) pending.push(child.pid);
  }
  return [...selected.values()];
}

/** Groups memory by the worker, its PDF children, and other descendant helpers. */
export function processRssBreakdown(tree, rootPids) {
  const roots = new Set(rootPids);
  const breakdown = {
    worker: { count: 0, rssBytes: 0 },
    pdfChild: { count: 0, rssBytes: 0 },
    otherHelper: { count: 0, rssBytes: 0 },
  };
  for (const row of tree) {
    const role = /--pdf-(?:text|text-with-visuals|visual|visual-pages)-child(?:\s|$)/.test(
      row.command,
    )
      ? 'pdfChild'
      : roots.has(row.pid)
        ? 'worker'
        : 'otherHelper';
    breakdown[role].count += 1;
    breakdown[role].rssBytes += row.rssBytes;
  }
  return breakdown;
}

export function normalizeForComparison(filePath, platform) {
  if (platform === 'win32') {
    return path.win32
      .normalize(filePath)
      .replace(/[\\/]+$/, '')
      .toLowerCase();
  }
  const normalized = path.posix.normalize(filePath).replace(/\/+$/, '');
  // macOS reports /var and /tmp through their /private targets.
  return platform === 'darwin' ? normalized.replace(/^\/private(?=\/)/, '') : normalized;
}

export function isInsideDirectory(filePath, directories, platform) {
  const candidate = normalizeForComparison(filePath, platform);
  const separator = platform === 'win32' ? '\\' : '/';
  return directories.some((directory) => {
    const root = normalizeForComparison(directory, platform);
    return candidate === root || candidate.startsWith(root + separator);
  });
}

/**
 * Sums regular files open on numeric descriptors inside the temp directories from
 * `lsof -n -P -F ftsn` output. Unlinked anonymous spools still report their original path.
 */
export function openTempBytesFromLsof(text, tempDirs, platform) {
  const seen = new Set();
  let bytes = 0;
  let count = 0;
  let pid = '';
  let file = {};
  const flush = () => {
    if (
      /^\d+$/.test(file.fd ?? '') &&
      file.type === 'REG' &&
      Number.isFinite(file.size) &&
      file.name !== undefined &&
      isInsideDirectory(file.name.replace(/ \(deleted\)$/, ''), tempDirs, platform)
    ) {
      // A descriptor inherited by a child refers to the same file; count it once.
      const key = `${file.name}\u0000${file.size}\u0000${file.inode ?? `${pid}:${file.fd}`}`;
      if (!seen.has(key)) {
        seen.add(key);
        bytes += file.size;
        count += 1;
      }
    }
    file = {};
  };
  for (const line of text.split(/\r?\n/)) {
    const tag = line[0];
    const value = line.slice(1);
    if (tag === 'p') {
      flush();
      pid = value;
    } else if (tag === 'f') {
      flush();
      file.fd = value;
    } else if (tag === 't') file.type = value;
    else if (tag === 's') file.size = Number(value);
    else if (tag === 'i') file.inode = value;
    else if (tag === 'n') file.name = value;
  }
  flush();
  return { bytes, count };
}

export function summarizeSamples(samples) {
  const summary = {
    sampleCount: samples.length,
    samplesWithWorker: 0,
    startedAt: samples[0]?.timestamp ?? null,
    endedAt: samples.at(-1)?.timestamp ?? null,
    peakTreeRssBytes: 0,
    peakTreeRssAt: null,
    peakTreeProcessRss: null,
    peakSingleProcessRssBytes: 0,
    peakProcessCount: 0,
    peakOpenTempBytes: 0,
    peakOpenTempAt: null,
    peakOpenTempFiles: 0,
    baselineFreeDiskBytes: samples[0]?.freeDiskBytes ?? null,
    minimumFreeDiskBytes: null,
    maximumFreeDiskDropBytes: 0,
  };
  for (const sample of samples) {
    if (sample.processCount > 0) summary.samplesWithWorker += 1;
    if (sample.treeRssBytes > summary.peakTreeRssBytes) {
      summary.peakTreeRssBytes = sample.treeRssBytes;
      summary.peakTreeRssAt = sample.timestamp;
      summary.peakTreeProcessRss = sample.processRssBreakdown ?? null;
    }
    summary.peakSingleProcessRssBytes = Math.max(
      summary.peakSingleProcessRssBytes,
      sample.largestProcessRssBytes,
    );
    summary.peakProcessCount = Math.max(summary.peakProcessCount, sample.processCount);
    if (sample.openTempBytes > summary.peakOpenTempBytes) {
      summary.peakOpenTempBytes = sample.openTempBytes;
      summary.peakOpenTempAt = sample.timestamp;
    }
    summary.peakOpenTempFiles = Math.max(summary.peakOpenTempFiles, sample.openTempFiles);
    if (Number.isFinite(sample.freeDiskBytes)) {
      if (
        summary.minimumFreeDiskBytes === null ||
        sample.freeDiskBytes < summary.minimumFreeDiskBytes
      ) {
        summary.minimumFreeDiskBytes = sample.freeDiskBytes;
      }
      if (Number.isFinite(summary.baselineFreeDiskBytes)) {
        summary.maximumFreeDiskDropBytes = Math.max(
          summary.maximumFreeDiskDropBytes,
          summary.baselineFreeDiskBytes - sample.freeDiskBytes,
        );
      }
    }
  }
  return summary;
}

export const CSV_COLUMNS = [
  'timestamp',
  'processCount',
  'treeRssBytes',
  'largestProcessRssBytes',
  'openTempBytes',
  'openTempFiles',
  'freeDiskBytes',
];

export function samplesToCsv(samples) {
  const lines = [CSV_COLUMNS.join(',')];
  for (const sample of samples)
    lines.push(CSV_COLUMNS.map((column) => sample[column] ?? '').join(','));
  return `${lines.join('\n')}\n`;
}

export function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return 'n/a';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let value = Math.abs(bytes);
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${bytes < 0 ? '-' : ''}${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.error)
    throw new Error(`${command} could not run: ${result.error.code ?? result.error}`);
  if (result.status !== 0) {
    throw new Error(
      `${command} exited with ${result.signal ? `signal ${result.signal}` : `status ${result.status}`}`,
    );
  }
  return result.stdout;
}

function listProcesses() {
  if (process.platform === 'win32') {
    return parseWindowsProcessCsv(
      run('powershell.exe', [
        '-NoProfile',
        '-Command',
        'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize,CommandLine | ConvertTo-Csv -NoTypeInformation',
      ]),
    );
  }
  return parsePsTable(run('ps', ['-axo', 'pid=,ppid=,rss=,command=']));
}

function readRootPids(options, rows) {
  if (options.pid !== undefined) return [options.pid];
  if (options.match !== undefined) {
    return rows
      .filter((row) => row.pid !== process.pid && row.command.includes(options.match))
      .map((row) => row.pid);
  }
  try {
    const pid = Number(fs.readFileSync(path.join(options.runtimeDir, 'worker.pid'), 'utf8').trim());
    return Number.isInteger(pid) && pid > 0 ? [pid] : [];
  } catch {
    return [];
  }
}

function openTempUsage(pids, tempDirs) {
  if (pids.length === 0) return { bytes: 0, count: 0 };
  if (process.platform === 'linux') {
    let lsofText = '';
    for (const pid of pids) {
      let descriptors = [];
      try {
        descriptors = fs.readdirSync(`/proc/${pid}/fd`);
      } catch {
        continue;
      }
      for (const fd of descriptors) {
        try {
          const target = fs.readlinkSync(`/proc/${pid}/fd/${fd}`);
          const stat = fs.statSync(`/proc/${pid}/fd/${fd}`);
          if (stat.isFile()) {
            lsofText += `p${pid}\nf${fd}\ntREG\ns${stat.size}\ni${stat.dev}:${stat.ino}\nn${target}\n`;
          }
        } catch {}
      }
    }
    return openTempBytesFromLsof(lsofText, tempDirs, 'linux');
  }
  if (process.platform === 'win32') {
    // Windows spools are named delete-on-close `.tmp*` files directly in the temp directory.
    let bytes = 0;
    let count = 0;
    for (const directory of tempDirs) {
      let entries = [];
      try {
        entries = fs.readdirSync(directory, { withFileTypes: true });
      } catch {
        continue;
      }
      for (const entry of entries) {
        if (!entry.isFile() || !entry.name.startsWith('.tmp')) continue;
        try {
          bytes += fs.statSync(path.join(directory, entry.name)).size;
          count += 1;
        } catch {}
      }
    }
    return { bytes, count };
  }
  return openTempBytesFromLsof(
    run('lsof', ['-n', '-P', '-F', 'ftsin', '-p', pids.join(',')]),
    tempDirs,
    process.platform,
  );
}

function freeDiskBytes(directory) {
  try {
    const stats = fs.statfsSync(directory);
    return stats.bavail * stats.bsize;
  } catch {
    return null;
  }
}

function takeSample(options, tempDirs) {
  const rows = listProcesses();
  const rootPids = readRootPids(options, rows);
  const tree = processTree(rows, rootPids);
  const temp = openTempUsage(
    tree.map((row) => row.pid),
    tempDirs,
  );
  return {
    timestamp: new Date().toISOString(),
    processCount: tree.length,
    treeRssBytes: tree.reduce((sum, row) => sum + row.rssBytes, 0),
    processRssBreakdown: processRssBreakdown(tree, rootPids),
    largestProcessRssBytes: tree.reduce((max, row) => Math.max(max, row.rssBytes), 0),
    openTempBytes: temp.bytes,
    openTempFiles: temp.count,
    freeDiskBytes: freeDiskBytes(tempDirs[0]),
  };
}

function readBackend(runtimeDir) {
  try {
    return fs.readFileSync(path.join(runtimeDir, 'gemma-backend'), 'utf8').trim() || null;
  } catch {
    return null;
  }
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(USAGE);
    return;
  }
  options.runtimeDir ??= defaultRuntimeDirectory(process.platform, process.env, os.homedir());
  const tempDirs = options.tempDirs.length > 0 ? options.tempDirs : [os.tmpdir()];
  const realTempDirs = [
    ...new Set(
      tempDirs.flatMap((directory) => {
        try {
          return [directory, fs.realpathSync(directory)];
        } catch {
          return [directory];
        }
      }),
    ),
  ];
  const out =
    options.out ??
    path.join(
      'target',
      'semantic-resource-measurements',
      new Date().toISOString().replaceAll(':', '-'),
    );
  fs.mkdirSync(out, { recursive: true });
  const samples = [];
  const started = Date.now();
  let stopping = false;
  process.on('SIGINT', () => {
    stopping = true;
  });
  console.log(
    `Sampling every ${options.interval} ms (runtime ${options.runtimeDir}, temp ${tempDirs.join(', ')}). Press Ctrl+C to stop.`,
  );
  while (!stopping) {
    const sample = takeSample(options, realTempDirs);
    samples.push(sample);
    console.log(
      `${sample.timestamp} processes=${sample.processCount} rss=${formatBytes(sample.treeRssBytes)} temp=${formatBytes(sample.openTempBytes)} free=${formatBytes(sample.freeDiskBytes)}`,
    );
    if (options.duration !== undefined && Date.now() - started >= options.duration * 1000) break;
    await new Promise((resolve) => setTimeout(resolve, options.interval));
  }
  const summary = {
    ...summarizeSamples(samples),
    platform: `${process.platform}-${process.arch}`,
    intervalMs: options.interval,
    runtimeDirectory: options.runtimeDir,
    tempDirectories: tempDirs,
    gemmaBackend: readBackend(options.runtimeDir),
    note: 'Sampled values; short-lived peaks between samples are not observed.',
  };
  fs.writeFileSync(path.join(out, 'samples.csv'), samplesToCsv(samples));
  fs.writeFileSync(path.join(out, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
  console.log(
    `Peak worker-tree RSS ${formatBytes(summary.peakTreeRssBytes)}, peak open temp ${formatBytes(summary.peakOpenTempBytes)}, max free-disk drop ${formatBytes(summary.maximumFreeDiskDropBytes)}.`,
  );
  console.log(`Wrote ${path.join(out, 'samples.csv')} and ${path.join(out, 'summary.json')}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error.message);
    console.error(USAGE);
    process.exitCode = 1;
  });
}
