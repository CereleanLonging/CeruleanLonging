import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';
if (process.env.CERULEAN_NATIVE_OWNED_JOB !== '1') throw Error('Use scripts/run-workspace-verification-ui-158.ps1 owned runner');
const dir = await mkdtemp('reports/workspace-verification-ui-158-'), cases = [], errors = [];
let browser, page, networkRequests = 0;
const files = ['WorkspaceVerificationResult.tsx', 'WorkspaceJobs.tsx', 'WorkspaceBrowser.tsx', 'contracts.ts'].map(n => 'apps/desktop/src/' + n).concat('tests/workspace-verification-ui.mjs');
const hashes = async () => Object.fromEntries(await Promise.all(files.map(async n => [n, createHash('sha256').update(await readFile(n)).digest('hex')])));
const before = await hashes(), entry = join(dir, 'entry.tsx');
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {WorkspaceRecordsHarness as WorkspaceBrowser} from '../../tests/fixtures/WorkspaceRecordsHarness';
const root=createRoot(document.getElementById('root')!);window.epoch=0;window.tick=0;
window.render=()=>{window.tick++;root.render(<React.StrictMode><div data-tick={window.tick}><WorkspaceBrowser key={window.epoch} enabled={true} busy={false} showPlans={false} send={async()=>{window.sends++;throw Error('unexpected mutation')}} onOverview={()=>{}} read={async q=>{
window.reads.push(q);
if(q.type==='overview')return {type:q.type,value:{product:'local',protocolVersion:1,dataDirectory:'original',mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:2,message:null}};
if(q.type==='plans')return {type:q.type,value:{revision:'base',offset:0,total:0,nextOffset:null,entries:[]}};
if(q.type==='projects')return {type:q.type,value:{revision:'projects',offset:0,total:2,nextOffset:null,entries:['p','other'].map(id=>({project:{id,root:id,timezone:'UTC'},revisionDigest:'plan',sequence:1,approved:false,taskCount:1,resourceHoldingTasks:1,unknownTasks:1}))}};
if(q.type==='jobs')return {type:q.type,value:{projectId:q.projectId,revision:'jobs',offset:0,total:1,nextOffset:null,entries:[{id:'operation',projectId:q.projectId,taskId:'task',attemptId:'attempt',status:{type:'verification',value:window.meta.state},startedAt:'start',updatedAt:'update',hasMessage:true}]}};
if(!['verificationResult','verificationResultArtifacts','verificationResultStages'].includes(q.type))throw Error('unexpected read '+q.type);
if(window.fail)throw Error('original verification unavailable');
let value=q.type==='verificationResult'?{...window.meta,projectId:q.projectId,operationId:q.operationId}:{projectId:q.projectId,operationId:q.operationId,revision:window.meta.revision,offset:q.offset,total:3,nextOffset:q.offset===0?2:null,entries:(q.type==='verificationResultArtifacts'?window.artifacts:window.stages).slice(q.offset,q.offset===0?2:3)};
if(window.patch)value={...value,...window.patch};
const result={type:q.type,value};if(window.hold){window.hold=false;return await new Promise(resolve=>{window.finish=()=>resolve(result)})}return result;
}}/></div></React.StrictMode>)};
window.set=s=>{window.epoch++;Object.assign(window,{reads:[],sends:0,fail:false,patch:null,hold:false,finish:null},s);window.render()};`);
await build({ configFile: false, logLevel: 'error', define: { 'process.env.NODE_ENV': JSON.stringify('production') }, oxc: { jsx: { runtime: 'automatic' } }, build: { outDir: join(dir, 'bundle'), emptyOutDir: false, lib: { entry, name: 'VerificationUI', formats: ['iife'], fileName: () => 'ui.js' } } });
const meta = { projectId: 'p', operationId: 'operation', taskId: 'task', attemptId: 'attempt', state: 'unknown', revision: 'result-v1', revisionDigest: 'original-plan', manifestDigest: 'original-manifest', snapshotDigest: 'original-snapshot', ledgerRecorded: false, evidenceDigest: 'original-evidence', observedAt: 'original-time', message: 'ORIGINAL RESULT', recovery: { evidenceDigest: 'recovery-evidence', passed: false }, recoveryError: 'ORIGINAL DIAGNOSTIC', artifactCount: 3, stageCount: 3 };
const artifacts = Array.from({ length: 3 }, (_, i) => ({ path: 'artifact-' + i, bytes: i + 1, sha256: 'artifact-digest-' + i }));
const stages = Array.from({ length: 3 }, (_, i) => ({ id: 'stage-' + i, name: 'Stage ' + i, commandDigest: 'command-' + i, timeoutSeconds: 30, state: i === 0 ? 'unknown' : 'passed', exitCode: i === 0 ? null : 0, timedOut: false, stdout: { path: 'stdout-' + i, bytes: 12, sha256: 'stdout-sha-' + i }, stderr: null }));
try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  page = await browser.newPage(); page.setDefaultTimeout(10000); await page.clock.install();
  page.on('pageerror', e => errors.push(e.message)); await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent('<html lang="zh-CN"><div id="root"></div></html>'); await page.addScriptTag({ content: await readFile(join(dir, 'bundle/ui.js'), 'utf8') });
  const result = () => page.getByRole('region', { name: '独立验收结果', exact: true });
  const nav = name => result().getByRole('navigation', { name, exact: true });
  const ready = async () => { await result().getByText('ORIGINAL RESULT', { exact: true }).waitFor(); await page.waitForFunction(() => !Array.from(document.querySelectorAll('button')).find(b => b.textContent === '刷新验收结果')?.disabled); };
  const reset = async extra => {
    await page.evaluate(s => window.set(s), { meta, artifacts, stages, ...extra });
    await page.waitForFunction(() => document.querySelector('[data-tick]')?.getAttribute('data-tick') === String(window.tick));
    await page.getByRole('button', { name: '查看项目 p 的作业', exact: true }).click();
    await page.getByRole('button', { name: '查看验收 operation 的结果', exact: true }).click(); await ready();
  };
  await reset();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'verificationResult', projectId: 'p', operationId: 'operation', revision: null });
  await result().getByText('原验收元数据与证据字段', { exact: true }).click();
  assert.deepEqual(JSON.parse(await result().locator('pre').innerText()), meta);
  assert.match(await result().innerText(), /未知结果仍锁定原资源/); assert.match(await result().innerText(), /验收待对账/);
  cases.push('production overview → jobs → exact verification operation; full original metadata and recovery fields; unknown remains locked');
  for (const [button, name, type, text] of [['读取产物首页', '验收产物分页', 'verificationResultArtifacts', 'artifact-2'], ['读取阶段首页', '验收阶段分页', 'verificationResultStages', 'Stage 2']]) {
    await result().getByRole('button', { name: button, exact: true }).click();
    await nav(name).getByRole('button', { name: '下一页' }).click();
    assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type, projectId: 'p', operationId: 'operation', offset: 2, revision: 'result-v1' });
    assert.match(await result().innerText(), new RegExp(text));
    await nav(name).getByRole('button', { name: '上一页' }).click();
  }
  assert.match(await result().innerText(), /stdout-sha-0/); assert.match(await result().innerText(), /尚无可确认引用/);
  cases.push('independent artifact/stage pages preserve real cursor, revision, all stage and log fields');
  await page.evaluate(() => window.fail = true); await nav('验收产物分页').getByRole('button', { name: '下一页' }).click(); await result().getByRole('alert').waitFor();
  assert.match(await result().innerText(), /artifact-0/); assert.match(await result().innerText(), /ORIGINAL RESULT/); assert.match(await result().innerText(), /上次内容仅供核对，请刷新/);
  assert.equal(await nav('验收阶段分页').getByRole('button', { name: '下一页' }).isDisabled(), true);
  await page.evaluate(() => window.fail = false); await result().getByRole('button', { name: '刷新验收结果', exact: true }).click(); await ready();
  assert.match(await result().innerText(), /artifact-0/); assert.equal(await result().getByRole('alert').count(), 0);
  cases.push('read failure retains metadata and both prior pages, locks continuation, same-version refresh recovers');
  for (const patch of [{ projectId: 'foreign' }, { operationId: 'foreign' }, { revision: 'foreign' }, { total: 4 }, { offset: 1 }, { nextOffset: 1 }, { entries: [] }, { entries: Array.from({ length: 21 }, () => artifacts[0]) }]) {
    await reset(); await result().getByRole('button', { name: '读取产物首页', exact: true }).click();
    await page.evaluate(p => window.patch = p, patch); await nav('验收产物分页').getByRole('button', { name: '下一页' }).click(); await result().getByRole('alert').waitFor();
    assert.match(await result().innerText(), /artifact-0/); assert.equal(await nav('验收产物分页').getByRole('button', { name: '下一页' }).isDisabled(), true);
  }
  cases.push('foreign identity, stale revision, changed totals, malformed cursor and oversized pages preserve original evidence');
  await reset(); await page.evaluate(() => window.hold = true); await result().getByRole('button', { name: '读取阶段首页', exact: true }).evaluate(b => { b.click(); b.click(); });
  await page.waitForFunction(() => typeof window.finish === 'function'); assert.equal(await page.evaluate(() => window.reads.filter(q => q.type === 'verificationResultStages').length), 1);
  await page.getByRole('button', { name: '查看项目 other 的作业', exact: true }).click();
  await page.getByRole('heading', { name: '项目 other 的作业', exact: true }).waitFor(); await page.evaluate(() => window.finish());
  assert.equal(await result().count(), 0);
  await page.getByRole('button', { name: '查看验收 operation 的结果', exact: true }).click(); await ready(); assert.match(await result().innerText(), /项目 other/);
  for (const patch of [{ projectId: 'foreign' }, { operationId: 'foreign' }, { taskId: 'foreign' }, { attemptId: 'foreign' }, { revision: '' }, { artifactCount: -1 }, { stageCount: 1.5 }, { state: 'released' }]) {
    await reset(); await page.evaluate(p => window.patch = p, patch);
    await result().getByRole('button', { name: '刷新验收结果', exact: true }).click(); await result().getByRole('alert').waitFor();
    assert.match(await result().innerText(), /ORIGINAL RESULT/); assert.match(await result().innerText(), /验收待对账/);
    assert.equal(await result().getByRole('button', { name: '读取产物首页', exact: true }).isDisabled(), true);
  }
  cases.push('metadata rejects foreign task/attempt/project/operation and malformed revision/count/state without replacing old result');
  cases.push('double-click guard and late page isolation on project switch');
  await reset(); await page.evaluate(() => window.hold = true); await result().getByRole('button', { name: '刷新验收结果', exact: true }).click(); await page.waitForFunction(() => typeof window.finish === 'function');
  await result().getByRole('button', { name: '关闭验收结果', exact: true }).click(); await page.evaluate(() => window.finish()); assert.equal(await result().count(), 0);
  cases.push('late metadata cannot reopen a closed result');
  for (const state of ['running', 'cancelling']) {
    await reset({ meta: { ...meta, state } }); await result().getByRole('button', { name: '读取产物首页', exact: true }).click();
    await page.evaluate(() => { window.meta = { ...window.meta, revision: 'result-v2' }; }); await page.clock.runFor(2100); await ready();
    assert.equal(await page.evaluate(() => window.reads.filter(q => q.type === 'verificationResult').length), 2);
    assert.equal(await page.evaluate(() => window.reads.filter(q => q.type === 'verificationResult').every(q => q.revision === null)), true);
    assert.equal(await result().getByText('产物页尚未读取。', { exact: true }).count(), 1);
    await page.evaluate(() => { window.meta = { ...window.meta, state: 'succeeded' }; }); await page.clock.runFor(2100); await ready();
    const stopped = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), stopped);
  }
  cases.push('running and cancelling poll metadata only at bounded interval with null revision; revision change invalidates pages; terminal stops');
  await reset({ meta: { ...meta, state: 'running' } }); await result().getByRole('button', { name: '关闭验收结果', exact: true }).click();
  const closed = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), closed);
  await reset({ meta: { ...meta, state: 'running' } }); await page.evaluate(() => window.fail = true); await page.clock.runFor(2100); await result().getByRole('alert').waitFor();
  const failed = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), failed); assert.match(await result().innerText(), /ORIGINAL RESULT/);
  await page.getByRole('button', { name: '关闭项目作业', exact: true }).click(); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), failed);
  cases.push('close, error, and leaving jobs stop active polling without retrying stale evidence');
  assert.equal(await page.evaluate(() => window.sends), 0); assert.equal(await page.evaluate(() => window.reads.some(q => q.type === 'snapshot')), false);
  assert.deepEqual(errors, []); assert.equal(networkRequests, 0); assert.deepEqual(await hashes(), before);
  await writeFile(join(dir, 'result.json'), JSON.stringify({ status: 'passed', cases, sourceBefore: before, sourceAfter: await hashes(), pageErrors: errors, networkRequests, modelCalls: 0, scope: 'Test-only record navigation host, WorkspaceJobs, WorkspaceVerificationResult with controlled workspace reads; not installed Tauri' }, null, 2));
  console.log(JSON.stringify({ status: 'passed', cases: cases.length, artifact: dir }));
} catch (e) {
  await page?.screenshot({ path: join(dir, 'failure.png'), fullPage: true }).catch(() => {});
  await writeFile(join(dir, 'failure.json'), JSON.stringify({ error: String(e), stack: e.stack, cases, errors }, null, 2)); throw e;
} finally { await browser?.close(); }
