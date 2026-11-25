import test from 'node:test';
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp, mkdir, writeFile, readFile, access} from 'node:fs/promises';
import {createServer} from 'node:net';
import {tmpdir} from 'node:os';
import {join, resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {nativeServerArguments} from './native-test.mjs';

const exec = promisify(execFile), root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
async function fixture() {
  const dir = await mkdtemp(join(tmpdir(), 'native-server-inputs-')), binary = join(dir, 'openrotmg'), data = join(dir, 'data');
  // The fake server records its exact argv so the launcher contract is observed, not inferred.
  await writeFile(binary, `#!/bin/sh\nprintf '%s\\n' "$@" > ${JSON.stringify(join(dir, 'argv'))}\n`, {mode: 0o700});
  await writeFile(join(dir, 'plain'), '', {mode: 0o600});
  await mkdir(data);
  await mkdir(join(dir, 'client/scripts'), {recursive: true});
  await writeFile(join(dir, 'client/scripts/run.py'), 'import sys\nassert sys.argv[1:] == ["--prepare-only"]\n');
  return {dir, binary, data};
}
const port = () => new Promise((ok, bad) => {const s = createServer().listen(0, '127.0.0.1', () => {const {port} = s.address(); s.close(e => e ? bad(e) : ok(port));}).once('error', bad);});
const launcher = async (dir, args) => exec('python3', [join(root, 'scripts/run.py'), String(await port()), '--test-state', join(dir, 'state'), '--no-shore-bots', '--client', join(dir, 'client'), ...args], {cwd: root});

test('source builds stay the default and an explicit retained binary/data pair is forwarded unchanged', async () => {
  const {binary, data} = await fixture();
  assert.deepEqual(await nativeServerArguments({}), ['--release']);
  assert.deepEqual(await nativeServerArguments({serverBinary: binary, data}), ['--server-binary', binary, '--data', data]);
});

test('invalid native server selections are refused before launch', async () => {
  const {dir, binary, data} = await fixture();
  for (const [selection, message] of [
    [{serverBinary: binary}, /given together/],
    [{data}, /given together/],
    [{serverBinary: 'openrotmg', data}, /absolute/],
    [{serverBinary: binary, data: 'data'}, /absolute/],
    [{serverBinary: join(dir, 'missing'), data}, /ENOENT/],
    [{serverBinary: data, data}, /must be a file/],
    [{serverBinary: join(dir, 'plain'), data}, /EACCES/],
    [{serverBinary: binary, data: binary}, /must be a directory/],
  ]) await assert.rejects(nativeServerArguments(selection), message, JSON.stringify(selection));
});

test('run.py starts the selected executable with its data and never builds', async () => {
  const {dir, binary, data} = await fixture();
  const {stdout} = await launcher(dir, ['--server-binary', binary, '--data', data]);
  assert.match(stdout, /Starting selected server binary/);
  assert.doesNotMatch(stdout, /Building/);
  const argv = (await readFile(join(dir, 'argv'), 'utf8')).trim().split('\n');
  assert.equal(argv[argv.indexOf('--data') + 1], data);
  assert.equal(argv[argv.indexOf('--test-state') + 1], join(dir, 'state'));
  await assert.rejects(access(join(dir, 'state/server-build')), {code: 'ENOENT'});
});

test('run.py refuses invalid selections before state, client preparation or build', async () => {
  const {dir, binary, data} = await fixture();
  for (const args of [
    ['--server-binary', binary],
    ['--data', data],
    ['--server-binary', binary, '--data', data, '--release'],
    ['--server-binary', join(dir, 'plain'), '--data', data],
    ['--server-binary', binary, '--data', join(dir, 'missing')],
  ]) {
    const error = await launcher(dir, args).then(() => null, e => e);
    assert.equal(error?.code, 2, JSON.stringify(args));
    assert.match(error.stderr, /--server-binary|--data/);
    await assert.rejects(access(join(dir, 'state')), {code: 'ENOENT'});
  }
});

test('runner exits after a case leaks a timer', async () => {
  const leak = `import {exitDespiteLingering} from ${JSON.stringify(join(root, 'scripts/native-test.mjs'))};setInterval(() => {}, 1000);process.exitCode = 1;exitDespiteLingering(200);`;
  const started = Date.now(), failure = await exec(process.execPath, ['--input-type=module', '-e', leak], {timeout: 10000}).catch(error => error);
  assert.equal(failure.code, 1);
  assert.match(failure.stderr, /lingering case handles: .*Timeout/);
  assert(Date.now() - started < 5000);
});
