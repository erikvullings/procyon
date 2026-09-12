import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { load } from 'js-yaml';

import { installNativeRuntimeAlias } from './build-semantic-developer-bundle.mjs';

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

function read(...segments) {
  return readFileSync(join(repoRoot, ...segments), 'utf8');
}

function workflow(name) {
  return load(read('.github', 'workflows', name));
}

function workflowText(name) {
  return read('.github', 'workflows', name);
}

/// Scratch space inside the repository, never a shared temporary directory.
function scratchDirectory(prefix) {
  const parent = join(repoRoot, 'target', 'desktop-packaging-tests');
  mkdirSync(parent, { recursive: true });
  const directory = join(parent, `${prefix}${process.pid}`);
  rmSync(directory, { force: true, recursive: true });
  mkdirSync(directory, { recursive: true });
  return directory;
}

test('desktop Cargo metadata and Tauri bootstrap config agree on product identity and icons', () => {
  const workspaceCargo = read('Cargo.toml');
  const cargo = read('apps', 'fm-desktop', 'src-tauri', 'Cargo.toml');
  const config = JSON.parse(read('apps', 'fm-desktop', 'src-tauri', 'tauri.conf.json'));
  const workspaceVersion = workspaceCargo.match(/^version = "([^"]+)"$/m)?.[1];

  assert.match(cargo, /version\.workspace = true/);
  assert.match(cargo, /\[package\.metadata\.desktop\]/);
  assert.match(cargo, /product-name = "Procyon"/);
  assert.match(cargo, /identifier = "nl\.erikvullings\.procyon"/);
  assert.match(cargo, /icons = \[/);
  assert.equal(config.productName, 'Procyon');
  assert.equal(config.mainBinaryName, 'Procyon');
  assert.equal(config.identifier, 'nl.erikvullings.procyon');

  const derived = JSON.parse(
    execFileSync('node', ['scripts/build-tauri.mjs', '--print-config'], {
      cwd: repoRoot,
      encoding: 'utf8',
    }),
  );
  assert.equal(derived.productName, config.productName);
  assert.equal(derived.version, workspaceVersion);
  assert.equal(derived.identifier, config.identifier);
  assert.deepEqual(derived.bundle.icon, config.bundle.icon);
});

test('the root Tauri build command uses the metadata-derived build wrapper', () => {
  const rootPackage = JSON.parse(read('package.json'));
  assert.equal(rootPackage.scripts['build:tauri'], 'node scripts/build-tauri.mjs');
});

test('Tauri targets installable macOS, Windows, and Linux bundle formats', () => {
  const config = JSON.parse(read('apps', 'fm-desktop', 'src-tauri', 'tauri.conf.json'));
  assert.deepEqual(config.bundle.targets, ['app', 'dmg', 'msi', 'nsis', 'deb', 'appimage']);
});

test('desktop archive support statically links liblzma', () => {
  const workspaceCargo = read('Cargo.toml');
  assert.match(workspaceCargo, /xz2 = \{version = "0\.1", features = \["static"\]\}/);
});

test('pull-request CI builds desktop bundles without any signing credentials', () => {
  const ciText = workflowText('ci.yml');
  const ci = workflow('ci.yml');
  assert.deepEqual([...ci.jobs.desktop.strategy.matrix.os].sort(), [
    'macos-latest',
    'windows-latest',
  ]);
  assert.match(JSON.stringify(ci.jobs.desktop), /build:tauri/);
  assert.doesNotMatch(ciText, /APPLE_|WINDOWS_|CERTIFICATE|SIGNING|notariz/i);
});

test('protected release workflow signs and notarizes macOS packages only', () => {
  const releaseText = workflowText('release-desktop.yml');
  const release = workflow('release-desktop.yml');
  assert.deepEqual(release.on.push.tags, ['v*']);
  assert.ok(release.on.workflow_dispatch);
  assert.equal(release.on.pull_request, undefined);

  for (const jobName of ['macos', 'windows', 'linux']) {
    assert.equal(release.jobs[jobName].environment, 'desktop-release');
  }

  assert.match(releaseText, /secrets\.APPLE_CERTIFICATE/);
  assert.match(releaseText, /secrets\.APPLE_CERTIFICATE_PASSWORD/);
  assert.match(releaseText, /secrets\.APPLE_API_ISSUER/);
  assert.match(releaseText, /secrets\.APPLE_API_KEY/);
  assert.match(releaseText, /secrets\.APPLE_API_KEY_P8/);
  assert.match(releaseText, /apple-actions\/import-codesign-certs@v7/);
  assert.match(releaseText, /DeveloperIDG2CA\.cer/);
  assert.match(releaseText, /f16cd3c54c7f83cea4bf1a3e6a0819c8aaa8e4a1528fd144715f350643d2df3a/);
  assert.match(releaseText, /Developer ID Application/);
  assert.match(releaseText, /codesign --verify/);
  assert.match(releaseText, /spctl --assess/);
  assert.match(releaseText, /stapler validate/);
  assert.match(releaseText, /--timeout 45m/);
  assert.match(releaseText, /--no-s3-acceleration/);
  assert.match(releaseText, /otool -L/);
  assert.match(releaseText, /Mach-O/);
  assert.match(releaseText, /System\/Library/);
  assert.match(releaseText, /external build-machine dependencies/);
  assert.equal(release.jobs.macos['timeout-minutes'], 120);

  const buildStep = release.jobs.macos.steps.find((step) =>
    /Build signed macOS bundles/.test(step.name ?? ''),
  );
  assert.ok(buildStep, 'expected a signed macOS build step');
  assert.equal(buildStep.env?.APPLE_API_ISSUER, undefined);
  assert.equal(buildStep.env?.APPLE_API_KEY, undefined);
  assert.doesNotMatch(releaseText, /WINDOWS_CERTIFICATE|signtool/i);
});

test('release workflow publishes signed macOS and unsigned Windows and Linux packages', () => {
  const releaseText = workflowText('release-desktop.yml');
  const release = workflow('release-desktop.yml');
  const chocolateyText = workflowText('publish-chocolatey.yml');
  const chocolatey = workflow('publish-chocolatey.yml');

  assert.match(releaseText, /build:tauri --target universal-apple-darwin/);
  assert.equal(release.jobs.linux['runs-on'], 'ubuntu-22.04');
  assert.match(releaseText, /libwebkit2gtk-4\.1-dev/);
  assert.match(releaseText, /bundle\/deb\/\*\.deb/);
  assert.match(releaseText, /bundle\/appimage\/\*\.AppImage/);
  assert.deepEqual(release.jobs.homebrew.needs, ['macos', 'linux']);
  assert.equal(release.jobs.homebrew.environment, 'desktop-release');
  assert.equal(release.jobs.chocolatey.needs, 'windows');
  assert.equal(release.jobs.chocolatey.uses, './.github/workflows/publish-chocolatey.yml');
  assert.equal(release.jobs.chocolatey.with.release_tag, `\${{ github.ref_name }}`);
  assert.equal(chocolatey.jobs.chocolatey.environment, 'desktop-release');
  assert.ok(chocolatey.on.workflow_dispatch);
  assert.ok(chocolatey.on.workflow_call);
  assert.match(releaseText, /vars\.HOMEBREW_TAP_REPOSITORY/);
  assert.match(releaseText, /secrets\.HOMEBREW_TAP_TOKEN/);
  assert.match(chocolateyText, /secrets\.CHOCOLATEY_API_KEY/);
  assert.match(chocolateyText, /choco pack/);
  assert.match(chocolateyText, /choco push/);
});

test('release workflow builds, verifies, signs, and publishes optional semantic payloads separately', () => {
  const releaseText = workflowText('release-desktop.yml');
  const release = workflow('release-desktop.yml');
  const smokeScript = read('scripts', 'smoke-semantic-production-bundle.mjs');
  const payloads = release.jobs['semantic-payloads'];
  const catalogs = release.jobs['semantic-catalogs'];
  const collect = release.jobs['semantic-collect'];
  const publish = release.jobs['semantic-publish'];

  assert.ok(payloads, 'expected a semantic payload build matrix');
  assert.match(payloads.if, /github\.event_name == 'workflow_dispatch'/);
  assert.match(payloads.if, /vars\.SEMANTIC_RELEASE_QUALIFIED == 'true'/);
  assert.deepEqual(payloads.strategy.matrix.include.map(({ target }) => target).sort(), [
    'linux-aarch64',
    'linux-x86_64',
    'macos-aarch64',
    'windows-x86_64',
  ]);
  assert.match(JSON.stringify(payloads), /semantic:bundle:production/);
  assert.match(JSON.stringify(payloads), /smoke-semantic-production-bundle/);
  assert.match(smokeScript, /cargo.*test/s);
  assert.match(smokeScript, /packaged_worker_ingests_recovers_after_crash_and_reopens_offline/);
  assert.match(smokeScript, /production_model_pack_activates_offline/);
  assert.match(smokeScript, /fm-semantic-components/);
  assert.match(smokeScript, /evaluate_semantic_production/);
  assert.match(JSON.stringify(payloads), /--evaluation-report/);
  const payloadPrecondition = payloads.steps.find(
    (step) => step.name === 'Verify exact-production semantic evaluation evidence',
  );
  assert.equal(
    payloadPrecondition.if,
    "github.event_name == 'push' && vars.SEMANTIC_RELEASE_QUALIFIED == 'true'",
  );
  assert.match(payloadPrecondition.run, /check-semantic-release-preconditions\.mjs/);
  assert.equal(catalogs.uses, './.github/workflows/sign-semantic-catalog.yml');
  assert.match(catalogs.if, /always\(\)/);
  assert.equal(catalogs.secrets, 'inherit');
  assert.match(JSON.stringify(catalogs), /semantic-catalog-\$\{\{ matrix.target \}\}/);
  assert.equal(collect.permissions.contents, 'read');
  assert.deepEqual(publish.needs, ['release', 'semantic-collect']);
  assert.match(publish.if, /github\.event_name == 'push'/);
  assert.equal(publish.permissions.contents, 'write');
  assert.match(JSON.stringify(payloads), /semantic-payloads-\$\{\{ matrix.target \}\}/);
  assert.match(JSON.stringify(payloads), /check-desktop-release\.mjs/);
  assert.equal(payloads.environment, 'desktop-release');
  assert.match(JSON.stringify(payloads), /PROCYON_REQUIRE_PLATFORM_SIGNING/);
  assert.match(JSON.stringify(payloads), /PROCYON_APPLE_SIGNING_IDENTITY/);
  assert.match(JSON.stringify(payloads), /Notarize and verify macOS semantic executables/);
  assert.match(JSON.stringify(payloads), /notarytool submit/);
  assert.match(JSON.stringify(payloads), /procyon\.semantic\.worker\.\*/);
  assert.match(JSON.stringify(payloads), /procyon\.semantic\.zvec-runtime\.\*/);
  assert.match(JSON.stringify(publish), /softprops\/action-gh-release@v2/);
  const installed = release.jobs['semantic-installed-qualification'];
  assert.match(installed.if, /always\(\)/);
  assert.equal(installed.permissions.contents, 'read');
  assert.match(JSON.stringify(installed), /qualify-semantic-installed\.mjs/);
  assert.match(JSON.stringify(payloads), /retention-days.*7/);
  assert.match(JSON.stringify(installed), /retention-days.*7/);
  assert.match(JSON.stringify(collect), /aggregate-semantic-production-evaluation\.mjs/);
  assert.match(JSON.stringify(collect), /--approved-report/);
  for (const [jobName, catalogTarget] of [
    ['macos', 'macos-aarch64'],
    ['windows', 'windows-x86_64'],
    ['linux', 'linux-x86_64'],
  ]) {
    const job = release.jobs[jobName];
    assert.deepEqual(job.needs, ['release', 'semantic-catalogs']);
    assert.match(job.if, /needs\.semantic-catalogs\.result == 'skipped'/);
    assert.match(JSON.stringify(job), new RegExp(`semantic-catalog-${catalogTarget}`));
    assert.match(JSON.stringify(job), /resources\/semantic/);
    assert.match(JSON.stringify(job), /export-semantic-verifying-key\.mjs/);
    const semanticSteps = job.steps.filter(
      (step) =>
        step.name === 'Verify exact-production semantic evaluation evidence' ||
        step.name?.startsWith('Embed the signed') ||
        step.name === 'Compile the production semantic catalog trust key',
    );
    assert.equal(semanticSteps.length, 3);
    for (const step of semanticSteps) {
      assert.equal(
        step.if,
        "github.event_name == 'push' && vars.SEMANTIC_RELEASE_QUALIFIED == 'true'",
      );
    }
    assert.doesNotMatch(JSON.stringify(job), /SEMANTIC_CATALOG_SIGNING_KEY_BASE64/);
  }
  assert.match(releaseText, /vars\.SEMANTIC_CATALOG_VERIFYING_KEY_BASE64/);
  assert.doesNotMatch(
    releaseText,
    /bundle\/(?:dmg|msi|nsis|deb|appimage).*semantic|semantic.*bundle\/(?:dmg|msi|nsis|deb|appimage)/i,
  );
});

test('semantic release preconditions require a current four-target measured go', () => {
  const planned = JSON.parse(
    execFileSync('node', ['scripts/check-semantic-release-preconditions.mjs', '--print-plan'], {
      cwd: repoRoot,
      encoding: 'utf8',
    }),
  );
  assert.deepEqual(planned.arguments.slice(0, 7), [
    'run',
    '--quiet',
    '--locked',
    '-p',
    'fm-application',
    '--example',
    'validate_semantic_release_report',
  ]);

  const current = spawnSync('node', ['scripts/check-semantic-release-preconditions.mjs'], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  assert.notEqual(current.status, 0);
  assert.match(`${current.stdout}${current.stderr}`, /not a measured, four-target go/i);

  const outputRoot = scratchDirectory('semantic-evaluation-report-');
  const forged = JSON.parse(read('docs', 'evaluations', 'semantic-production-v1.json'));
  forged.decision = 'go';
  forged.productionMeasurement = true;
  forged.blockingReasons = [];
  forged.measurements = [{}, {}, {}, {}];
  const forgedPath = join(outputRoot, 'forged-go.json');
  writeFileSync(forgedPath, JSON.stringify(forged));
  const forgedResult = spawnSync(
    'node',
    ['scripts/check-semantic-release-preconditions.mjs', '--report', forgedPath],
    { cwd: repoRoot, encoding: 'utf8' },
  );
  assert.notEqual(forgedResult.status, 0);
  assert.match(`${forgedResult.stdout}${forgedResult.stderr}`, /does not support a go/i);

  const approvedMismatch = { ...forged, measurementBasis: 'different reviewed evidence' };
  const approvedPath = join(outputRoot, 'approved.json');
  writeFileSync(approvedPath, JSON.stringify(approvedMismatch));
  const mismatch = spawnSync(
    'node',
    [
      'scripts/check-semantic-release-preconditions.mjs',
      '--report',
      forgedPath,
      '--approved-report',
      approvedPath,
    ],
    { cwd: repoRoot, encoding: 'utf8' },
  );
  assert.notEqual(mismatch.status, 0);
});

test('release desktop builds fail closed without a measured knowledge-search go decision', () => {
  const release = workflow('release-desktop.yml');
  const releaseText = workflowText('release-desktop.yml');

  for (const jobName of ['macos', 'linux', 'windows']) {
    const steps = release.jobs[jobName].steps ?? [];
    const qualification = steps.findIndex((step) =>
      /export-knowledge-release-qualification\.mjs/.test(step.run ?? ''),
    );
    const preconditions = steps.findIndex((step) =>
      /check-knowledge-search-preconditions\.mjs/.test(step.run ?? ''),
    );
    const build = steps.findIndex((step) => /build:tauri/.test(step.run ?? ''));

    assert.ok(qualification >= 0, `${jobName} must record the knowledge-search decision`);
    assert.ok(preconditions >= 0, `${jobName} must verify the full-text preconditions`);
    assert.ok(build >= 0, `${jobName} must build the desktop bundle`);
    assert.ok(qualification < build, `${jobName} must decide before it builds`);
    assert.ok(preconditions < build, `${jobName} must verify before it builds`);

    // The compiled flag is decided by the script from the protected variable,
    // so the step itself is unconditional and cannot be bypassed by a dispatch.
    assert.equal(steps[qualification].if, undefined);
    assert.equal(
      steps[qualification].env?.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED,
      // biome-ignore lint/suspicious/noTemplateCurlyInString: GitHub Actions expression syntax.
      '${{ vars.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED }}',
    );
    // The expensive supported-platform checks only run for a candidate that
    // claims a measured go, so an ordinary release does no duplicate work.
    assert.equal(steps[preconditions].if, "vars.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED == 'true'");
    assert.doesNotMatch(steps[preconditions].if, /workflow_dispatch/);
  }

  // The existing semantic gate keeps its own, separate variable and behaviour.
  assert.match(releaseText, /vars\.SEMANTIC_RELEASE_QUALIFIED == 'true'/);
  assert.match(releaseText, /vars\.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED/);
});

test('knowledge qualification exporter compiles the flag only for an exact measured go', () => {
  const outputRoot = scratchDirectory('knowledge-qualification-');

  function exportQualification(value) {
    const environmentFile = join(outputRoot, `github-${value ?? 'unset'}.env`);
    writeFileSync(environmentFile, '');
    const env = { ...process.env };
    delete env.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED;
    if (value !== undefined) env.KNOWLEDGE_SEARCH_RELEASE_QUALIFIED = value;
    const stdout = execFileSync(
      'node',
      ['scripts/export-knowledge-release-qualification.mjs', environmentFile],
      { cwd: repoRoot, env, encoding: 'utf8' },
    );
    return { written: readFileSync(environmentFile, 'utf8'), stdout };
  }

  // Unset and an explicit no-go both fail closed: nothing is compiled in.
  for (const value of [undefined, '', 'false']) {
    const { written, stdout } = exportQualification(value);
    assert.equal(written, '');
    assert.match(stdout, /not production visible/i);
  }

  const qualified = exportQualification('true');
  assert.equal(qualified.written, 'PROCYON_KNOWLEDGE_SEARCH_RELEASE_QUALIFIED=true\n');

  // An ambiguous value is a configuration error, never a silent go.
  for (const value of ['TRUE', '1', 'yes', ' true']) {
    assert.throws(
      () => exportQualification(value),
      /Command failed/,
      `${value} must not qualify a release`,
    );
  }
});

test('knowledge release preconditions run the supported-platform full-text lifecycle tests', () => {
  const planned = JSON.parse(
    execFileSync('node', ['scripts/check-knowledge-search-preconditions.mjs', '--print-plan'], {
      cwd: repoRoot,
      encoding: 'utf8',
    }),
  );
  const storage = read('crates', 'fm-semantic-worker', 'src', 'zvec_storage.rs');

  assert.equal(planned.command, 'cargo');
  assert.deepEqual(planned.reportValidator.arguments.slice(0, 7), [
    'run',
    '--quiet',
    '--locked',
    '-p',
    'fm-application',
    '--example',
    'validate_knowledge_release_report',
  ]);
  assert.ok(planned.commonArguments.includes('--features'));
  assert.ok(planned.commonArguments.includes('zvec'));
  assert.deepEqual(planned.commonArguments.slice(0, 5), [
    'test',
    '--locked',
    '-p',
    'fm-semantic-worker',
    '--lib',
  ]);
  assert.equal(planned.invocations.length, planned.tests.length);
  assert.ok(
    planned.tests.some((name) => /migrat/.test(name)),
    'migration coverage is required',
  );
  assert.ok(
    planned.tests.some((name) => /full_text/.test(name)),
    'full-text lifecycle coverage is required',
  );
  for (const [index, name] of planned.tests.entries()) {
    const [, unqualified] = name.split('zvec_storage::tests::');
    assert.match(
      storage,
      new RegExp(`fn ${unqualified}\\(`),
      `${name} must exist in the Zvec storage tests`,
    );
    assert.deepEqual(planned.invocations[index].slice(-3), ['--', '--exact', name]);
  }
});

test('knowledge release preconditions require a repository-recorded measured go', () => {
  const current = spawnSync(
    'node',
    ['scripts/check-knowledge-search-preconditions.mjs', '--check-report'],
    { cwd: repoRoot, encoding: 'utf8' },
  );
  assert.notEqual(current.status, 0);
  assert.match(`${current.stdout}${current.stderr}`, /not a measured go/i);

  const outputRoot = scratchDirectory('knowledge-report-');
  const reports = [
    {
      name: 'unmeasured-go',
      value: { decision: 'go', productionMeasurement: false, blockingReasons: [] },
      accepted: false,
    },
    {
      name: 'blocked-go',
      value: {
        decision: 'go',
        productionMeasurement: true,
        blockingReasons: ['supported-platform evidence missing'],
      },
      accepted: false,
    },
  ];
  for (const candidate of reports) {
    const report = join(outputRoot, `${candidate.name}.json`);
    writeFileSync(report, JSON.stringify(candidate.value));
    const result = spawnSync(
      'node',
      ['scripts/check-knowledge-search-preconditions.mjs', '--check-report', '--report', report],
      { cwd: repoRoot, encoding: 'utf8' },
    );
    assert.equal(result.status === 0, candidate.accepted, candidate.name);
  }

  const forged = JSON.parse(read('docs', 'evaluations', 'knowledge-retrieval-v1.json'));
  forged.decision = 'go';
  forged.productionMeasurement = true;
  forged.blockingReasons = [];
  const report = join(outputRoot, 'forged-go.json');
  writeFileSync(report, JSON.stringify(forged));
  const forgedResult = spawnSync(
    'node',
    ['scripts/check-knowledge-search-preconditions.mjs', '--check-report', '--report', report],
    { cwd: repoRoot, encoding: 'utf8' },
  );
  assert.notEqual(forgedResult.status, 0);
  assert.match(`${forgedResult.stdout}${forgedResult.stderr}`, /evidence does not support a go/i);
});

test('developer semantic bundle exposes the native runtime under its loader filename', () => {
  const outputRoot = scratchDirectory('semantic-runtime-alias-');
  for (const [platform, library] of [
    ['darwin', 'libzvec_c_api.dylib'],
    ['linux', 'libzvec_c_api.so'],
    ['win32', 'zvec_c_api.dll'],
  ]) {
    const source = join(outputRoot, platform, 'build', `content-addressed-${library}`);
    const bundle = join(outputRoot, platform, 'bundle');
    mkdirSync(dirname(source), { recursive: true });
    mkdirSync(join(bundle, 'artifacts'), { recursive: true });
    writeFileSync(source, `native-runtime-${platform}`);

    const alias = installNativeRuntimeAlias(source, bundle, platform);

    assert.equal(alias, join(bundle, 'artifacts', library));
    assert.equal(readFileSync(alias, 'utf8'), `native-runtime-${platform}`);
  }
});

test('release verification key exporter writes only a validated public key as hex', () => {
  const outputRoot = mkdtempSync(join(tmpdir(), 'procyon-semantic-key-'));
  const environmentFile = join(outputRoot, 'github.env');
  const publicKeyFile = join(outputRoot, 'semantic-catalog.pub');
  const signingKeyFile = join(outputRoot, 'semantic-catalog.key');
  const key = Buffer.alloc(32, 0x2a);
  const signingKey = Buffer.alloc(32, 0x3b);

  execFileSync(
    'node',
    ['scripts/export-semantic-verifying-key.mjs', environmentFile, publicKeyFile, signingKeyFile],
    {
      cwd: repoRoot,
      env: {
        ...process.env,
        VERIFYING_KEY_BASE64: key.toString('base64'),
        SIGNING_KEY_BASE64: signingKey.toString('base64'),
      },
    },
  );

  assert.equal(
    readFileSync(environmentFile, 'utf8'),
    `PROCYON_SEMANTIC_CATALOG_VERIFYING_KEY_HEX=${key.toString('hex')}\n`,
  );
  assert.deepEqual(readFileSync(publicKeyFile), key);
  assert.deepEqual(readFileSync(signingKeyFile), signingKey);
  assert.throws(
    () =>
      execFileSync('node', ['scripts/export-semantic-verifying-key.mjs', environmentFile], {
        cwd: repoRoot,
        env: {
          ...process.env,
          VERIFYING_KEY_BASE64: Buffer.alloc(31).toString('base64'),
        },
        stdio: 'pipe',
      }),
    /Command failed/,
  );
});

test('package-manager generator creates a Homebrew cask and Chocolatey installer package', () => {
  const outputRoot = mkdtempSync(join(tmpdir(), 'procyon-packages-'));
  const checksum = 'a'.repeat(64);
  const commonArgs = ['--version', '1.2.3', '--sha256', checksum, '--repository', 'example/fm'];

  const caskPath = join(outputRoot, 'Casks', 'procyon.rb');
  execFileSync(
    'node',
    [
      'scripts/generate-package-manager-files.mjs',
      'homebrew',
      ...commonArgs,
      '--asset',
      'Procyon_1.2.3_universal.dmg',
      '--linux-sha256',
      'b'.repeat(64),
      '--linux-asset',
      'Procyon_1.2.3_amd64.AppImage',
      '--output',
      caskPath,
    ],
    { cwd: repoRoot },
  );
  const cask = readFileSync(caskPath, 'utf8');
  assert.match(cask, /cask "procyon" do/);
  assert.match(cask, /version "1\.2\.3"/);
  assert.match(cask, new RegExp(`sha256 "${checksum}"`));
  assert.match(
    cask,
    /os macos: "Procyon_1\.2\.3_universal\.dmg", linux: "Procyon_1\.2\.3_amd64\.AppImage"/,
  );
  assert.match(cask, /releases\/download\/v1\.2\.3\/#\{os\}/);
  assert.match(cask, /app "Procyon\.app"/);
  assert.match(cask, /binary "#{appdir}\/Procyon\.app\/Contents\/Resources\/procyon"/);
  assert.match(cask, /on_linux do/);
  assert.match(cask, new RegExp(`sha256 "${'b'.repeat(64)}"`));
  assert.match(cask, /app_image "Procyon_1\.2\.3_amd64\.AppImage", target: "Procyon\.AppImage"/);

  const tauriConfig = JSON.parse(read('apps', 'fm-desktop', 'src-tauri', 'tauri.conf.json'));
  assert.equal(tauriConfig.bundle.resources['resources/procyon'], 'procyon');
  assert.equal(tauriConfig.bundle.resources['resources/semantic'], 'semantic');
  const launcher = read('apps', 'fm-desktop', 'src-tauri', 'resources', 'procyon');
  assert.match(launcher, /exec \/usr\/bin\/open .* --args "\$@"/);
  assert.notEqual(
    statSync(join(repoRoot, 'apps/fm-desktop/src-tauri/resources/procyon')).mode & 0o111,
    0,
  );

  const chocolateyDir = join(outputRoot, 'chocolatey');
  execFileSync(
    'node',
    [
      'scripts/generate-package-manager-files.mjs',
      'chocolatey',
      ...commonArgs,
      '--asset',
      'Procyon_1.2.3_x64-setup.exe',
      '--output',
      chocolateyDir,
    ],
    { cwd: repoRoot },
  );
  const nuspec = readFileSync(join(chocolateyDir, 'procyon.nuspec'), 'utf8');
  const install = readFileSync(join(chocolateyDir, 'tools', 'chocolateyinstall.ps1'), 'utf8');
  assert.match(nuspec, /<id>procyon<\/id>/);
  assert.match(nuspec, /<version>1\.2\.3<\/version>/);
  assert.match(
    nuspec,
    /<licenseUrl>https:\/\/github\.com\/example\/fm\/blob\/main\/LICENSE<\/licenseUrl>/,
  );
  assert.doesNotMatch(nuspec, /<license(?:\s|>)/);
  assert.match(nuspec, /<iconUrl>.*icons\/icon\.png<\/iconUrl>/);
  assert.match(nuspec, /releases\/tag\/v1\.2\.3/);
  assert.match(install, /Install-ChocolateyPackage @packageArgs/);
  assert.match(install, /silentArgs\s*= '\/S'/);
  assert.match(install, new RegExp(`checksum64\\s+= '${checksum}'`));
});

test('Chocolatey nuspec uses dotted version identifiers, not our hyphenated pre-release suffix', () => {
  const outputRoot = mkdtempSync(join(tmpdir(), 'procyon-packages-'));
  const checksum = 'b'.repeat(64);
  const chocolateyDir = join(outputRoot, 'chocolatey');
  execFileSync(
    'node',
    [
      'scripts/generate-package-manager-files.mjs',
      'chocolatey',
      '--version',
      '0.1.0-6',
      '--sha256',
      checksum,
      '--repository',
      'example/fm',
      '--asset',
      'Procyon_0.1.0-6_x64-setup.exe',
      '--output',
      chocolateyDir,
    ],
    { cwd: repoRoot },
  );
  const nuspec = readFileSync(join(chocolateyDir, 'procyon.nuspec'), 'utf8');
  assert.match(nuspec, /<version>0\.1\.0\.6<\/version>/);
  assert.doesNotMatch(nuspec, /<version>0\.1\.0-6<\/version>/);
  // The GitHub release/tag/asset names keep the hyphenated version - only the nuspec's own
  // <version> (and therefore the resulting .nupkg filename) needs the dotted form.
  assert.match(nuspec, /releases\/tag\/v0\.1\.0-6/);
});

test('desktop CI runs platform packaging smoke tests after building', () => {
  const desktop = workflow('ci.yml').jobs.desktop;
  const commands = (desktop.steps ?? [])
    .map((step) => step.run)
    .filter((command) => typeof command === 'string');
  assert.ok(commands.some((command) => /smoke-desktop-package\.mjs/.test(command)));
});

test('desktop package smoke crosses native installer boundaries and retains isolated logs', () => {
  const smoke = read('scripts', 'smoke-desktop-package.mjs');
  assert.match(smoke, /hdiutil.*attach/s);
  assert.match(smoke, /msiexec\.exe/);
  assert.match(smoke, /dpkg-deb/);
  assert.match(smoke, /--appimage-extract/);
  assert.match(smoke, /xvfb-run/);
  assert.match(smoke, /FM_LOG_FILE/);
  assert.match(smoke, /PROCYON_QUALIFICATION_EVIDENCE_ROOT/);
  assert.match(smoke, /PROCYON_QUALIFICATION_CATALOG_DIRECTORY/);
  assert.match(smoke, /installed semantic\/\$\{name\} differs/);
});

test('installed semantic qualification has portable cleanup and AppImage build prerequisites', () => {
  const qualification = read('scripts', 'qualify-semantic-installed.mjs');
  const installed = workflow('release-desktop.yml').jobs['semantic-installed-qualification'];
  const linuxDependencies = installed.steps.find(
    (step) => step.name === 'Install Linux package and launch dependencies',
  );

  assert.match(qualification, /\brmSync\(filenameCanaryFile, \{ force: true \}\)/);
  assert.doesNotMatch(qualification, /\bfs\./);
  assert.match(qualification, /\bcpSync\(collected, path\.join\(evidence, 'safe-evidence'\)/);
  assert.match(linuxDependencies.run, /\bxdg-utils\b/);
});

test('README documents release versioning, package managers, smoke checks, and no auto-update', () => {
  const readme = read('README.md');
  assert.match(readme, /## Desktop releases/);
  assert.match(readme, /Cargo\.toml/);
  assert.match(readme, /v<version>/);
  assert.match(readme, /release notes/i);
  assert.match(readme, /Developer ID Application/i);
  assert.match(readme, /notariz/i);
  assert.match(readme, /SmartScreen/i);
  assert.match(readme, /APPLE_CERTIFICATE/);
  assert.match(readme, /APPLE_API_KEY_P8/);
  assert.doesNotMatch(readme, /APPLE_ID|APPLE_PASSWORD|APPLE_TEAM_ID|WINDOWS_CERTIFICATE/);
  assert.match(readme, /manual smoke/i);
  assert.match(readme, /brew install --cask/);
  assert.match(readme, /choco install procyon/);
  assert.match(readme, /HOMEBREW_TAP_TOKEN/);
  assert.match(readme, /CHOCOLATEY_API_KEY/);
  assert.match(readme, /auto-update is not included/i);
  assert.match(readme, /\.deb/);
  assert.match(readme, /AppImage/);
});
