'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '..');
const source = path.join(root, 'build/npm');
const pkg = require(path.join(source, 'package.json'));
const installer = require(path.join(source, 'install.js'));

function run(program, args, options = {}) {
  const result = spawnSync(program, args, {encoding: 'utf8', ...options});
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${program}: ${result.stderr}`);
  return result.stdout;
}

async function main() {
  assert.ok(process.env.npm_execpath, 'Run this with npm --prefix build/npm run test:package');
  const asset = installer.assetName();
  const releaseAsset = path.join(root, 'dist/artifacts', asset);
  const binary = process.env.SIGNOZ_TEST_BINARY || (fs.existsSync(releaseAsset) ? releaseAsset : path.join(root, 'target/release', process.platform === 'win32' ? 'signoz-cli.exe' : 'signoz-cli'));
  assert.equal(run(binary, ['--version']).trim(), `signoz-cli ${pkg.version}`);
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'signoz-npm-pack-'));
  try {
    const env = {...process.env, npm_config_cache: path.join(temporary, 'cache')};
    const cliEnv = {...process.env, SIGNOZ_CONFIG: path.join(temporary, 'empty.toml')};
    delete cliEnv.SIGNOZ_URL;
    delete cliEnv.SIGNOZ_API_KEY;
    const npm = args => run(process.execPath, [process.env.npm_execpath, ...args], {cwd: source, env});
    const packed = JSON.parse(npm(['pack', '--ignore-scripts', '--json', '--pack-destination', temporary]))[0];
    assert.equal(packed.name, '@cognelis/signoz-cli');
    assert.equal(packed.version, pkg.version);
    const allow = new Set(['package.json', 'README.md', 'LICENSE', 'install.js', 'check-release.js', 'bin/signoz-cli.js']);
    assert.deepEqual(new Set(packed.files.map(file => file.path)), allow);
    const project = path.join(temporary, 'project');
    npm(['install', '--prefix', project, '--ignore-scripts', '--no-audit', '--no-fund', '--package-lock=false', path.join(temporary, packed.filename)]);
    const installed = path.join(project, 'node_modules/@cognelis/signoz-cli');
    const launcher = path.join(installed, 'bin/signoz-cli.js');
    assert.ok(fs.existsSync(path.join(project, 'node_modules/.bin', process.platform === 'win32' ? 'signoz-cli.cmd' : 'signoz-cli')));
    const data = fs.readFileSync(binary);
    // The pre-release network fixture supplies the freshly built native binary.
    // Post-release verification repeats installation against the public URLs.
    await require(path.join(installed, 'install.js')).install({
      repository: 'https://github.com/example/signoz-cli',
      fetcher: async url => url.endsWith('/checksums.txt') ? Buffer.from(`${installer.sha256(data)} ${asset}\n`) : data,
    });
    const value = JSON.parse(run(process.execPath, [launcher, 'version', '--output', 'json'], {env: cliEnv}));
    assert.equal(value.data.version, pkg.version);
    assert.equal(value.schemaVersion, '1');
    const invalid = spawnSync(process.execPath, [launcher, '--output', 'json', 'logs', 'search', '--since', '0h'], {encoding: 'utf8', env: cliEnv});
    assert.equal(invalid.status, 2);
    assert.equal(invalid.stdout, '');
    assert.equal(JSON.parse(invalid.stderr).error.code, 'validation');
    assert.ok(run(process.execPath, [launcher, 'completion', 'zsh'], {env: cliEnv}).startsWith('#compdef signoz-cli\n'));
    console.log(`Packed ${packed.filename}; verified allowed files, native launch, JSON, completion, and exit-code forwarding.`);
  } finally { fs.rmSync(temporary, {recursive: true, force: true}); }
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
