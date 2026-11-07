import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert(process.env.CERULEAN_CACHE_SESSION);
const run = await mkdtemp(resolve('reports/verification-retry-ui-'));
const output = join(process.env.CERULEAN_CACHE_SESSION, 'verification-retry-ui');
const job = { projectId: 'project', operationId: 'original', taskId: 'task', attemptId: 'attempt', state: 'failed', updatedAt: '2026-11-07T00:00:00Z' };
const detail = { ...job, revisionDigest: 'a'.repeat(64), manifestDigest: 'b'.repeat(64), evidenceDigest: 'c'.repeat(64), observedAt: '2026-11-07T00:00:00Z', ledgerRecorded: false, snapshotDigest: null, artifacts: [], stages: [], recovery: null, recoveryError: null };
const entry = join(run, 'entry.tsx');
await writeFile(entry, `import React,{useState} from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';import{VerificationJobs}from'../../apps/desktop/src/WorkflowPlans';
window.commands=[];const root=createRoot(document.getElementById('root'));
function Fixture({allowed,busy}){const[detail,setDetail]=useState(null);return <VerificationJobs jobs={[${JSON.stringify(job)}]} detail={detail} busy={busy} send={async command=>{window.commands.push(command);if(command.type==='readVerificationDetail')setDetail({...${JSON.stringify(detail)},canRetryStart:allowed});return true;}}/>;}
window.show=(allowed,busy=false)=>flushSync(()=>root.render(<Fixture key={String(allowed)+String(busy)} allowed={allowed} busy={busy}/>));`);
await build({ configFile: false, logLevel: 'error', define: { 'process.env.NODE_ENV': JSON.stringify('production') }, oxc: { jsx: { runtime: 'automatic' } }, build: { outDir: output, emptyOutDir: false, lib: { entry, name: 'RetryUI', formats: ['iife'], fileName: () => 'ui.js' } } });
const browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
const cases = [], errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  page.on('pageerror', error => errors.push(String(error)));
  const css = await readFile('apps/desktop/src/style.css', 'utf8');
  await page.route('**/*', route => route.request().url() === 'https://cerulean.test/'
    ? route.fulfill({ contentType: 'text/html', body: `<html lang="zh-CN"><meta charset="utf-8"><style>${css}</style><div id="root"></div></html>` })
    : route.abort());
  await page.goto('https://cerulean.test/');
  await page.addScriptTag({ path: join(output, 'ui.js') });
  const button = name => page.getByRole('button', { name, exact: true });
  await page.evaluate(() => window.show(false));
  await button('查看阶段结果').click();
  await page.getByRole('dialog', { name: '验收阶段结果' }).waitFor();
  assert.equal(await button('审阅重新启动检查').count(), 0);
  cases.push('Completed or uncertain check has no preflight restart action');
  await page.evaluate(() => { window.commands = []; window.show(true); });
  await button('查看阶段结果').click();
  await button('审阅重新启动检查').click();
  assert(await button('确认重新启动这一次检查').isDisabled());
  assert.deepEqual((await page.evaluate(() => window.commands)).map(c => c.type), ['readVerificationDetail']);
  cases.push('Read and review do not execute a check; an explanation is required');
  await page.getByLabel('已修复的启动问题').fill('已修复父目录权限预检');
  await page.getByRole('dialog').screenshot({ path: join(run, 'review.png') });
  await button('确认重新启动这一次检查').click();
  assert.deepEqual(await page.evaluate(() => window.commands.at(-1)), { type: 'retryVerificationStart', projectId: 'project', originalOperationId: 'original', evidenceDigest: 'c'.repeat(64), reason: '已修复父目录权限预检' });
  assert.equal(await page.getByRole('dialog').count(), 0);
  cases.push('One explicit confirmation sends the original evidence and closes the review');
  await page.evaluate(() => window.show(true, true));
  assert(await button('查看阶段结果').isDisabled());
  assert.deepEqual(errors, []);
  cases.push('Busy state prevents repeated submission');
  await writeFile(join(run, 'result.json'), JSON.stringify({ passed: true, cases, modelCalls: 0, scope: 'Browser fixture using production VerificationJobs' }, null, 2));
  console.log(JSON.stringify({ passed: true, cases: cases.length, report: run }));
} finally {
  await browser.close();
}
