// Interactive production React components with an isolated recording sink.
// No native IPC, provider request, user application or installed product action.
import { readFile, writeFile, mkdtemp } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB, '1', 'Run in the dedicated owned Job wrapper');
const dir = await mkdtemp(resolve('reports/worker-supervision-ui-'));
const sourcePaths = ['SupervisionFields.tsx', 'WorkerProfiles.tsx', 'ProjectPlanning.tsx', 'WorkflowPlans.tsx', 'ManagementReview.tsx', 'WorkspaceManagementDecisions.tsx'].map(name => `apps/desktop/src/${name}`);
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
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {ProjectPlanning} from '../../apps/desktop/src/ProjectPlanning';import {WorkflowPlans} from '../../apps/desktop/src/WorkflowPlans';import {supervisionDeliverySummary} from '../../apps/desktop/src/SupervisionFields';const root=createRoot(document.getElementById('root')!);window.commands=[];let epoch=0;const send=async c=>{window.commands.push(c);return true;};window.renderPlanning=data=>root.render(<ProjectPlanning key={++epoch} data={data} busy={false} send={send}/>);window.renderApproval=workflows=>root.render(<WorkflowPlans key={++epoch} workflows={workflows} revisions={[]} busy={false} send={send}/>);window.renderDelivery=advice=>root.render(<p aria-label="OMP建议状态">{supervisionDeliverySummary(advice)}</p>);window.renderPlanning(${JSON.stringify(snapshot)});`);
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
    ...draft, version: 1, worker: ['codex', 'omp'].includes(draft.workerAdapter) ? { executable: { ...pin, path: draft.codexExecutable }, model: draft.workerModel } : undefined,
    policy: { maxRequests: draft.workerMaxRequests, maxSeconds: draft.maxSeconds, supervision: draft.supervision },
    check: { id: 'check', args: draft.checkArgs, timeoutSeconds: 60 }, acceptance: acceptance(draft.acceptance),
    profiles: draft.profiles?.map(p => ({ id: p.id, description: p.description, workerAdapter: p.workerAdapter,
      supervisionOverride: p.supervisionOverride, resourceKeys: p.resourceKeys,
      worker: ['codex', 'omp'].includes(p.workerAdapter) ? { executable: { ...pin, path: p.codexExecutable }, model: p.workerModel } : undefined,
      apiWorker: p.apiWorker, artifactPaths: p.artifactPaths, acceptance: acceptance(p.acceptance) })),
  } }] };
}
async function fillProfile(index, adapter) {
  await page.getByRole('button', { name: '添加执行配置', exact: true }).click();
  await page.locator(`#profile-${index}-id`).fill(`profile-${index}`);
  await page.locator(`#profile-${index}-description`).fill(`隔离测试配置 ${index}`);
  await page.locator(`#profile-${index}-adapter`).selectOption(adapter);
  if (adapter === 'codex' || adapter === 'omp') await page.locator(`#profile-${index}-exe`).fill(`D:\\fixture-${adapter}.exe`);
  await page.locator(`#profile-${index}-model`).fill(`fixture-model-${index}`);
  await page.locator(`#profile-${index}-artifacts`).fill(`result-${index}.txt`);
}

try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  page = await browser.newPage({ viewport: { width: 1200, height: 980 }, deviceScaleFactor: 1 });
  page.setDefaultTimeout(10000);
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent(`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>`);
  await page.addScriptTag({ content: script });
  await page.locator('#planning-requirement').fill('验证独立执行配置的监督授权');
  await page.locator('#planning-chief').fill('fixture-chief');
  await page.locator('#planning-deputy').fill('fixture-deputy');
  await page.locator('#planning-worker').fill('fixture-default');
  await page.locator('#planning-artifacts').fill('default.txt');
  let draft = await saveConfiguration();
  assert.equal(draft.supervision, undefined); passed('legacy default omits supervision');

  await page.getByLabel('记录连续命令失败', { exact: true }).check();
  await page.locator('#supervision-threshold').fill('3');
  await page.locator('#supervision-advice-limit').fill('1');
  await fillProfile(0, 'deepseek');
  assert.equal(await page.locator('#profile-0-supervision-mode').inputValue(), 'disabled');
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, globalPolicy);
  assert.deepEqual(draft.profiles[0].supervisionOverride, { mode: 'disabled' });
  passed('new API profile explicitly disabled and retains global Codex supervision');

  await page.locator('#profile-0-description').fill('修改混合配置说明');
  assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isChecked(), true);
  assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isDisabled(), false);
  assert.equal(await page.locator('#supervision-threshold').inputValue(), '3');
  passed('editing mixed profiles does not clear or disable global supervision');
  assert.equal(await page.locator('#profile-0-supervision-mode option[value=enabled]').isDisabled(), false);
  await page.locator('#profile-0-supervision-mode').selectOption('inherit');
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);
  draft = await saveConfiguration();
  assert.equal(draft.profiles[0].supervisionOverride, undefined);
  passed('API profile can inherit approved record-only supervision');
  await page.getByLabel('允许向当前执行轮次发送建议', { exact: true }).check();
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);
  draft = await saveConfiguration();
  assert.equal(draft.supervision.deliveryEnabled, true);
  passed('API inherited delivery grant is explicitly preserved in save');
  await page.getByLabel('允许向当前执行轮次发送建议', { exact: true }).uncheck();
  globalPolicy.deliveryEnabled = false;
  await page.locator('#profile-0-supervision-mode').selectOption('disabled');
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);

  await fillProfile(1, 'codex');
  await page.locator('#profile-1-supervision-mode').selectOption('enabled');
  assert.equal(await page.locator('#profile-1-supervision-threshold').inputValue(), '3');
  await page.locator('#profile-1-supervision-threshold').fill('4');
  await page.locator('#profile-1-supervision-limit').fill('2');
  await page.getByRole('region', { name: '执行配置 2', exact: true }).getByLabel('允许向此配置的当前执行轮次发送建议').check();
  await fillProfile(2, 'codex');
  await page.locator('#profile-2-supervision-mode').selectOption('inherit');
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, globalPolicy);
  assert.deepEqual(draft.profiles[1].supervisionOverride, { mode: 'enabled', policy: customPolicy });
  assert.equal(draft.profiles[2].supervisionOverride, undefined);
  passed('three-state exact save keeps explicit custom enabled and legacy absent inheritance');
  const enabledDraft = structuredClone(draft);
  await page.getByRole('region', { name: '执行配置 2', exact: true }).getByRole('region', { name: '执行配置异常监督' }).screenshot({ path: join(dir, 'custom-supervision.png') });

  await page.locator('#profile-1-supervision-limit').fill('0');
  const delivery = page.getByRole('region', { name: '执行配置 2', exact: true }).getByLabel('允许向此配置的当前执行轮次发送建议');
  assert.equal(await delivery.isChecked(), false); assert.equal(await delivery.isDisabled(), true);
  draft = await saveConfiguration();
  assert.equal(draft.profiles[1].supervisionOverride.policy.deliveryEnabled, false);
  assert.match(await page.getByRole('region', { name: '执行配置 2', exact: true }).innerText(), /不请求或发送建议/);
  passed('zero advice clears delivery grant and accurately describes record-only behavior');

  const restored = savedFixture(enabledDraft, 'saved-three-states');
  await page.evaluate(data => window.renderPlanning(data), restored);
  await page.waitForFunction(() => document.querySelector('#profile-1-supervision-threshold')?.value === '4');
  assert.equal(await page.locator('#profile-0-supervision-mode').inputValue(), 'disabled');
  assert.equal(await page.locator('#profile-1-supervision-mode').inputValue(), 'enabled');
  assert.equal(await page.locator('#profile-2-supervision-mode').inputValue(), 'inherit');
  assert.equal(await page.locator('#profile-1-supervision-limit').inputValue(), '2');
  assert.equal(await page.getByRole('region', { name: '执行配置 2', exact: true }).getByLabel('允许向此配置的当前执行轮次发送建议').isChecked(), true);
  assert.equal(await page.locator('#supervision-threshold').inputValue(), '3');
  passed('saved three-state profile policies and independent global policy restore after remount');
  const restoredDraft = await saveConfiguration();
  assert.deepEqual(restoredDraft.profiles.map(p => p.supervisionOverride), enabledDraft.profiles.map(p => p.supervisionOverride));
  passed('restored policies survive another save without collapsing absent and disabled');

  const unsupported = structuredClone(restored);
  unsupported.projectSetups[0].digest = 'saved-invalid-api-enabled';
  unsupported.projectSetups[0].setup.profiles[0].supervisionOverride = { mode: 'enabled', policy: customPolicy };
  await page.evaluate(data => window.renderPlanning(data), unsupported);
  await page.waitForFunction(() => document.querySelector('#profile-0-supervision-mode')?.value === 'enabled');
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);
  assert.equal(await page.locator('#profile-0-supervision-threshold').inputValue(), '4');
  const unsupportedDelivery = page.getByRole('region', { name: '执行配置 1', exact: true }).getByLabel('允许向此配置的当前执行轮次发送建议');
  assert.equal(await unsupportedDelivery.isChecked(), true);
  assert.equal(await unsupportedDelivery.isDisabled(), false);
  assert.equal(await page.getByRole('region', { name: '执行配置 1', exact: true }).getByRole('alert').count(), 0);
  await page.getByRole('region', { name: '执行配置 1', exact: true }).getByRole('region', { name: '执行配置异常监督' }).screenshot({ path: join(dir, 'unsupported-policy.png') });
  await page.locator('#profile-0-supervision-mode').selectOption('disabled');
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);
  passed('persisted API delivery remains visible and can be explicitly disabled');

  await page.locator('#profile-1-adapter').selectOption('deepseek');
  assert.equal(await page.locator('#profile-1-supervision-mode').inputValue(), 'enabled');
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);
  assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isChecked(), true);
  passed('changing to API adapter preserves explicit delivery choice');

  await page.evaluate(data => window.renderPlanning(data), restored);
  await page.waitForFunction(() => document.querySelector('#profile-1-supervision-threshold')?.value === '4');
  await page.locator('#planning-adapter').selectOption('deepseek');
  await page.locator('#planning-worker').fill('fixture-default-api');
  assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isChecked(), true);
  assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isDisabled(), false);
  assert.equal(await page.getByLabel('允许向当前执行轮次发送建议', { exact: true }).isDisabled(), false);
  assert.equal(await page.locator('#profile-1-supervision-threshold').inputValue(), '4');
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, globalPolicy);
  assert.deepEqual(draft.profiles[1].supervisionOverride, { mode: 'enabled', policy: customPolicy });
  passed('API default preserves record-only policy and separately supervised Codex profile');
  await page.locator('#profile-0-supervision-mode').selectOption('inherit');
  draft = await saveConfiguration();
  assert.equal(draft.profiles[0].supervisionOverride, undefined);
  passed('API adapter can inherit non-delivery global policy');

  const apiSaved = savedFixture(draft, 'api-recording-restored');
  await page.evaluate(data => window.renderPlanning(data), apiSaved);
  await page.waitForFunction(() => document.querySelector('#planning-adapter')?.value === 'deepseek');
  assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isChecked(), true);
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, globalPolicy);
  passed('API record-only default survives remount and exact save');

  const invalidDefault = structuredClone(apiSaved);
  invalidDefault.projectSetups[0].digest = 'api-default-invalid-delivery';
  invalidDefault.projectSetups[0].setup.policy.supervision = customPolicy;
  await page.evaluate(data => window.renderPlanning(data), invalidDefault);
  await page.waitForFunction(() => document.querySelector('#supervision-threshold')?.value === '4');
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);
  const defaultDelivery = page.getByLabel('允许向当前执行轮次发送建议', { exact: true });
  assert.equal(await defaultDelivery.isChecked(), true);
  await defaultDelivery.uncheck();
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, { ...customPolicy, deliveryEnabled: false });
  passed('restored API delivery grant can be explicitly disabled without losing policy');

  for (const adapter of ['openai', 'anthropic', 'deepseek', 'qwen', 'kimi', 'grok']) {
    await page.locator('#planning-adapter').selectOption(adapter);
    await page.locator('#planning-worker').fill('fixture-api-model');
    if (adapter === 'qwen') await page.locator('#planning-api-workspace').fill('fixture-workspace');
    assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isChecked(), true);
    assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isDisabled(), false);
    assert.equal(await defaultDelivery.isDisabled(), false);
    await defaultDelivery.check();
    draft = await saveConfiguration();
    assert.equal(draft.supervision.deliveryEnabled, true);
    await defaultDelivery.uncheck();
  }
  passed('all six API selections preserve explicit delivery and record-only policy');
  await page.locator('#planning-adapter').selectOption('omp');
  await page.locator('#planning-executable').fill('D:\\fixture-omp.exe');
  await page.locator('#planning-worker').fill('fixture-omp-model');
  assert.equal(await page.getByLabel('记录连续文件工具失败', { exact: true }).isChecked(), true);
  assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), false);
  assert.equal(await defaultDelivery.isDisabled(), false);
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, { ...customPolicy, deliveryEnabled: false });
  passed('OMP default preserves and saves record-only supervision');
  await defaultDelivery.check();
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, customPolicy);
  assert.match(await page.getByRole('region', { name: '异常监督配置', exact: true }).innerText(), /原文件工具.*原任务队列与消息流.*结果未知不重发/s);
  passed('OMP default explicitly enables delivery without changing supervision bounds');

  await page.locator('#profile-1-adapter').selectOption('omp');
  await page.locator('#profile-1-exe').fill('D:\\fixture-omp-profile.exe');
  await page.locator('#profile-1-model').fill('fixture-omp-profile-model');
  const ompProfileDelivery = page.getByRole('region', { name: '执行配置 2', exact: true }).getByLabel('允许向此配置的当前执行轮次发送建议');
  assert.equal(await ompProfileDelivery.isChecked(), true);
  assert.equal(await ompProfileDelivery.isDisabled(), false);
  assert.equal(await page.getByRole('region', { name: '执行配置 2', exact: true }).getByRole('alert').count(), 0);
  await ompProfileDelivery.uncheck();
  draft = await saveConfiguration();
  assert.deepEqual(draft.profiles[1].supervisionOverride, { mode: 'enabled', policy: { ...customPolicy, deliveryEnabled: false } });
  assert.deepEqual(draft.supervision, customPolicy);
  const ompSaved = savedFixture(draft, 'omp-default-delivery-profile-recording');
  await page.evaluate(data => window.renderPlanning(data), ompSaved);
  await page.waitForFunction(() => document.querySelector('#planning-adapter')?.value === 'omp' && document.querySelector('#profile-1-adapter')?.value === 'omp');
  assert.equal(await defaultDelivery.isChecked(), true);
  assert.equal(await ompProfileDelivery.isChecked(), false);
  draft = await saveConfiguration();
  assert.deepEqual(draft.supervision, customPolicy);
  assert.equal(draft.profiles[1].supervisionOverride.policy.deliveryEnabled, false);
  passed('OMP delivery and record-only profile policies restore independently and survive exact save');
  await page.locator('#supervision-advice-limit').fill('0');
  assert.equal(await defaultDelivery.isChecked(), false);
  assert.equal(await defaultDelivery.isDisabled(), true);
  draft = await saveConfiguration();
  assert.equal(draft.supervision.maxAdvicePerAttempt, 0);
  assert.equal(draft.supervision.deliveryEnabled, false);
  passed('OMP zero advice quota clears delivery and keeps it disabled');

  for (const adapter of ['qoder', 'workbuddy']) {
    await page.evaluate(data => window.renderPlanning(data), ompSaved);
    await page.waitForFunction(() => document.querySelector('#planning-adapter')?.value === 'omp' && document.querySelector('#supervision-advice-limit')?.value === '2');
    await page.locator('#planning-adapter').selectOption(adapter);
    assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isChecked(), true);
    assert.equal(await page.getByRole('button', { name: '保存需求与执行配置', exact: true }).isDisabled(), true);
    await page.getByLabel('记录连续命令失败', { exact: true }).uncheck();
    assert.equal(await page.getByLabel('记录连续命令失败', { exact: true }).isDisabled(), true);
  }
  passed('Qoder and WorkBuddy remain gated until unsupported default supervision is explicitly disabled');

  const ompAdvice = { omp: { requestId: 'original-omp-request', toolHead: 'a'.repeat(64) }, api: null,
    incidentId: 'original-incident', taskId: 'original-task', attemptId: 'original-attempt',
    threadId: '', turnId: '', observationDigest: 'b'.repeat(64), suggestion: 'Original advice', rationale: 'Original failures', current: false, delivered: true };
  for (const [deliveryState, expected] of [
    ['recordOnly', /仅保存供审阅，未向原任务发送/],
    ['awaitingApplication', /等待原任务应用建议，尚未确认投送/],
    ['appliedLocally', /本地入账，尚无原任务接收证明/],
    ['applicationUnknown', /待核对，不会自动重发/],
    ['adviceOutcomeUnknown', /待核对，不会自动重发/],
    ['commandAccepted', /已确认建议命令；尚未确认建议入队/],
    ['queued', /进入原任务队列，尚未在原任务消息流中观察到建议/],
    ['toolResultReleased', /本地刷新到传输通道；尚无对端接收确认/],
    ['messageObserved', /原任务消息流已观察到建议，不代表模型执行或验收/],
    ['notSent', /建议在发送前撤回，原任务继续执行/],
    ['unrecognized', /状态待核对，不会自动重发/],
  ]) {
    await page.evaluate(value => window.renderDelivery(value), { ...ompAdvice, deliveryState });
    const status = page.getByLabel('OMP建议状态', { exact: true });
    await status.waitFor();
    await page.waitForFunction(([source, flags]) => new RegExp(source, flags).test(document.querySelector('[aria-label="OMP建议状态"]')?.textContent ?? ''), [expected.source, expected.flags]);
    const text = await status.innerText();
    assert.match(text, expected);
    if (deliveryState !== 'messageObserved') assert.doesNotMatch(text, /原任务消息流已观察到建议/);
  }
  passed('OMP stages separate local flush, command acceptance, queue evidence and original message observation despite stale delivered flags');

  const taskCases = [
    { id: 'default', goal: '默认继承监督任务', adapterId: 'codex', supervisionOverride: undefined, expected: /沿用项目默认.*连续失败 3 次.*最多 1 条建议/ },
    { id: 'custom', goal: '单独投送监督任务', adapterId: 'codex', profileId: 'profile-custom', supervisionOverride: { mode: 'enabled', policy: customPolicy }, expected: /单独设置.*连续失败 4 次.*最多 2 条建议.*允许向原执行轮次发送建议/ },
    { id: 'disabled', goal: 'API关闭监督任务', adapterId: 'deepseek', profileId: 'profile-api', supervisionOverride: { mode: 'disabled' }, expected: /单独设置.*未启用异常监督/ },
    { id: 'record', goal: '只记录不请求建议任务', adapterId: 'codex', profileId: 'profile-record', supervisionOverride: { mode: 'enabled', policy: { schemaVersion: 1, repeatedFailureThreshold: 5, maxAdvicePerAttempt: 0 } }, expected: /单独设置.*连续失败 5 次.*不请求或发送建议/ },
    { id: 'omp-record', goal: 'OMP只保存建议任务', adapterId: 'omp', profileId: 'profile-omp-record', supervisionOverride: { mode: 'enabled', policy: { ...customPolicy, deliveryEnabled: false } }, expected: /单独设置.*最多 2 条建议.*可请求并保存建议，不向执行器发送/ },
    { id: 'omp-delivery', goal: 'OMP原任务投送建议', adapterId: 'omp', profileId: 'profile-omp-delivery', supervisionOverride: { mode: 'enabled', policy: customPolicy }, expected: /单独设置.*最多 2 条建议.*允许向原执行轮次发送建议/ },
  ];
  const workflow = { project: snapshot.workflows[0].project, state: { tasks: {} }, revisionDigest: 'b'.repeat(64), approval: null, revision: {
    project: snapshot.workflows[0].project, policy: { tools: ['read-files'], maxRequests: 9, maxSeconds: 300, budgetUsdCents: null, supervision: globalPolicy },
    workflow: { version: 1, tasks: taskCases.map(t => ({ id: t.id, goal: t.goal, dependencies: [] })) },
    execution: taskCases.map(t => ({ taskId: t.id, adapterId: t.adapterId, profileId: t.profileId, supervisionOverride: t.supervisionOverride, artifactPaths: [`${t.id}.txt`], checks: [] })),
  } };
  await page.evaluate(value => window.renderApproval([value]), workflow);
  await page.getByRole('button', { name: '审阅执行方案', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '审阅项目执行方案', exact: true });
  for (const t of taskCases) {
    const section = dialog.getByRole('heading', { name: t.goal, exact: true }).locator('..');
    assert.match(await section.innerText(), t.expected);
    await section.screenshot({ path: join(dir, `task-policy-${t.id}.png`) });
  }
  await dialog.screenshot({ path: join(dir, 'task-approval-policies.png') });
  passed('approval shows all six final task policies including OMP record-only and delivery alongside executor and profile');
  const approvalCount = await page.evaluate(() => window.commands.length);
  await page.getByRole('button', { name: '确认执行授权', exact: true }).click();
  await page.waitForFunction(expected => window.commands.length === expected, approvalCount + 1);
  assert.deepEqual(await lastCommand(), { type: 'approveRevision', projectId: 'project', digest: 'b'.repeat(64) });
  passed('approval recording sink binds the reviewed revision digest');

  const noGlobal = structuredClone(workflow);
  delete noGlobal.revision.policy.supervision;
  await page.evaluate(value => window.renderApproval([value]), noGlobal);
  await page.getByRole('button', { name: '审阅执行方案', exact: true }).click();
  const noGlobalDialog = page.getByRole('dialog', { name: '审阅项目执行方案', exact: true });
  assert.match(await noGlobalDialog.getByRole('heading', { name: taskCases[0].goal, exact: true }).locator('..').innerText(), /沿用项目默认.*未启用异常监督/);
  assert.match(await noGlobalDialog.getByRole('heading', { name: taskCases[1].goal, exact: true }).locator('..').innerText(), taskCases[1].expected);
  passed('approval explicit enabled is independent of an absent global policy');
  assert.deepEqual(errors, []); assert.equal(networkRequests, 0);
  const sourceAfter = await sourceHashes(); assert.deepEqual(sourceAfter, sourceBefore, 'Product components must not change while this fixture is built and tested');
  await writeFile(join(dir, 'commands.json'), JSON.stringify(await page.evaluate(() => window.commands), null, 2));
  await writeFile(join(dir, 'result.json'), JSON.stringify({ status: 'passed', scope: 'isolated interactive production React form and approval components; recording sink, no native IPC or durable core validation', cases, pageErrors: errors, networkRequests, modelCalls: 0, userDesktopActions: 0, sourceBefore, sourceAfter, artifact: dir }, null, 2));
  console.log(JSON.stringify({ status: 'passed', cases: cases.length, artifact: dir }));
} catch (error) {
  await page?.screenshot({ path: join(dir, 'failure.png'), fullPage: true }).catch(() => {});
  await writeFile(join(dir, 'failure.json'), JSON.stringify({ error: String(error), stack: error.stack, cases, pageErrors: errors, networkRequests, artifact: dir }, null, 2));
  throw error;
} finally { await browser?.close(); }
