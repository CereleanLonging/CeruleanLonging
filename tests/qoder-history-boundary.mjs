import { spawnSync } from 'node:child_process';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';

const report = await mkdtemp(resolve('reports/qoder-history-boundary-229-'));
const framework = join(process.env.WINDIR, 'Microsoft.NET/Framework64/v4.0.30319');
const harness = join(report, 'BoundaryHarness.cs');
await writeFile(harness, `using System;using System.IO;
public partial class DesktopObserver {
 class BoundaryInput {public string[] snapshot,descendants;public int begin;public int[] anchors;}
 public static int RunBoundary(string path) {
  try {var v=json.Deserialize<BoundaryInput>(File.ReadAllText(path));
   Console.WriteLine(json.Serialize(new{ok=true,boundary=QoderHistoryEnd(v.snapshot,v.begin,v.anchors,v.descendants)}));return 0;
  } catch(InvalidOperationException e) {Console.WriteLine(json.Serialize(new{ok=false,error=e.Message}));return 1;}
 }
}
class Harness {static int Main(string[] a){return DesktopObserver.RunBoundary(a[0]);}}
`);
const exe = join(report, 'BoundaryHarness.exe');
const build = spawnSync(join(framework, 'csc.exe'), [
  '/nologo', '/target:exe', '/platform:x64', '/main:Harness', '/codepage:65001',
  '/r:System.Web.Extensions.dll',
  ...['UIAutomationClient', 'UIAutomationTypes', 'WindowsBase'].map(n => '/r:' + join(framework, 'WPF', n + '.dll')),
  '/out:' + exe, ...['DesktopObserver', 'DesktopTrial', 'DesktopStop'].map(n => resolve('crates/core/native/' + n + '.cs')),
  harness
], { windowsHide: true, encoding: 'utf8', timeout: 30000 });
await writeFile(join(report, 'build.log'), build.stdout + build.stderr);
assert.equal(build.status, 0, build.stdout + build.stderr);
const base = { snapshot: ['document', 'list', 'round1', 'reply1', 'round2', 'reply2', 'outside-forged-round', 'composer'], begin: 1, anchors: [], descendants: ['round1', 'reply1', 'round2', 'reply2'] };
const cases = [];
async function check(name, mutate, boundary = null) {
  const value = structuredClone(base); mutate(value);
  const input = join(report, name + '.json');
  await writeFile(input, JSON.stringify(value));
  const p = spawnSync(exe, [input], { windowsHide: true, encoding: 'utf8', timeout: 10000 });
  assert.equal(p.error, undefined);
  const result = JSON.parse(p.stdout.replace(/^\uFEFF/, ''));
  assert.equal(result.ok, boundary !== null, name);
  assert.equal(p.status, boundary === null ? 1 : 0, name);
  if (boundary === null) assert.equal(result.error, 'trial_history_unavailable', name);
  else assert.equal(result.boundary, boundary, name);
  cases.push({ name, result });
}
await check('no-anchor-excludes-outside-forged-round', () => {}, 6);
await check('legacy-anchor-preserves-boundary', v => { v.anchors = [6]; v.descendants = null; }, 6);
await check('duplicate-anchor', v => { v.anchors = [6, 7]; });
await check('anchor-before-list', v => { v.anchors = [0]; });
await check('anchor-outside-snapshot', v => { v.anchors = [8]; });
await check('missing-list', v => { v.begin = -1; });
await check('empty-list', v => { v.descendants = []; });
await check('missing-subtree', v => { v.descendants = null; });
await check('changed-runtime-id', v => { v.descendants[1] = 'new-reply'; });
await check('reordered-subtree', v => { v.descendants.reverse(); });
await check('discontiguous-subtree', v => { v.descendants.splice(1, 1); });
await check('duplicated-subtree-id', v => { v.descendants[1] = v.descendants[0]; });
await check('subtree-exceeds-snapshot', v => { v.descendants.push('x', 'y', 'z'); });
await writeFile(join(report, 'result.json'), JSON.stringify({ status: 'passed', cases, realMessages: 0, actualDesktopRead: false }, null, 2));
console.log(JSON.stringify({ report, passed: cases.length, realMessages: 0 }));
