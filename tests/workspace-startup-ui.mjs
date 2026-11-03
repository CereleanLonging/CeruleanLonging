import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';
if (process.env.CERULEAN_NATIVE_OWNED_JOB !== '1') throw Error('Use scripts/run-workspace-startup-ui-159.ps1 owned runner');
const dir = await mkdtemp('reports/workspace-startup-ui-159-'), cases = [], errors = [];
let browser, page, networkRequests = 0;
const files = ['WorkspaceCodexStartups.tsx', 'WorkspaceBrowser.tsx', 'contracts.ts'].map(n => 'apps/desktop/src/' + n).concat('tests/workspace-startup-ui.mjs');
const hashes = async () => Object.fromEntries(await Promise.all(files.map(async n => [n, createHash('sha256').update(await readFile(n)).digest('hex')])));
const before = await hashes(), entry = join(dir, 'entry.tsx');
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {WorkspaceRecordsHarness as WorkspaceBrowser} from '../../tests/fixtures/WorkspaceRecordsHarness';
const root=createRoot(document.getElementById('root')!);window.epoch=0;
window.render=()=>root.render(<React.StrictMode><div data-epoch={window.epoch}><WorkspaceBrowser key={window.epoch} enabled={true} busy={!!window.busy} showPlans={false} send={async()=>{window.sends++;throw Error('unexpected mutation')}} onOverview={()=>{}} read={async q=>{
window.reads.push(q);
if(q.type==='overview')return {type:q.type,value:{product:'local',protocolVersion:1,dataDirectory:'original',mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:2,message:null}};
if(q.type==='plans')return {type:q.type,value:{revision:'plans',offset:0,total:0,nextOffset:null,entries:[]}};
if(q.type==='projects')return {type:q.type,value:{revision:'projects',offset:0,total:2,nextOffset:null,entries:['p','other'].map(id=>({project:{id,root:id,timezone:'UTC'},revisionDigest:'plan',sequence:1,approved:false,taskCount:1,resourceHoldingTasks:1,unknownTasks:1}))}};
if(!['codexStartups','codexStartup'].includes(q.type))throw Error('unexpected read '+q.type);
window.active++;window.maxActive=Math.max(window.maxActive,window.active);
try{
if(window.fail)throw Error('original startup unavailable');
const rows=window.rows.map(s=>({...s,projectId:q.projectId}));
let value=q.type==='codexStartups'?{projectId:q.projectId,revision:window.revision,offset:q.offset,total:rows.length,nextOffset:q.offset+2<rows.length?q.offset+2:null,entries:rows.slice(q.offset,q.offset+2)}:{projectId:q.projectId,revision:window.revision,detail:{...window.meta,summary:rows.find(s=>s.attemptId===q.attemptId)}};
if(window.patch)value={...value,...window.patch};
const result={type:q.type,value};if(window.hold){window.hold=false;return await new Promise(resolve=>{window.finish=()=>resolve(result)})}return result;
}finally{window.active--}
}}/></div></React.StrictMode>);
window.set=s=>{window.epoch++;Object.assign(window,{reads:[],sends:0,active:0,maxActive:0,fail:false,patch:null,hold:false,finish:null,busy:false},s);window.render()};`);
await build({ configFile: false, logLevel: 'error', define: { 'process.env.NODE_ENV': JSON.stringify('production') }, oxc: { jsx: { runtime: 'automatic' } }, build: { outDir: join(dir, 'bundle'), emptyOutDir: false, lib: { entry, name: 'StartupUI', formats: ['iife'], fileName: () => 'ui.js' } } });
const rows = Array.from({ length: 3 }, (_, i) => ({ projectId: 'p', taskId: 'task-' + i, attemptId: 'attempt-' + i, managementOperation: i ? 'operation' : null, state: i === 0 ? 'prepared' : i === 1 ? 'advanced' : 'legacy', intentHash: i === 2 ? null : 'intent-' + i }));
const meta = { workspace: 'original-workspace', revisionDigest: 'original-plan', workerModel: 'original-model', startedAt: 'original-time', protocol: 'original-protocol', evidenceDigest: 'original-evidence', blockedReason: 'ORIGINAL BLOCKED', reason: 'ORIGINAL REASON', workflowSequence: 7 };
try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  page = await browser.newPage(); page.setDefaultTimeout(10000); await page.clock.install();
  page.on('pageerror', e => errors.push(e.message)); await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent('<html lang="zh-CN"><div id="root"></div></html>'); await page.addScriptTag({ content: await readFile(join(dir, 'bundle/ui.js'), 'utf8') });
  const region = () => page.getByRole('region', { name: '项目编程启动记录', exact: true });
  const nav = () => region().getByRole('navigation', { name: '启动记录分页', exact: true });
  const ready = async () => { await region().getByText('原项目共 3 条启动记录', { exact: true }).waitFor(); await page.waitForFunction(() => !Array.from(document.querySelectorAll('button')).find(b => b.textContent === '刷新启动首页')?.disabled); };
  const reset = async extra => {
    await page.evaluate(value => window.set(value), { rows, meta, revision: 'startup-v1', ...extra });
    await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch') === String(window.epoch));
    await page.getByRole('button', { name: '查看项目 p 的编程启动记录', exact: true }).click(); await ready();
  };
  const inspect = async () => { await region().getByRole('button', { name: '查看启动 attempt-0 的原证据', exact: true }).click(); await region().getByText('原阻断说明：ORIGINAL BLOCKED', { exact: true }).waitFor(); };
  await reset();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'codexStartups', projectId: 'p', offset: 0, revision: null });
  assert.equal(await page.evaluate(() => window.reads.some(q => q.type === 'jobs')), false);
  await inspect();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'codexStartup', projectId: 'p', attemptId: 'attempt-0', revision: 'startup-v1' });
  await region().getByText('原启动完整字段', { exact: true }).click();
  assert.deepEqual(JSON.parse(await region().locator('pre').innerText()), { projectId: 'p', revision: 'startup-v1', detail: { ...meta, summary: rows[0] } });
  cases.push('production project overview reaches startup intent without jobs; separate type/value page/detail and all original fields');
  await nav().getByRole('button', { name: '下一页' }).click(); await region().getByRole('button', { name: '查看启动 attempt-2 的原证据' }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'codexStartups', projectId: 'p', offset: 2, revision: 'startup-v1' });
  assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), true);
  await nav().getByRole('button', { name: '上一页' }).click(); await ready();
  cases.push('actual variable page cursor and revision preserved in both directions; legacy record visible');
  await inspect(); await page.evaluate(() => window.fail = true); await nav().getByRole('button', { name: '下一页' }).click(); await region().getByRole('alert').waitFor();
  assert.match(await region().innerText(), /ORIGINAL BLOCKED/); assert.match(await region().innerText(), /attempt-0/); assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), true);
  const failed = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), failed);
  await page.evaluate(() => window.fail = false); await region().getByRole('button', { name: '刷新启动首页' }).click(); await ready(); assert.equal(await region().getByRole('alert').count(), 0);
  cases.push('failed read keeps prior page and detail, disables continuation and stops polling; manual refresh recovers');
  for (const patch of [{ projectId: 'foreign' }, { revision: 'foreign' }, { total: 4 }, { offset: 1 }, { nextOffset: 1 }, { entries: [] }, { entries: Array.from({ length: 21 }, () => rows[0]) }, { entries: [rows[0], rows[0]] }, { entries: [{ ...rows[0], projectId: 'foreign' }] }, { entries: [{ ...rows[0], state: 'invented' }] }]) {
    await reset(); await inspect(); await page.evaluate(p => window.patch = p, patch); await nav().getByRole('button', { name: '下一页' }).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(), /ORIGINAL BLOCKED/); assert.match(await region().innerText(), /attempt-0/);
  }
  cases.push('foreign project/state, wrong revision/total/offset/cursor, empty, duplicate and oversized pages rejected without replacing evidence');
  for (const patch of [{ projectId: 'foreign' }, { revision: 'foreign' }, { detail: { ...meta, summary: { ...rows[0], taskId: 'foreign' } } }, { detail: { ...meta, summary: { ...rows[0], attemptId: 'foreign' } } }, { detail: { ...meta, summary: { ...rows[0], intentHash: 'foreign' } } }, { detail: { ...meta, workflowSequence: -1, summary: rows[0] } }]) {
    await reset(); await inspect(); await page.evaluate(p => window.patch = p, patch); await region().getByRole('button', { name: '查看启动 attempt-0 的原证据' }).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(), /ORIGINAL BLOCKED/);
  }
  cases.push('detail identity, revision, task, attempt, intent and workflow sequence refusal retains original detail');
  await reset(); await inspect(); await page.evaluate(() => window.patch = { entries: [{ ...window.rows[0], intentHash: 'x'.repeat(65536) }] }); await region().getByRole('button', { name: '刷新启动首页' }).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(), /ORIGINAL BLOCKED/);
  await reset(); await inspect(); await page.evaluate(() => window.patch = { detail: { ...window.meta, summary: window.rows[0], reason: 'x'.repeat(524288) } }); await region().getByRole('button', { name: '查看启动 attempt-0 的原证据' }).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(), /ORIGINAL BLOCKED/);
  await reset(); await inspect(); await page.evaluate(() => window.rows[0].intentHash = 'changed-without-revision'); await region().getByRole('button', { name: '刷新启动首页' }).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(), /ORIGINAL BLOCKED/);
  cases.push('page/detail byte bounds and inconsistent same-version content refused without losing prior evidence');
  await reset(); await page.evaluate(() => window.hold = true); await region().getByRole('button', { name: '查看启动 attempt-0 的原证据' }).evaluate(b => { b.click(); b.click(); }); await page.waitForFunction(() => typeof window.finish === 'function');
  assert.equal(await page.evaluate(() => window.reads.filter(q => q.type === 'codexStartup').length), 1);
  await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.maxActive), 1);
  await page.getByRole('button', { name: '查看项目 other 的编程启动记录', exact: true }).click(); await region().getByRole('heading', { name: '项目 other 的编程启动记录' }).waitFor(); await page.evaluate(() => window.finish()); assert.equal(await region().getByRole('region', { name: '原启动证据' }).count(), 0);
  cases.push('double click and active polling do not overlap; late detail isolated on project switch');
  await reset(); await inspect(); await page.evaluate(() => window.hold = true); await region().getByRole('button', { name: '查看启动 attempt-0 的原证据' }).click(); await page.waitForFunction(() => typeof window.finish === 'function'); await region().getByRole('button', { name: '关闭启动详情' }).click(); await page.evaluate(() => window.finish()); assert.equal(await region().getByRole('region', { name: '原启动证据' }).count(), 0);
  await reset(); await page.evaluate(() => window.hold = true); await region().getByRole('button', { name: '刷新启动首页' }).click(); await page.waitForFunction(() => typeof window.finish === 'function'); await page.getByRole('button', { name: '关闭项目启动记录' }).click(); await page.evaluate(() => window.finish()); await page.clock.runFor(10000); assert.equal(await region().count(), 0);
  cases.push('late detail cannot reopen closed detail; late page cannot reopen unmounted project');
  for (const state of ['prepared', 'advanced']) {
    await reset({ rows: rows.map(s => ({ ...s, state })) }); await inspect(); const count = await page.evaluate(() => window.reads.length); await page.clock.runFor(2100); assert.equal(await page.evaluate(() => window.reads.length), count + 1); assert.equal(await page.evaluate(() => window.reads.at(-1).type), 'codexStartups'); assert.equal(await page.evaluate(() => window.reads.at(-1).revision), null);
    await page.evaluate(() => { window.revision = 'startup-v2'; window.rows = window.rows.map(s => ({ ...s, state: 'closed' })); }); await page.clock.runFor(2100); assert.equal(await region().getByRole('region', { name: '原启动证据' }).count(), 0); const terminal = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), terminal);
  }
  cases.push('prepared/advanced poll one bounded page after 2 seconds without detail requests; changed revision invalidates detail and closed stops');
  await reset(); await page.evaluate(() => { window.busy = true; window.render(); }); await page.waitForFunction(() => Array.from(document.querySelectorAll('button')).find(b => b.textContent === '刷新启动首页')?.disabled); const paused = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), paused); await page.evaluate(() => { window.busy = false; window.render(); }); await ready(); await page.clock.runFor(2100); assert.equal(await page.evaluate(() => window.reads.length), paused + 1);
  await page.evaluate(value => window.set(value), { rows: [], meta, revision: 'empty' }); await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch') === String(window.epoch)); await page.getByRole('button', { name: '查看项目 p 的编程启动记录', exact: true }).click(); await region().getByText('原项目尚无 Codex 启动记录；可刷新核对最新记录。', { exact: true }).waitFor(); const empty = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), empty); assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), true);
  await page.evaluate(value => window.set(value), { rows, meta, revision: 'startup-v1', fail: true }); await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch') === String(window.epoch)); await page.getByRole('button', { name: '查看项目 p 的编程启动记录', exact: true }).click(); await region().getByRole('alert').waitFor(); assert.match(await region().innerText(), /启动记录尚未取得/); const initialFailure = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), initialFailure);
  cases.push('busy pauses active polling and resumes without overlap; true empty page guides refresh; initial error never claims empty');
  await reset(); await page.getByRole('button', { name: '关闭项目启动记录' }).click(); const closed = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), closed);
  assert.equal(await page.evaluate(() => window.sends), 0); assert.equal(await page.evaluate(() => window.reads.some(q => !['overview', 'plans', 'projects', 'codexStartups', 'codexStartup'].includes(q.type))), false);
  assert.deepEqual(errors, []); assert.equal(networkRequests, 0); assert.deepEqual(await hashes(), before);
  await writeFile(join(dir, 'result.json'), JSON.stringify({ status: 'passed', cases, sourceBefore: before, sourceAfter: await hashes(), pageErrors: errors, networkRequests, modelCalls: 0, schema: { pageQuery: { type: 'codexStartups', projectId: 'p', offset: 0, revision: null }, detailQuery: { type: 'codexStartup', projectId: 'p', attemptId: 'attempt-0', revision: 'startup-v1' }, reply: '{type,value}' }, scope: 'Test-only record navigation host and WorkspaceCodexStartups with offline controlled reads; not installed Tauri' }, null, 2));
  console.log(JSON.stringify({ status: 'passed', cases: cases.length, artifact: dir }));
} catch (e) {
  await writeFile(join(dir, 'result.json'), JSON.stringify({ status: 'failed', cases, error: String(e), pageErrors: errors, networkRequests }, null, 2));
  throw e;
} finally { await browser?.close(); }
