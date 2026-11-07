import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert(process.env.CERULEAN_CACHE_SESSION, 'Managed cache required');
const report = resolve(process.argv[2]);
const scratch = join(process.env.CERULEAN_CACHE_SESSION, 'scratch/qoder-version-ui');
await mkdir(scratch, { recursive: true });
const entry = join(scratch, 'entry.tsx');
const source = resolve('apps/desktop/src').replaceAll('\\', '/');
await writeFile(entry, `import React from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';
import {ProjectRuns} from '${source}/ProjectRuns';
import {DesktopSessions} from '${source}/DesktopSessions';
const root=createRoot(document.getElementById('root'));
window.commands=[];
const send=async command=>{window.commands.push(command);return true;};
const read=async query=>({type:'projectActivity',value:{projectId:query.projectId,entries:[],total:0,head:null,hasOlder:false,hasNewer:false}});
window.show=(data,kind)=>flushSync(()=>root.render(kind==='sessions'?<DesktopSessions data={data} busy={false} send={send}/>:<ProjectRuns data={data} busy={false} send={send} read={read}/>));`);
await build({ configFile: false, logLevel: 'error', define: { 'process.env.NODE_ENV': JSON.stringify('production') },
  oxc: { jsx: { runtime: 'automatic' } }, build: { outDir: join(scratch, 'bundle'), emptyOutDir: false,
    lib: { entry, name: 'QoderVersionUI', formats: ['iife'], fileName: () => 'ui.js' } } });
function snapshot(version, bridge = 'bridge') {
  const binding = { id: 'binding', projectId: 'project', label: 'isolated Qoder', bridgeSha256: bridge,
    observation: { window: { app: 'qoder', version, pid: 1, hwnd: '1', title: 'Fixture' }, sessionId: 'session', modelLabel: 'fixture-model', draftEmpty: true, busy: false } };
  return { dataDirectory: 'synthetic', desktopAccess: { bridgeSha256: 'bridge', bindings: [binding], windows: [], trials: [], tasks: [] },
    openaiConnection: { keyConfigured: true }, providerCredentials: [],
    codexJobs: [], projectRuns: [], projectSetups: [{ digest: 'setup', setup: { projectId: 'project', chiefModel: 'chief', deputyModel: 'deputy', managementMaxRequests: 10 } }],
    workflows: [{ project: { id: 'project', root: 'D:/fixture' }, revisionDigest: 'revision', approval: {},
      revision: { execution: [{ taskId: 'task', adapterId: 'qoder', workerDirectory: { path: 'D:/fixture/executors/worker' }, desktopWorker: { binding } }] },
      state: { tasks: { task: { state: 'pending' } } } }] };
}
let browser;
const cases = [];
const errors = [];
let networkRequests = 0;
let failure;
try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  const page = await browser.newPage();
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent('<meta charset="utf-8"><div id="root"></div>');
  await page.addScriptTag({ content: await readFile(join(scratch, 'bundle/ui.js'), 'utf8') });
  const show = async (data, kind = 'runs') => {
    await page.evaluate(({ data, kind }) => window.show(data, kind), { data, kind });
    if (kind === 'sessions') await page.locator('summary').filter({ hasText: '已保存的会话关联' }).click();
  };
  const start = () => page.getByRole('button', { name: '启动已批准项目', exact: true });
  for (const version of ['0.4.3.0', '0.4.8.0']) {
    await show(snapshot(version));
    assert(await start().isEnabled());
    await start().click();
    await page.getByRole('button', { name: '确认启动调度', exact: true }).click();
    await show(snapshot(version), 'sessions');
    await page.getByRole('button', { name: '准备固定消息验证', exact: true }).click();
    cases.push(`${version} permits reviewed run and fixed-message preparation`);
  }
  for (const version of ['0.4.8.1', '0.4.9.0', '0.5.0.0']) {
    await show(snapshot(version));
    assert(await start().isDisabled());
    await show(snapshot(version), 'sessions');
    assert.equal(await page.getByRole('button', { name: '准备固定消息验证', exact: true }).count(), version === '0.5.0.0' ? 0 : 1);
    cases.push(`${version} remains gated`);
  }
  await show(snapshot('0.4.8.0', 'changed'));
  assert(await start().isDisabled());
  cases.push('changed observer identity blocks run');
  const commands = await page.evaluate(() => window.commands);
  assert.deepEqual(commands.map(command => command.type), ['startProjectRun', 'prepareDesktopTrial', 'startProjectRun', 'prepareDesktopTrial']);
  assert.deepEqual(errors, []);
  assert.equal(networkRequests, 0);
} catch (error) { failure = error.stack; }
finally { await browser?.close(); }
const result = { passed: !failure, error: failure, cases, errors, networkRequests, realModelRequests: 0, browserClosed: !browser?.isConnected() };
await writeFile(join(report, 'qoder-version-ui.json'), JSON.stringify(result, null, 2), { flag: 'wx' });
console.log(JSON.stringify(result));
if (failure) process.exitCode = 1;
