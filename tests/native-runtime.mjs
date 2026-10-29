import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
export async function assertNativeRuntime(){
  const hash=async file=>createHash('sha256').update(await readFile(file)).digest('hex');
  assert.equal(await hash('target/debug/cerulean-runtime.exe'),await hash('target/debug/cerulean-core.exe'),
    'Native runtime is stale. Run scripts/check.ps1 or the desktop build workflow before native tests.');
}
