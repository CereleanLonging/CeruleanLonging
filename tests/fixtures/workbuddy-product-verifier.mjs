// Trusted, pinned launcher. Generated code runs only in the restricted child VM.
import { realpathSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';

const ordinary = value => value.startsWith('\\\\?\\UNC\\')
  ? '\\\\' + value.slice(8)
  : value.startsWith('\\\\?\\') ? value.slice(4) : value;
const original = process.cwd();
const cwd = ordinary(original);
assert.equal(realpathSync.native(original), realpathSync.native(cwd));
const verifier = fileURLToPath(new URL('./workbuddy-coding-verifier.mjs', import.meta.url));
const artifact = join(cwd, 'sum.mjs');
const env = Object.fromEntries(Object.entries(process.env).filter(([name]) =>
  ['SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP'].includes(name.toUpperCase())));
const result = spawnSync(process.execPath, [
  '--permission', '--allow-fs-read=' + artifact, '--allow-fs-read=' + verifier,
  '--experimental-vm-modules', verifier, artifact,
], { cwd, env, encoding: 'utf8', windowsHide: true, timeout: 10000, maxBuffer: 65536 });
if (result.stdout) process.stdout.write(result.stdout);
if (result.stderr) process.stderr.write(result.stderr);
if (result.error) process.stderr.write('Restricted verifier failed: ' + result.error.code + '\n');
process.exitCode = result.error ? 1 : result.status ?? 1;
