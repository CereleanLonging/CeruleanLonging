import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { qoderCandidate, qoderUpgradeBlock } from '../apps/desktop/src/qoderUpgrade.ts';

assert(process.env.CERULEAN_CACHE_SESSION);
assert.equal(process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE, '1');
const scratch = mkdtempSync(join(process.env.CERULEAN_CACHE_SESSION, 'qoder-upgrade-policy-'));
const source = readFileSync('crates/core/native/DesktopTrial.cs', 'utf8');
const method = source.slice(source.indexOf('    static bool QoderCandidate('), source.indexOf('    static void Enabled('));
assert(method.includes('UInt16.TryParse'));
writeFileSync(join(scratch, 'policy.cs'), `using System;using System.Linq;
class Policy {
${method}
static void Main(string[] values) { foreach(var value in values) Console.WriteLine(QoderCandidate(value)?"true":"false"); }
}`);
function run(program, args) {
  const result = spawnSync(program, args, { windowsHide: true, encoding: 'utf8', timeout: 60000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  return result.stdout.trim();
}
const executable = join(scratch, 'policy.exe');
run(join(process.env.SystemRoot, 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'),
  ['/nologo', `/out:${executable}`, join(scratch, 'policy.cs')]);
const versions = ['0.4.3.0', '0.4.8.0', '0.4.8.1', '0.4.9.0', '0.4.65535.65535',
  'unknown', '0.4.2.0', '0.5.0.0', '1.4.9.0', '0.4.9', '0.4.9.0-beta', '0.4.+9.0', '0.4.9.65536'];
const expected = versions.map((_, index) => index < 5);
assert.deepEqual(versions.map(qoderCandidate), expected);
assert.deepEqual(run(executable, versions).split(/\r?\n/).map(value => value === 'true'), expected);

const bridge = 'f'.repeat(64);
const observation = { window: { app: 'qoder', hwnd: '100', pid: 10, startTicks: '123',
  exe: 'D:/Qoder/Qoder.exe', title: 'project', version: '0.4.9.0' },
  capturedAt: '2026-11-07T00:00:03Z', sessionId: 'session', modelLabel: 'model' };
const binding = { observation, bridgeSha256: bridge };
const trial = { review: { observation, bridgeSha256: bridge, marker: 'marker', baseline: ['1', '2'] },
  proof: { readerSha256: bridge, marker: 'marker', sessionId: 'session', baseline: ['1', '2'],
    promptSeen: true, completed: true, replyMatches: true, observedAt: '2026-11-07T00:00:02Z' } };
assert(qoderUpgradeBlock(binding, [], bridge));
assert.equal(qoderUpgradeBlock(binding, [trial], bridge), null);
const changes = [
  value => { value.observation.window.version = '0.4.10.0'; },
  value => { value.observation.window.pid++; },
  value => { value.observation.window.startTicks = '456'; },
  value => { value.observation.window.exe = 'D:/Other/Qoder.exe'; },
  value => { value.observation.sessionId = 'foreign'; },
  value => { value.observation.modelLabel = 'foreign'; },
  value => { value.bridgeSha256 = '0'.repeat(64); },
  value => { value.observation.capturedAt = '2026-11-07T00:00:01Z'; },
];
for (const change of changes) {
  const changed = structuredClone(binding);
  change(changed);
  assert(qoderUpgradeBlock(changed, [trial], bridge));
}
const lost = structuredClone(trial);
lost.proof = null;
assert(qoderUpgradeBlock(binding, [lost], bridge));
const older = structuredClone(binding);
older.observation.window.version = '0.4.8.0';
assert.equal(qoderUpgradeBlock(older, [], bridge), null);
console.log(JSON.stringify({ passed: true, nativeAndFrontendVersions: versions.length,
  changedIdentityRejections: changes.length, unfinishedProbeRejected: true, legacyBindingPreserved: true,
  modelRequests: 0 }));
