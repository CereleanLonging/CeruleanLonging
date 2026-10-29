// Interactive production React components with an isolated recording sink.
// No native IPC, provider request, user application or installed product action.
import { readFile, writeFile, mkdtemp } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB, '1', 'Run in the dedicated owned Job wrapper');
const dir = await mkdtemp(resolve('reports/currency-product-ui-130-'));
const sourcePaths = ['CurrencyBudgetFields.tsx', 'WorkerProfiles.tsx', 'ProjectPlanning.tsx', 'WorkflowPlans.tsx'].map(name => `apps/desktop/src/${name}`);
async function sourceHashes() { return Object.fromEntries(await Promise.all(sourcePaths.map(async path => [path, createHash('sha256').update(await readFile(path)).digest('hex')]))); }
const sourceBefore = await sourceHashes();
const globalPolicy = { schemaVersion: 1, repeatedFailureThreshold: 3, maxAdvicePerAttempt: 1 };
const customPolicy = { schemaVersion: 1, repeatedFailureThreshold: 4, maxAdvicePerAttempt: 2, deliveryEnabled: true };
const snapshot = {
  workflows: [{ project: { id: 'project', root: 'D:\\fixture-project' }, state: { tasks: {} }, approval: null }],
  projectSetups: [], validators: [{ id: 'tests', name: '独立验收工具' }], managementDecisions: [],
  managementHistory: { projects: [] }, openaiConnection: { models: [], keyConfigured: false },
  codexConnection: { models: [], executable: 'D:\\fixture-codex.exe' },
  providerConnections: [], projectRuns: [], codexJobs: [], desktopBindings: [],
};
const entry = join(dir, 'entry.tsx');
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {ProjectPlanning} from '../../apps/desktop/src/ProjectPlanning';import {WorkflowPlans} from '../../apps/desktop/src/WorkflowPlans';const root=createRoot(document.getElementById('root')!);window.commands=[];let epoch=0;const send=async c=>{window.commands.push(c);return true;};window.renderPlanning=data=>root.render(<ProjectPlanning key={++epoch} data={data} busy={false} send={send}/>);window.renderApproval=workflows=>root.render(<WorkflowPlans key={++epoch} workflows={workflows} revisions={[]} busy={false} send={send}/>);window.renderPlanning(${JSON.stringify(snapshot)});`);
await build({ configFile: false, logLevel: 'error', define: { 'process.env.NODE_ENV': JSON.stringify('production') }, oxc: { jsx: { runtime: 'automatic' } }, build: { outDir: join(dir, 'bundle'), emptyOutDir: false, lib: { entry, name: 'WorkerSupervision', formats: ['iife'], fileName: () => 'ui.js' } } });
const script = await readFile(join(dir, 'bundle/ui.js'), 'utf8');
const css = await readFile('apps/desktop/src/style.css', 'utf8');
const cases = [], errors = [], commands = [];
let browser, page, networkRequests = 0;
function passed(name) { cases.push(name); }
async function lastCommand() { return page.evaluate(() => window.commands.at(-1)); }
async function saveConfiguration() {
  const count = await page.evaluate(() => window.commands.length);
  await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).click();
  await page.waitForFunction(expected => window.commands.length === expected, count + 1);
  const command = await lastCommand();
  assert.equal(command.type, 'configureProject'); commands.push(command);
  return command.draft;
}
function savedFixture(draft, digest) {
  const pin = { path: 'D:\\fixture-codex.exe', sha256: 'a'.repeat(64) };
  const acceptance = value => ({ schemaVersion: 1, stages: value.stages.map(s => ({ validatorId: s.validatorId, check: { id: s.id, name: s.name, args: s.args, timeoutSeconds: s.timeoutSeconds } })) });
  return { ...snapshot, projectSetups: [{ digest, setup: {
    ...draft, version: 1, worker: draft.workerAdapter === 'codex' ? { executable: pin, model: draft.workerModel } : undefined,
    policy: { maxRequests: draft.workerMaxRequests, maxSeconds: draft.maxSeconds, supervision: draft.supervision, currencyBudget: draft.currencyBudget },
    check: { id: 'check', args: draft.checkArgs, timeoutSeconds: 60 }, acceptance: acceptance(draft.acceptance),
    profiles: draft.profiles?.map(p => ({ id: p.id, description: p.description, workerAdapter: p.workerAdapter,
      supervisionOverride: p.supervisionOverride, resourceKeys: p.resourceKeys,
      worker: p.workerAdapter === 'codex' ? { executable: pin, model: p.workerModel } : undefined,
      apiWorker: p.apiWorker, artifactPaths: p.artifactPaths, acceptance: acceptance(p.acceptance) })),
  } }] };
}
try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  page.setDefaultTimeout(10000);
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent(`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>`);
  await page.addScriptTag({ content: script });
  await page.locator('#planning-requirement').fill('离线核查金额预算配置与审批');
  await page.locator('#planning-chief').fill('fixture-chief');
  await page.locator('#planning-deputy').fill('fixture-deputy');
  await page.locator('#planning-adapter').selectOption('openai');
  await page.locator('#planning-worker').fill('fixture-worker');
  await page.locator('#planning-artifacts').fill('sum.mjs');
  assert.equal((await saveConfiguration()).currencyBudget, undefined);
  passed('legacy setup omits currency settings');
  const save = page.getByRole('button', { name: '保存需求与执行配置', exact: true });
  await page.locator('#currency-enabled').check();
  await page.locator('#currency-limit').fill('12.345678901');
  await page.getByRole('button', { name: '按当前模型补充报价', exact: true }).click();
  assert.equal(await save.isDisabled(), true);
  for (let i = 0; i < 3; i++) {
    await page.locator(`#currency-${i}-input`).fill('1.234567');
    await page.locator(`#currency-${i}-output`).fill('2.5');
    await page.locator(`#currency-${i}-write`).fill('0');
    await page.locator(`#currency-${i}-from`).fill('2026-10-01T00:00');
    await page.locator(`#currency-${i}-until`).fill('2026-11-01T00:00');
    await page.locator(`#currency-${i}-source`).fill('隔离验证报价，不代表供应商实际价格');
  }
  let draft = await saveConfiguration();
  assert.equal(draft.currencyBudget.limitUsdNanos, 12345678901);
  assert.equal(draft.currencyBudget.quotes.length, 3);
  assert.deepEqual(draft.currencyBudget.quotes[0].rates, { input: 1234567, output: 2500000, cacheRead: null, cacheWrite: 0 });
  assert.equal(draft.currencyBudget.quotes[0].validFrom, '2026-10-01T00:00:00.000Z');
  passed('decimal input preserves all nine/six decimal places and unknown versus zero cache prices');
  const original = structuredClone(draft);
  for (const invalid of ['9007199.254740992', '-1', '1e2', '0.0000000001']) {
    await page.locator('#currency-limit').fill(invalid);
    assert.equal(await save.isDisabled(), true);
  }
  await page.locator('#currency-limit').fill('12.345678901');
  await page.locator('#currency-0-output').fill('1.0000001');
  assert.equal(await save.isDisabled(), true);
  await page.locator('#currency-0-output').fill('2.5');
  await page.locator('#currency-0-until').fill('2026-09-01T00:00');
  assert.equal(await save.isDisabled(), true);
  await page.locator('#currency-0-until').fill('2026-11-01T00:00');
  assert.equal(await save.isDisabled(), false);
  passed('overflow, negative/exponent, excessive precision and reversed validity block save');
  await page.locator('#planning-worker').fill('unquoted-worker');
  assert.equal(await save.isDisabled(), true);
  await page.locator('#planning-worker').fill('fixture-worker');
  assert.equal(await save.isDisabled(), false);
  passed('model change immediately invalidates missing quote without clearing prices');
  const restored = savedFixture(original, 'currency-saved');
  restored.openaiConnection.keyConfigured = true;
  await page.evaluate(data => window.renderPlanning(data), restored);
  await page.waitForFunction(() => document.querySelector('#currency-limit')?.value === '12.345678901');
  assert.equal(await page.locator('#currency-0-input').inputValue(), '1.234567');
  assert.equal(await page.locator('#currency-0-read').inputValue(), '');
  assert.equal(await page.getByRole('button', { name: '请求主指挥生成方案', exact: true }).isDisabled(), true);
  draft = await saveConfiguration();
  assert.deepEqual(draft.currencyBudget, original.currencyBudget);
  await page.getByRole('region', { name: '项目金额预算', exact: true }).screenshot({ path: join(dir, 'currency-configuration.png') });
  passed('saved setup restores exact quotes, can resave unchanged and prevents model request');
  await page.locator('#currency-enabled').uncheck();
  assert.equal((await saveConfiguration()).currencyBudget, undefined);
  passed('explicit disable omits settings from next configuration');
  const workflow = { project: snapshot.workflows[0].project, state: { tasks: {} }, revisionDigest: 'b'.repeat(64), approval: null, revision: {
    project: snapshot.workflows[0].project, policy: { tools: ['read-files'], maxRequests: 9, maxSeconds: 300, currencyBudget: original.currencyBudget },
    workflow: { version: 1, tasks: [{ id: 'task', goal: '编写求和函数', dependencies: [] }] },
    execution: [{ taskId: 'task', adapterId: 'openai', artifactPaths: ['sum.mjs'], checks: [] }],
  } };
  await page.evaluate(value => window.renderApproval([value]), workflow);
  await page.getByRole('button', { name: '审阅执行方案', exact: true }).click();
  const review = page.getByRole('region', { name: '批准的金额预算', exact: true });
  const text = await review.innerText();
  for (const expected of ['$12.345678901', 'fixture-chief', 'fixture-deputy', 'fixture-worker', '$1.234567', '$2.5', '缓存读 未知 / 写 $0', '2026-11-01', '不代表供应商实际价格']) assert.ok(text.includes(expected), expected);
  await review.screenshot({ path: join(dir, 'currency-approval.png') });
  await page.getByRole('button', { name: '确认执行授权', exact: true }).click();
  assert.deepEqual(await lastCommand(), { type: 'approveRevision', projectId: 'project', digest: 'b'.repeat(64) });
  passed('approval shows all quoted terms and binds reviewed revision digest');
  assert.deepEqual(errors, []); assert.equal(networkRequests, 0);
  const sourceAfter = await sourceHashes(); assert.deepEqual(sourceAfter, sourceBefore);
  await writeFile(join(dir, 'commands.json'), JSON.stringify(await page.evaluate(() => window.commands), null, 2));
  await writeFile(join(dir, 'result.json'), JSON.stringify({ status: 'passed', scope: 'production React components with recording sink; no native IPC or model requests', cases, pageErrors: errors, networkRequests, modelCalls: 0, sourceBefore, sourceAfter, artifact: dir }, null, 2));
  console.log(JSON.stringify({ status: 'passed', cases: cases.length, artifact: dir }));
} catch (error) {
  await page?.screenshot({ path: join(dir, 'failure.png'), fullPage: true }).catch(() => {});
  await writeFile(join(dir, 'failure.json'), JSON.stringify({ error: String(error), stack: error.stack, cases, pageErrors: errors, networkRequests }, null, 2));
  throw error;
} finally { await browser?.close(); }
