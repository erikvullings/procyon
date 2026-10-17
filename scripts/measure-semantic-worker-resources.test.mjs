import assert from 'node:assert/strict';
import test from 'node:test';
import {
  defaultRuntimeDirectory,
  formatBytes,
  isInsideDirectory,
  openTempBytesFromLsof,
  parseArguments,
  parsePsTable,
  parseWindowsProcessCsv,
  processRssBreakdown,
  processTree,
  run,
  samplesToCsv,
  summarizeSamples,
} from './measure-semantic-worker-resources.mjs';

test('sampling command failure does not become an empty process list', () => {
  assert.equal(run(process.execPath, ['-p', '"ready"']).trim(), 'ready');
  assert.throws(() => run(process.execPath, ['-e', 'process.exit(7)']), /exited with status 7/);
  assert.throws(() => run('procyon-nonexistent-process-list-tool', []), /could not run: ENOENT/);
});

test('default runtime directory follows each platform config root', () => {
  assert.match(
    defaultRuntimeDirectory('darwin', {}, '/Users/a'),
    /^\/Users\/a\/Library\/Application Support\/procyon\/semantic\/worker-runtime$/,
  );
  assert.equal(
    defaultRuntimeDirectory('win32', { APPDATA: 'C:\\Users\\a\\AppData\\Roaming' }, 'C:\\Users\\a'),
    'C:\\Users\\a\\AppData\\Roaming\\procyon\\semantic\\worker-runtime',
  );
  assert.equal(
    defaultRuntimeDirectory('linux', {}, '/home/a'),
    '/home/a/.config/procyon/semantic/worker-runtime',
  );
});

test('arguments are validated', () => {
  const options = parseArguments(['--pid', '42', '--interval', '250', '--temp-dir', '/tmp']);
  assert.equal(options.pid, 42);
  assert.equal(options.interval, 250);
  assert.deepEqual(options.tempDirs, ['/tmp']);
  assert.throws(() => parseArguments(['--interval', '0']));
  assert.throws(() => parseArguments(['--pid']));
  assert.throws(() => parseArguments(['--pid', '1', '--match', 'payload']));
  assert.throws(() => parseArguments(['--unknown', 'x']));
});

test('process tree includes descendants of the worker but not siblings', () => {
  const rows = parsePsTable(
    [
      '    1     0   100 /sbin/launchd',
      '  500     1  2048 /Applications/Procyon.app/Contents/MacOS/Procyon',
      '  600   500  4096 /Users/a/Library/Application Support/procyon/semantic/workers/x/payload --x',
      '  601   600  1024 /Users/a/.../payload --pdf-visual-pages-child',
      '  700   500   512 /usr/bin/other',
      'garbage line',
    ].join('\n'),
  );
  assert.equal(rows.length, 5);
  assert.equal(rows[2].rssBytes, 4096 * 1024);
  assert.match(rows[2].command, /Application Support/);
  const tree = processTree(rows, [600]);
  assert.deepEqual(tree.map((row) => row.pid).sort(), [600, 601]);
  assert.deepEqual(processTree(rows, [999]), []);
});

test('Windows process CSV uses working-set bytes', () => {
  const rows = parseWindowsProcessCsv(
    [
      '"ProcessId","ParentProcessId","WorkingSetSize","CommandLine"',
      '"10","4","8192","C:\\Program Files\\Procyon\\payload.exe --x"',
      '"11","10","4096",',
    ].join('\r\n'),
  );
  assert.deepEqual(
    rows.map(({ pid, ppid, rssBytes }) => ({ pid, ppid, rssBytes })),
    [
      { pid: 10, ppid: 4, rssBytes: 8192 },
      { pid: 11, ppid: 10, rssBytes: 4096 },
    ],
  );
});

test('peak process attribution groups worker, PDF children, and external helpers without commands', () => {
  const tree = parsePsTable(
    [
      '600 500 1024 /opt/procyon/payload --ocrmypdf-executable /opt/ocrmypdf',
      '601 600 2048 /opt/procyon/payload --pdf-text-with-visuals-child',
      '602 601 4096 /opt/homebrew/bin/python3 -m ocrmypdf private.pdf',
      '603 602 512 /opt/homebrew/bin/tesseract private.pdf',
    ].join('\n'),
  );
  const breakdown = processRssBreakdown(tree, [600]);
  assert.deepEqual(breakdown, {
    worker: { count: 1, rssBytes: 1024 * 1024 },
    pdfChild: { count: 1, rssBytes: 2048 * 1024 },
    otherHelper: { count: 2, rssBytes: 4608 * 1024 },
  });
  assert.deepEqual(processRssBreakdown(tree, [600, 601]), breakdown);
  const summary = summarizeSamples([
    { ...sample('t1', 4, 7_000_000, 0, 1000), processRssBreakdown: breakdown },
    sample('t2', 1, 100, 0, 900),
  ]);
  assert.deepEqual(summary.peakTreeProcessRss, breakdown);
  assert.doesNotMatch(JSON.stringify(summary), /private\.pdf|ocrmypdf|tesseract/);
  assert.equal(summarizeSamples([]).peakTreeProcessRss, null);
});

test('temp-directory containment is prefix-safe and platform-aware', () => {
  assert.ok(isInsideDirectory('/private/var/folders/x/T/.tmpA', ['/var/folders/x/T/'], 'darwin'));
  assert.ok(!isInsideDirectory('/var/folders/x/Tother/.tmpA', ['/var/folders/x/T'], 'darwin'));
  assert.ok(
    isInsideDirectory(
      'C:/Users/A/AppData/Local/Temp/.tmp1',
      ['c:\\users\\a\\appdata\\local\\temp'],
      'win32',
    ),
  );
  assert.ok(!isInsideDirectory('/private/tmp/x', ['/tmp'], 'linux'));
});

test('lsof output counts open regular temp files once, ignoring other descriptors', () => {
  const lsof = [
    'p600',
    'fcwd',
    'tDIR',
    'n/',
    'f3',
    'tREG',
    's1000',
    'i11',
    'n/private/var/folders/x/T/.tmpSpool',
    'f4',
    'tREG',
    's5000',
    'i12',
    'n/Users/a/Library/Application Support/procyon/semantic/zvec/data',
    'f5',
    'tPIPE',
    'n->0x1',
    'ftxt',
    'tREG',
    's9',
    'n/var/folders/x/T/loaded-library',
    'p601',
    'f7',
    'tREG',
    's1000',
    'i11',
    'n/private/var/folders/x/T/.tmpSpool',
    'f8',
    'tREG',
    's250',
    'i13',
    'n/var/folders/x/T/.tmpStage (deleted)',
  ].join('\n');
  assert.deepEqual(openTempBytesFromLsof(lsof, ['/var/folders/x/T'], 'darwin'), {
    bytes: 1250,
    count: 2,
  });
  assert.deepEqual(openTempBytesFromLsof('', ['/tmp'], 'linux'), { bytes: 0, count: 0 });
});

test('summary reports peaks and the largest free-disk drop', () => {
  const samples = [
    sample('t0', 0, 0, 0, 1000),
    sample('t1', 2, 300, 50, 900),
    sample('t2', 3, 500, 20, 700),
    sample('t3', 1, 100, 0, 950),
  ];
  const summary = summarizeSamples(samples);
  assert.equal(summary.sampleCount, 4);
  assert.equal(summary.samplesWithWorker, 3);
  assert.equal(summary.peakTreeRssBytes, 500);
  assert.equal(summary.peakTreeRssAt, 't2');
  assert.equal(summary.peakOpenTempBytes, 50);
  assert.equal(summary.peakOpenTempAt, 't1');
  assert.equal(summary.peakProcessCount, 3);
  assert.equal(summary.minimumFreeDiskBytes, 700);
  assert.equal(summary.maximumFreeDiskDropBytes, 300);
  assert.equal(summarizeSamples([]).peakTreeRssAt, null);
});

test('CSV has a header and one row per sample', () => {
  const lines = samplesToCsv([sample('t0', 1, 2, 3, 4)])
    .trim()
    .split('\n');
  assert.equal(lines.length, 2);
  assert.match(lines[0], /^timestamp,/);
  assert.match(lines[1], /^t0,/);
});

test('bytes are formatted in binary units', () => {
  assert.equal(formatBytes(512), '512 B');
  assert.equal(formatBytes(2.5 * 1024 ** 3), '2.5 GiB');
  assert.equal(formatBytes(null), 'n/a');
});

function sample(timestamp, processCount, treeRssBytes, openTempBytes, freeDiskBytes) {
  return {
    timestamp,
    processCount,
    treeRssBytes,
    largestProcessRssBytes: treeRssBytes,
    openTempBytes,
    openTempFiles: openTempBytes > 0 ? 1 : 0,
    freeDiskBytes,
  };
}
