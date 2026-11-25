import { describe, expect, it } from 'vitest';
import { effectsOf, isReadOnlyScriptCommand, scanScript, scriptReadsOnly } from './command-effects';

const site = { cwd: '/work/repo', home: '/home/me' };

describe('effects of shell lines', () => {
  it('lists programs, writes, deletes, network and privilege', () => {
    const effects = effectsOf(
      'git pull && rm -rf build dist && cp a.txt out/ && sudo make install > log.txt',
      site,
    );
    expect(effects.readable).toBe(true);
    expect(effects.programs).toEqual(['git', 'rm', 'cp', 'make']);
    expect(effects.deletes).toEqual(['build', 'dist']);
    expect(effects.writes).toEqual(['out/', 'log.txt']);
    expect(effects.network).toBe(true);
    expect(effects.privilege).toBe(true);
  });

  it('reads code given to a shell with -c, and notices downloads', () => {
    const effects = effectsOf(
      `bash -c 'curl -o x.tgz https://example.com/x.tgz; mv x.tgz y/'`,
      site,
    );
    expect(effects.network).toBe(true);
    expect(effects.writes).toContain('(a downloaded file)');
    expect(effects.deletes).toEqual(['x.tgz']);
  });

  it('says so when the line cannot be read', () => {
    expect(effectsOf(`echo 'open`, site).readable).toBe(false);
  });
});

describe('script scans', () => {
  const python = (code: string) => scanScript('python', code, { known: true, cwd: site.cwd });
  const node = (code: string) => scanScript('node', code, { known: true, cwd: site.cwd });

  it('finds a read-only python script read-only', () => {
    const script = python(
      'import os, json\nfrom pathlib import Path\nfor p in Path(".").glob("*.json"):\n    data = json.loads(p.read_text())\n    print(p, len(data))\nsys.stdout.write("done")',
    );
    expect(script.writes).toBe(false);
    expect(script.modules).toEqual([]);
    expect(scriptReadsOnly(script)).toBe(true);
  });

  it('finds writes, deletes, network, subprocesses and dynamic code', () => {
    expect(python("open('out.txt', 'w').write('x')").writes).toBe(true);
    expect(python('open(p, mode)').writes).toBe(true);
    expect(python("df.to_csv('x.csv')").writes).toBe(true);
    expect(python("import shutil\nshutil.rmtree('build')").deletes).toBe(true);
    expect(python("import requests\nrequests.get('https://x')").network).toBe(true);
    expect(python("import subprocess\nsubprocess.run(['ls'])").subprocess).toBe(true);
    expect(python("exec(open('x.py').read())").dynamicCode).toBe(true);
    expect(python("getattr(os, 'sys' + 'tem')('ls')").dynamicCode).toBe(true);
    expect(python("print(open('/etc/passwd').read())").outsideFolder).toBe(true);
    expect(python("print(os.environ['HOME'])").outsideFolder).toBe(true);
    expect(python("print(open('.env').read())").outsideFolder).toBe(true);
    expect(python('import torch').modules).toEqual(['torch']);
  });

  it('reads node the same way', () => {
    expect(
      scriptReadsOnly(node("const fs = require('fs'); console.log(fs.readdirSync('.'))")),
    ).toBe(true);
    expect(node("require('fs').writeFileSync('x', 'y')").writes).toBe(true);
    expect(node("require('child_process').execSync('ls')").subprocess).toBe(true);
    expect(node("fetch('https://x')").network).toBe(true);
    expect(node("eval('1+1')").dynamicCode).toBe(true);
    expect(node("import x from 'left-pad'").modules).toEqual(['left-pad']);
  });
});

describe('read-only script commands run without asking', () => {
  it('accepts inline code and script files that only read', () => {
    expect(isReadOnlyScriptCommand(`python3 -c "import os; print(os.listdir('.'))"`, site)).toBe(
      true,
    );
    expect(
      isReadOnlyScriptCommand('python3 count.py data.csv', site, [
        {
          path: 'count.py',
          text: 'import sys\nprint(sum(1 for _ in open(sys.argv[1])))',
          truncated: false,
        },
      ]),
    ).toBe(true);
    expect(isReadOnlyScriptCommand(`node -e "console.log(1)"`, site)).toBe(true);
  });

  it('refuses anything it cannot see in full, or that does more than read', () => {
    // The script file was not given, or only partly.
    expect(isReadOnlyScriptCommand('python3 count.py', site)).toBe(false);
    expect(
      isReadOnlyScriptCommand('python3 count.py', site, [
        { path: 'count.py', text: 'print(1)', truncated: true },
      ]),
    ).toBe(false);
    // Writes, a pipe, a redirect, an argument outside the folder, a credential.
    expect(isReadOnlyScriptCommand(`python3 -c "open('x','w')"`, site)).toBe(false);
    expect(isReadOnlyScriptCommand(`python3 -c "print(1)" | sh`, site)).toBe(false);
    expect(isReadOnlyScriptCommand(`python3 -c "print(1)" > out.txt`, site)).toBe(false);
    expect(
      isReadOnlyScriptCommand('python3 count.py /etc/passwd', site, [
        { path: 'count.py', text: 'import sys\nprint(open(sys.argv[1]).read())', truncated: false },
      ]),
    ).toBe(false);
    expect(
      isReadOnlyScriptCommand('python3 count.py .env', site, [
        { path: 'count.py', text: 'import sys\nprint(open(sys.argv[1]).read())', truncated: false },
      ]),
    ).toBe(false);
    // Code built from a variable cannot be read.
    expect(isReadOnlyScriptCommand('python3 -c "$CODE"', site)).toBe(false);
    // Another program entirely.
    expect(isReadOnlyScriptCommand('make', site)).toBe(false);
  });
});
