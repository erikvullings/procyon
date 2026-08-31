import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const [, , format, ...rawArgs] = process.argv;

/** @param {string[]} args */
function parseOptions(args) {
  /** @type {Record<string, string>} */
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!flag?.startsWith('--') || value === undefined) {
      throw new Error(`Expected --name value pairs, received: ${args.join(' ')}`);
    }
    options[flag.slice(2)] = value;
  }
  return options;
}

const options = parseOptions(rawArgs);
for (const name of ['version', 'sha256', 'repository', 'asset', 'output']) {
  if (!options[name]) throw new Error(`Missing required option --${name}`);
}

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(options.version)) {
  throw new Error(`Invalid release version: ${options.version}`);
}
if (!/^[0-9a-f]{64}$/i.test(options.sha256)) {
  throw new Error('SHA-256 must contain exactly 64 hexadecimal characters');
}
if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(options.repository)) {
  throw new Error(`Invalid GitHub repository: ${options.repository}`);
}
if (!/^[A-Za-z0-9_.+-]+$/.test(options.asset)) {
  throw new Error(`Invalid release asset name: ${options.asset}`);
}

const assetUrl = `https://github.com/${options.repository}/releases/download/v${options.version}/${options.asset}`;

function generateHomebrew() {
  const cask = `cask "procyon" do
  version "${options.version}"
  sha256 "${options.sha256.toLowerCase()}"

  url "${assetUrl}"
  name "Procyon"
  desc "Dual-pane file manager"
  homepage "https://github.com/${options.repository}"

  app "Procyon.app"
  binary "#{appdir}/Procyon.app/Contents/MacOS/Procyon", target: "procyon"
end
`;
  mkdirSync(dirname(options.output), { recursive: true });
  writeFileSync(options.output, cask, 'utf8');
}

function generateChocolatey() {
  // Chocolatey rejects the hyphenated pre-release suffix our other release artifacts use
  // (e.g. "0.1.0-6") - it wants dotted numeric identifiers instead ("0.1.0.6").
  const chocolateyVersion = options.version.replace('-', '.');
  const nuspec = `<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2011/10/nuspec.xsd">
  <metadata>
    <id>procyon</id>
    <version>${chocolateyVersion}</version>

    <title>Procyon</title>
    <authors>Erik Vullings</authors>
    <owners>Erik Vullings</owners>

    <requireLicenseAcceptance>false</requireLicenseAcceptance>

    <projectUrl>https://github.com/${options.repository}</projectUrl>
    <packageSourceUrl>https://github.com/${options.repository}</packageSourceUrl>
    <projectSourceUrl>https://github.com/${options.repository}</projectSourceUrl>
    <docsUrl>https://github.com/${options.repository}#readme</docsUrl>
    <bugTrackerUrl>https://github.com/${options.repository}/issues</bugTrackerUrl>

    <licenseUrl>https://github.com/${options.repository}/blob/main/LICENSE</licenseUrl>

    <iconUrl>https://raw.githubusercontent.com/${options.repository}/main/apps/fm-desktop/src-tauri/icons/icon.png</iconUrl>

    <summary>
      Dual-pane file manager for Windows and macOS.
    </summary>

    <description>
      Procyon is a modern dual-pane file manager built with Tauri.
      It provides efficient file navigation and file management with
      native desktop hosts for Windows and macOS.
    </description>

    <tags>
      file-manager dual-pane explorer tauri windows macos productivity
    </tags>

    <releaseNotes>
      https://github.com/${options.repository}/releases/tag/v${options.version}
    </releaseNotes>
  </metadata>
</package>
`;
  const install = `$ErrorActionPreference = 'Stop'

$packageArgs = @{
  packageName    = 'procyon'
  fileType       = 'exe'
  url64bit       = '${assetUrl}'
  checksum64     = '${options.sha256.toLowerCase()}'
  checksumType64 = 'sha256'
  silentArgs     = '/S'
  validExitCodes = @(0)
  softwareName   = 'Procyon*'
}

Install-ChocolateyPackage @packageArgs
`;
  const toolsDir = join(options.output, 'tools');
  mkdirSync(toolsDir, { recursive: true });
  writeFileSync(join(options.output, 'procyon.nuspec'), nuspec, 'utf8');
  writeFileSync(join(toolsDir, 'chocolateyinstall.ps1'), install, 'utf8');
}

if (format === 'homebrew') generateHomebrew();
else if (format === 'chocolatey') generateChocolatey();
else throw new Error(`Expected package format "homebrew" or "chocolatey", received: ${format}`);
