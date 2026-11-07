// Real packaged cold-backup operations with disposable data; no resident core,
// credentials, network calls or user's project directories.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, readdirSync, renameSync, cpSync, lstatSync } from 'node:fs';
import { resolve, join, dirname } from 'node:path';

const [packagePath, reportPath] = process.argv.slice(2);
assert(packagePath && reportPath, 'Expected package directory and fresh report path');
assert(process.env.CERULEAN_CACHE_SESSION, 'Disposable managed test session required');
const session = resolve(process.env.CERULEAN_CACHE_SESSION.replace(/^\\\\\?\\/, ''));
assert(session.startsWith(resolve('.temp/build-cache/jobs') + '\\'));
const root = join(session, 'scratch', 'current-backup');
mkdirSync(root);
const data = join(root, 'data'), archive = join(root, 'backup');
const exe = join(resolve(packagePath), 'cerulean-runtime.exe');
const version = JSON.parse(readFileSync('package.json', 'utf8')).version;
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const cases = [], calls = [];
function invoke(args, expected = 0) {
  const result = spawnSync(exe, args, { cwd: root, input: '', encoding: 'utf8', windowsHide: true, timeout: 30000, maxBuffer: 2 * 1024 * 1024 });
  assert(!result.error, result.error?.message);
  assert.equal(result.status, expected, result.stderr);
  return result;
}
function cli(args, expected = 0) {
  const result = invoke(['--workbench', '--data-dir', data, ...args], expected);
  const response = JSON.parse(expected ? result.stderr : result.stdout);
  assert.equal(response.ok, expected === 0);
  calls.push({ command: args.slice(0, 2).join(' '), exit: result.status });
  return response;
}
function inventory(directory, prefix = '') {
  const result = {};
  for (const entry of readdirSync(join(directory, prefix), { withFileTypes: true })) {
    assert(!entry.isSymbolicLink());
    const path = join(prefix, entry.name);
    if (entry.isDirectory()) {
      result[path] = 'directory';
      Object.assign(result, inventory(directory, path));
    } else result[path] = sha(readFileSync(join(directory, path)));
  }
  return result;
}

try {
  const info = JSON.parse(invoke(['--build-info']).stdout);
  assert.equal(info.version, version);
  assert(info.backupWriterVersions.includes(version));
  assert(info.backupWriterVersions.includes('0.1.47'));
  assert(!info.backupWriterVersions.includes('999.0.0'));
  assert.deepEqual(readdirSync(root), []);
  cases.push('Build info accepts its current writer, retains the previous writer and has no filesystem side effects');

  invoke(['--stdio', '--data-dir', data]); // Open and close a real empty Store.
  mkdirSync(join(data, 'empty'));
  writeFileSync(join(data, 'synthetic-journal.json'), '{"attempt":"original","state":"unknown"}\n');
  const original = inventory(data);
  const created = cli(['backup', 'create', '--to', archive]).result;
  const manifestPath = join(archive, 'manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  assert.equal(manifest.version, version);
  assert.equal(cli(['backup', 'verify', '--from', archive]).result.manifestDigest, created.manifestDigest);
  assert.deepEqual(inventory(data), original);
  cases.push('Current packaged runtime creates and verifies its own cold backup without changing source data');

  cli(['backup', 'restore', '--from', archive, '--manifest-digest', created.manifestDigest], 3);
  assert.deepEqual(inventory(data), original);
  cases.push('Restore refuses an existing destination and leaves its bytes unchanged');

  const preserved = join(root, 'preserved-original');
  // Both resolved directory names are fixed siblings in this newly allocated job.
  assert.equal(dirname(resolve(data)), root);
  assert.equal(dirname(resolve(preserved)), root);
  assert(!lstatSync(data).isSymbolicLink());
  renameSync(data, preserved);
  cli(['backup', 'restore', '--from', archive, '--manifest-digest', '0'.repeat(64)], 3);
  cli(['backup', 'restore', '--from', archive, '--manifest-digest', created.manifestDigest]);
  assert.deepEqual(inventory(data), original);
  assert.deepEqual(inventory(preserved), original);
  cli(['status'], 4);
  cases.push('Approved original-location restore is byte-exact, rejects a stale digest and never starts a service');

  const invalid = join(root, 'invalid-copy');
  cpSync(archive, invalid, { recursive: true, errorOnExist: true, force: false });
  const invalidManifest = join(invalid, 'manifest.json');
  writeFileSync(invalidManifest, JSON.stringify({ ...manifest, version: '999.0.0' }));
  assert.match(JSON.stringify(cli(['backup', 'verify', '--from', invalid], 3)), /unsupported backup format\/version/);
  assert.match(JSON.stringify(cli(['backup', 'restore', '--from', invalid, '--manifest-digest', sha(readFileSync(invalidManifest))], 3)), /unsupported backup format\/version/);
  cases.push('An unknown future writer is rejected for verification and restore');
  writeFileSync(invalidManifest, JSON.stringify({ ...manifest, schema: 2 }));
  assert.match(JSON.stringify(cli(['backup', 'verify', '--from', invalid], 3)), /unsupported backup format\/version/);
  cases.push('Unknown backup schema remains rejected');
  writeFileSync(invalidManifest, JSON.stringify(manifest));
  writeFileSync(join(invalid, 'payload', 'synthetic-journal.json'), 'tampered');
  assert.match(JSON.stringify(cli(['backup', 'verify', '--from', invalid], 3)), /inventory\/hash mismatch/);
  assert.deepEqual(inventory(data), original);
  assert.equal(cli(['backup', 'verify', '--from', archive]).result.manifestDigest, created.manifestDigest);
  cases.push('Tampered payload is rejected while the original backup and restored bytes remain intact');

  writeFileSync(reportPath, JSON.stringify({ passed: true, version, cases, calls, runtimeSha256: sha(readFileSync(exe)), byteExactRestore: true, realProviderCalls: 0, residentCoreStarted: false, scope: 'Real packaged cold-backup and restore; synthetic empty Store and journal only' }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ passed: true, cases: cases.length, report: reportPath }));
} catch (error) {
  writeFileSync(reportPath, JSON.stringify({ passed: false, cases, calls, error: String(error.stack) }, null, 2), { flag: 'wx' });
  throw error;
}
