import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

assert.equal(process.platform, 'win32');
assert(process.env.CERULEAN_CACHE_SESSION);
assert.equal(process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE, '1');
const root = resolve('.');
const temporary = mkdtempSync(join(process.env.CERULEAN_CACHE_SESSION, 'bridge-determinism-'));
const builder = join(temporary, 'build-bridge.exe');
function run(program, args, env = process.env) {
  const result = spawnSync(program, args, { env, encoding: 'utf8', windowsHide: true, timeout: 120000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  return result;
}
run('rustc', ['--edition=2021', join(root, 'crates/core/build.rs'), '-o', builder]);
function build(name) {
  const source = join(temporary, name, 'source');
  const output = join(temporary, name, 'output');
  mkdirSync(join(source, 'native'), { recursive: true });
  mkdirSync(output, { recursive: true });
  for (const file of ['DesktopObserver.cs', 'DesktopTrial.cs', 'DesktopStop.cs']) {
    copyFileSync(join(root, 'crates/core/native', file), join(source, 'native', file));
  }
  run(builder, [], { ...process.env, CARGO_CFG_TARGET_OS: 'windows', CARGO_MANIFEST_DIR: source, OUT_DIR: output });
  return { source, output, executable: join(output, 'CeruleanDesktopObserver.exe') };
}
const first = build('first');
await new Promise(resolve => setTimeout(resolve, 1200));
const second = build('second');
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
assert.equal(hash(first.executable), hash(second.executable), 'Identical source must retain the desktop binding across time and source/output paths');
const probe = spawnSync(first.executable, [JSON.stringify({ version: 0, op: 'discover' })], { encoding: 'utf8', windowsHide: true, timeout: 15000 });
assert.ifError(probe.error);
assert.equal(probe.status, 1);
assert.deepEqual(JSON.parse(probe.stdout), { version: 1, ok: false, error: 'protocol_mismatch' });
const changed = join(second.source, 'native/DesktopObserver.cs');
const original = readFileSync(changed, 'utf8');
assert(original.includes('request_limit'));
writeFileSync(changed, original.replaceAll('request_limit', 'request_limit_probe'));
run(builder, [], { ...process.env, CARGO_CFG_TARGET_OS: 'windows', CARGO_MANIFEST_DIR: second.source, OUT_DIR: second.output });
assert.notEqual(hash(first.executable), hash(second.executable), 'Changed bridge behavior must invalidate the old binding');
console.log(JSON.stringify({ passed: true, cases: 3, deterministicSha256: hash(first.executable), changedSha256: hash(second.executable), nativeUiActions: 0 }));
