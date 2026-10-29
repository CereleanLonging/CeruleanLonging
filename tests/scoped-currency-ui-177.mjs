// Production configuration and approval components, with a recording sink only.
// Run through the dedicated Windows Job wrapper; no native IPC or provider calls.
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB, '1', 'Use run-scoped-currency-ui-177.ps1');
assert.ok(process.env.CERULEAN_SCOPED_UI_RUN_DIR, 'The owned wrapper supplies the evidence directory');
const batch = resolve('reports/scoped-ledger-177');
const run = resolve(process.env.CERULEAN_SCOPED_UI_RUN_DIR);
assert.equal(dirname(run), batch, 'Evidence must remain in batch 177');
assert.match(run.slice(batch.length + sep.length), /^ui-owned-[a-f0-9]{32}$/);
const dir = join(run, 'ui');
await mkdir(dir); // Existing evidence is never reused or overwritten.
const sourcePaths = [
  'ScopedCurrencyBudgetFields.tsx', 'CurrencyBudgetFields.tsx', 'ProjectPlanning.tsx',
  'WorkflowPlans.tsx', 'ExecutionAuthorization.tsx', 'ApiWorkerFields.tsx',
  'WorkerProfiles.tsx', 'contracts.ts', 'style.css',
].map(name => `apps/desktop/src/${name}`);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
async function sourceHashes() {
  return Object.fromEntries(await Promise.all(sourcePaths.map(async path => [path, hash(await readFile(path))])));
}
const sourceBefore = await sourceHashes();
const snapshots = join(dir, 'sources');
await mkdir(snapshots);
for (const source of sourcePaths) {
  const path = join(snapshots, source.split('/').at(-1));
  await copyFile(source, path);
  assert.equal(hash(await readFile(path)), sourceBefore[source], 'Source snapshot must match the pre-build hash');
}
await writeFile(join(dir, 'source-index.json'), JSON.stringify(sourcePaths.map(source => ({
  source, snapshot: join(snapshots, source.split('/').at(-1)), sha256: sourceBefore[source],
})), null, 2), { encoding: 'utf8', flag: 'wx' });

const snapshot = {
  workflows: [{ project: { id: 'project', root: 'D:\\owned-scoped-fixture' }, state: { tasks: {} }, approval: null }],
  projectSetups: [], validators: [{ id: 'tests', name: '隔离验收工具' }], managementDecisions: [],
  managementHistory: { projects: [] }, openaiConnection: { models: [], keyConfigured: false },
  codexConnection: { models: [], executable: 'D:\\owned-fixture-codex.exe' },
  providerConnections: [], projectRuns: [], codexJobs: [], desktopBindings: [], currencyBudgets: [],
};
const modelIds = ['fixture-chief', 'fixture-deputy', 'fixture-worker'];
const from = '2026-10-01T00:00:00+08:00', until = '2026-11-01T00:00:00+08:00';
const sources = modelIds.map(model => `隔离分档报价/${model}，非供应商价目表`);
const countSource = '隔离计数条款，明确零上限但仍需账单核对';
const expected = {
  schema: 2, quotes: [], limitUsdNanos: 12345678901,
  scopedQuotes: modelIds.map((model, i) => ({
    schema: 1, target: { kind: 'openAi' }, requestedModel: model,
    requestedService: { kind: 'explicit', tier: 'default' },
    reportedModels: i === 0 ? [model, 'fixture-chief-snapshot'] : [model],
    reportedServices: i === 0 ? ['default', 'priority'] : ['default'],
    billingBasis: 'perRequest', contextMetric: 'totalInputIncludingCache',
    bands: [{ actualModel: model, actualService: 'default', input: { from: 0, through: 272000 },
      rates: { input: 1234567, output: 2500000, cacheRead: null, cacheWrite: 0 } }],
    validFrom: from, validUntil: until, source: sources[i],
    ...(i === 0 ? { countPrice: { schema: 1, maximumUsdNanos: 0, maximumBodyBytes: 65536, source: countSource } } : {}),
  })),
};
const additionalBands = [
  { model: 'fixture-chief', service: 'default', from: '272001', through: '1000000', input: '2.345678', output: '5.25', read: '0.125', write: '4.75' },
  { model: 'fixture-chief', service: 'priority', from: '0', through: '1000000', input: '3.5', output: '6.5', read: '0.5', write: '7' },
  { model: 'fixture-chief-snapshot', service: 'default', from: '0', through: '1000000', input: '1.5', output: '3.5', read: '0', write: '4' },
  { model: 'fixture-chief-snapshot', service: 'priority', from: '0', through: '1000000', input: '5', output: '8', read: '0.75', write: '9' },
];
expected.scopedQuotes[0].bands.push(
  { actualModel: 'fixture-chief', actualService: 'default', input: { from: 272001, through: 1000000 }, rates: { input: 2345678, output: 5250000, cacheRead: 125000, cacheWrite: 4750000 } },
  { actualModel: 'fixture-chief', actualService: 'priority', input: { from: 0, through: 1000000 }, rates: { input: 3500000, output: 6500000, cacheRead: 500000, cacheWrite: 7000000 } },
  { actualModel: 'fixture-chief-snapshot', actualService: 'default', input: { from: 0, through: 1000000 }, rates: { input: 1500000, output: 3500000, cacheRead: 0, cacheWrite: 4000000 } },
  { actualModel: 'fixture-chief-snapshot', actualService: 'priority', input: { from: 0, through: 1000000 }, rates: { input: 5000000, output: 8000000, cacheRead: 750000, cacheWrite: 9000000 } },
);

function savedFixture(draft, digest) {
  return { ...snapshot, openaiConnection: { models: [], keyConfigured: true }, projectSetups: [{ digest, setup: {
    ...draft, version: 1, policy: { maxRequests: draft.workerMaxRequests, maxSeconds: draft.maxSeconds, currencyBudget: draft.currencyBudget },
    check: { id: 'project-check', args: draft.checkArgs, timeoutSeconds: 60 },
    acceptance: { schemaVersion: 1, stages: draft.acceptance.stages.map(stage => ({ validatorId: stage.validatorId,
      check: { id: stage.id, name: stage.name, args: stage.args, timeoutSeconds: stage.timeoutSeconds } })) },
  } }] };
}
function workflowFixture(budget) {
  return { project: snapshot.workflows[0].project, state: { tasks: {} }, revisionDigest: 'b'.repeat(64), approval: null, revision: {
    project: snapshot.workflows[0].project,
    policy: { tools: ['read-files'], maxRequests: 9, maxSeconds: 300, currencyBudget: budget },
    workflow: { version: 1, tasks: [{ id: 'task', goal: '受控分档预算编程方案', dependencies: [] }] },
    execution: [{ taskId: 'task', adapterId: 'openai', artifactPaths: ['sum.mjs'], checks: [] }],
  } };
}

const entry = join(dir, 'entry.tsx');
const importPath = name => JSON.stringify(relative(dir, resolve(`apps/desktop/src/${name}`)).replaceAll('\\', '/'));
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';
import {ProjectPlanning} from ${importPath('ProjectPlanning')};import {WorkflowPlans} from ${importPath('WorkflowPlans')};
const root=createRoot(document.getElementById('root')!);let epoch=0;window.commands=[];
const send=async command=>{window.commands.push(structuredClone(command));return true;};
window.renderPlanning=data=>root.render(<ProjectPlanning key={++epoch} data={data} busy={false} send={send}/>);
window.renderApproval=workflow=>root.render(<WorkflowPlans key={++epoch} workflows={[workflow]} revisions={[]} busy={false} send={send}/>);
window.renderPlanning(${JSON.stringify(snapshot)});`, { encoding: 'utf8', flag: 'wx' });

const cases = [], errors = [], screenshots = [];
let networkRequests = 0, browser, page, browserClosed = false, failure, recordedCommands = [];
const passed = name => cases.push(name);
const saveName = '保存需求与执行配置';
async function commandCount() { return page.evaluate(() => window.commands.length); }
async function lastCommand() { return page.evaluate(() => window.commands.at(-1)); }
async function saveConfiguration() {
  const before = await commandCount();
  const save = page.getByRole('button', { name: saveName, exact: true });
  assert.equal(await save.isDisabled(), false, 'Valid scoped terms enable the actual planning save');
  await save.click();
  await page.waitForFunction(count => window.commands.length === count, before + 1);
  const command = await lastCommand();
  assert.equal(command.type, 'configureProject');
  return command.draft;
}
async function blockedWithoutSubmission(name) {
  const save = page.getByRole('button', { name: saveName, exact: true });
  await page.waitForFunction(label => [...document.querySelectorAll('button')].find(button => button.textContent === label)?.disabled, saveName);
  assert.equal(await save.isDisabled(), true, name);
  const before = await commandCount();
  // This checks the form's submit handler as well as its disabled save button:
  // invalid edits must never send the previously valid value held by the parent.
  await page.locator('.project-planning form').evaluate(form => form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(await commandCount(), before, `${name}: invalid form must not send a stale draft`);
}
async function shot(name, locator) {
  const path = join(dir, name);
  if (locator) await locator.screenshot({ path });
  else await page.screenshot({ path, fullPage: false });
  screenshots.push(path);
}
async function restored(draft, digest) {
  await page.evaluate(data => window.renderPlanning(data), savedFixture(draft, digest));
  await page.waitForFunction(() => document.querySelector('#scoped-currency-limit')?.value === '12.345678901');
}
async function expectReview(budget) {
  const review = page.getByRole('region', { name: '批准的金额预算', exact: true });
  const text = await review.innerText();
  for (const term of [
    '$12.345678901', ...modelIds, 'fixture-chief-snapshot', 'default、priority',
    '272001 至 1000000（含两端）', '2.345678 / 5.25', '0.125 / 4.75',
    '未知 / 0', from, until, ...sources, '$0 / 次', '65536 字节', countSource,
    '未知，不代表免费。', '成功后仍待账单核销。',
  ]) assert.ok(text.includes(term), `Missing reviewed term: ${term}`);
  if (budget.scopedQuotes[1].requestedService.kind === 'omitted') assert.ok(text.includes('请求档位：未指定'));
  const rows = await review.locator('tbody tr').evaluateAll(items => items.map(row => [...row.querySelectorAll('td')].map(cell => cell.textContent)));
  assert.deepEqual(rows, [
    ['fixture-chief / default', '0 至 272000（含两端）', '1.234567 / 2.5', '未知 / 0'],
    ['fixture-chief / default', '272001 至 1000000（含两端）', '2.345678 / 5.25', '0.125 / 4.75'],
    ['fixture-chief / priority', '0 至 1000000（含两端）', '3.5 / 6.5', '0.5 / 7'],
    ['fixture-chief-snapshot / default', '0 至 1000000（含两端）', '1.5 / 3.5', '0 / 4'],
    ['fixture-chief-snapshot / priority', '0 至 1000000（含两端）', '5 / 8', '0.75 / 9'],
    ['fixture-deputy / default', '0 至 272000（含两端）', '1.234567 / 2.5', '未知 / 0'],
    ['fixture-worker / default', '0 至 272000（含两端）', '1.234567 / 2.5', '未知 / 0'],
  ], 'Every original price band and all four price categories must be visible');
  return review;
}

try {
  await build({ configFile: false, logLevel: 'error', define: { 'process.env.NODE_ENV': JSON.stringify('production') },
    oxc: { jsx: { runtime: 'automatic' } }, build: { outDir: join(dir, 'bundle'), emptyOutDir: false,
      lib: { entry, name: 'ScopedCurrency177', formats: ['iife'], fileName: () => 'ui.js' } } });
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  page.setDefaultTimeout(10000);
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent('<html lang="zh-CN"><meta charset="utf-8"><body><div id="root"></div></body></html>');
  await page.addStyleTag({ content: `${await readFile('apps/desktop/src/style.css', 'utf8')}body{padding:24px}` });
  await page.addScriptTag({ content: await readFile(join(dir, 'bundle/ui.js'), 'utf8') });
  await page.locator('#planning-requirement').fill('仅验证分档预算配置与审阅，不执行编程或模型调用');
  await page.locator('#planning-chief').fill(modelIds[0]);
  await page.locator('#planning-deputy').fill(modelIds[1]);
  await page.locator('#planning-adapter').selectOption('openai');
  await page.locator('#planning-worker').fill(modelIds[2]);
  await page.locator('#planning-artifacts').fill('sum.mjs');
  assert.equal(await commandCount(), 0);
  assert.equal((await saveConfiguration()).currencyBudget, undefined);
  passed('Initial mount sends nothing; an unchanged no-budget configuration still omits currencyBudget');

  await page.getByRole('button', { name: '配置服务档位与上下文报价', exact: true }).click();
  await blockedWithoutSubmission('Incomplete new schema2');
  await page.locator('#scoped-currency-limit').fill('12.345678901');
  await page.getByRole('button', { name: '补充当前模型的分档报价', exact: true }).click();
  for (let i = 0; i < 3; i++) {
    for (const [field, value] of Object.entries({ from: '0', through: '272000', input: '1.234567', output: '2.5', read: '', write: '0' })) {
      await page.locator(`#scoped-${i}-band-0-${field}`).fill(value);
    }
    for (const [field, value] of Object.entries({ from, until, source: sources[i] })) await page.locator(`#scoped-${i}-${field}`).fill(value);
  }
  await page.locator('#scoped-0-models').fill('fixture-chief\nfixture-chief-snapshot');
  await page.locator('#scoped-0-services').fill('default\npriority');
  await blockedWithoutSubmission('Missing actual model and tier combinations');
  const chiefQuote = page.getByRole('group', { name: 'OpenAI · fixture-chief', exact: true });
  for (let i = 0; i < additionalBands.length; i++) {
    await chiefQuote.getByRole('button', { name: '添加价格区间', exact: true }).click();
    for (const [field, value] of Object.entries(additionalBands[i])) await page.locator(`#scoped-0-band-${i + 1}-${field}`).fill(value);
  }
  await page.locator('#scoped-0-countFee').fill('0');
  await blockedWithoutSubmission('Zero count fee without a request scope or source');
  await page.locator('#scoped-0-countBody').fill('65536');
  await page.locator('#scoped-0-countSource').fill(countSource);
  const original = await saveConfiguration();
  assert.deepEqual(original.currencyBudget, expected);
  passed('New schema2 contains all three configured models, two actual tiers, alias models, seven price bands and exact decimal amounts');
  passed('Explicit zero count fee and cache write remain zero; absent count terms and cache read remain unknown');

  await page.locator('#scoped-0-countFee').fill('');
  await blockedWithoutSubmission('Count size and source without a fee');
  await page.locator('#scoped-0-countFee').fill('0');
  await page.locator('#scoped-0-countBody').fill('0');
  await blockedWithoutSubmission('Zero count request size');
  await page.locator('#scoped-0-countBody').fill('262145');
  await blockedWithoutSubmission('Count request size beyond contract');
  await page.locator('#scoped-0-countBody').fill('65536');
  await page.locator('#scoped-0-countSource').fill('');
  await blockedWithoutSubmission('Count terms without independent source');
  await page.locator('#scoped-0-countSource').fill(countSource);
  assert.deepEqual((await saveConfiguration()).currencyBudget, expected);
  passed('Each incomplete count term blocks both save button and form submission; repairing terms preserves the original approved data');
  for (const field of ['countFee', 'countBody', 'countSource']) await page.locator(`#scoped-0-${field}`).fill('');
  const withoutCount = await saveConfiguration();
  assert.equal(Object.hasOwn(withoutCount.currencyBudget.scopedQuotes[0], 'countPrice'), false);
  await page.locator('#scoped-0-countFee').fill('0');
  await page.locator('#scoped-0-countBody').fill('65536');
  await page.locator('#scoped-0-countSource').fill(countSource);
  assert.deepEqual((await saveConfiguration()).currencyBudget, expected);
  passed('Clearing all three count fields removes countPrice; restoring explicit zero does not confuse unknown with free');

  for (const invalid of ['9007199.254740992', '-1', '1e2', '0.0000000001', '0']) {
    await page.locator('#scoped-currency-limit').fill(invalid);
    await blockedWithoutSubmission(`Invalid project amount ${invalid}`);
  }
  await page.locator('#scoped-currency-limit').fill('12.345678901');
  for (const invalid of ['1.0000001', '-1', '1e2']) {
    await page.locator('#scoped-0-band-0-output').fill(invalid);
    await blockedWithoutSubmission(`Invalid rate ${invalid}`);
  }
  await page.locator('#scoped-0-band-0-output').fill('2.5');
  await page.locator('#scoped-0-band-1-from').fill('272000');
  await blockedWithoutSubmission('Overlapping inclusive intervals');
  await page.locator('#scoped-0-band-1-from').fill('272001');
  await page.locator('#scoped-0-services').fill('default\nauto');
  await blockedWithoutSubmission('Auto is not an observed billing tier');
  await page.locator('#scoped-0-services').fill('default\npriority');
  await page.locator('#scoped-0-until').fill('2026-09-01T00:00:00+08:00');
  await blockedWithoutSubmission('Reversed quote validity');
  await page.locator('#scoped-0-until').fill(until);
  await page.locator('#scoped-0-source').fill(' ');
  await blockedWithoutSubmission('Missing quote source');
  await page.locator('#scoped-0-source').fill(sources[0]);
  assert.deepEqual((await saveConfiguration()).currencyBudget, expected);
  passed('Overflow, exponent, negative values, excessive precision, overlap, auto tier, reversed dates and missing source cannot submit stale valid settings');

  await page.locator('#planning-worker').fill('unquoted-worker');
  await blockedWithoutSubmission('Changed worker without a quote');
  await page.locator('#planning-worker').fill(modelIds[2]);
  await page.locator('#planning-adapter').selectOption('anthropic');
  await page.locator('#planning-worker').fill(modelIds[2]);
  await blockedWithoutSubmission('Scoped OpenAI terms cannot silently apply to another provider');
  await page.locator('#planning-adapter').selectOption('openai');
  await page.locator('#planning-worker').fill(modelIds[2]);
  assert.deepEqual((await saveConfiguration()).currencyBudget, expected);
  passed('Changing model or provider invalidates pricing immediately without discarding entered terms');

  const restoredDraft = structuredClone(original);
  restoredDraft.currencyBudget.scopedQuotes[1].requestedService = { kind: 'omitted' };
  await restored(restoredDraft, 'scoped-original-digest');
  assert.equal(await page.locator('#scoped-0-band-1-input').inputValue(), '2.345678');
  assert.equal(await page.locator('#scoped-0-band-0-read').inputValue(), '');
  assert.equal(await page.locator('#scoped-0-band-0-write').inputValue(), '0');
  assert.equal(await page.locator('#scoped-0-from').inputValue(), from);
  assert.equal(await page.locator('#scoped-0-countFee').inputValue(), '0');
  assert.deepEqual((await saveConfiguration()).currencyBudget, restoredDraft.currencyBudget);
  // Force publish/collect as well: a submit alone can merely reuse the saved value.
  await page.locator('#scoped-0-band-1-input').fill('2.345679');
  await page.locator('#scoped-0-band-1-input').fill('2.345678');
  assert.deepEqual((await saveConfiguration()).currencyBudget, restoredDraft.currencyBudget);
  assert.equal(await page.getByRole('button', { name: '请求主指挥生成方案', exact: true }).isDisabled(), true);
  await page.locator('#scoped-currency-limit').scrollIntoViewIfNeeded();
  await shot('scoped-configuration-desktop.png');
  passed('Reopened schema2 survives actual collection and resave with every original field, omitted tier, time zone, aliases, bands, count source and unknown/zero intact');

  await page.getByRole('button', { name: '审阅规划预算', exact: true }).click();
  await expectReview(restoredDraft.currencyBudget);
  await shot('scoped-planning-review-desktop.png');
  const beforePlanning = await commandCount();
  await page.getByRole('button', { name: '确认规划预算', exact: true }).click();
  await page.waitForFunction(count => window.commands.length === count, beforePlanning + 1);
  assert.deepEqual(await lastCommand(), { type: 'authorizePlanningBudget', projectId: 'project', setupDigest: 'scoped-original-digest' });
  passed('Planning review shows all original terms and confirms only the exact saved configuration digest');

  // Switching off schema2 replaces this component with the legacy/disabled
  // selector. Assert that replacement instead of waiting on a removed input.
  await page.locator('#scoped-currency-enabled').click();
  await page.waitForFunction(() => !document.querySelector('#scoped-currency-enabled') && document.querySelector('#currency-enabled')?.checked === false);
  assert.equal(await page.locator('#scoped-currency-limit').count(), 0);
  assert.equal((await saveConfiguration()).currencyBudget, undefined);
  passed('Explicit budget disable removes scoped settings from the next configuration');

  const legacyDraft = structuredClone(original);
  legacyDraft.currencyBudget = { schema: 1, limitUsdNanos: 12345678901, quotes: modelIds.map((model, i) => ({
    schema: 1, target: { kind: 'openAi' }, requestedModel: model, reportedModels: [model],
    rates: { input: 1234567, output: 2500000, cacheRead: null, cacheWrite: 0 },
    validFrom: '2026-10-01T00:00:00.000Z', validUntil: '2026-11-01T00:00:00.000Z', source: sources[i],
    ...(i === 0 ? { countPrice: { schema: 1, maximumUsdNanos: 0, maximumBodyBytes: 65536, source: countSource } } : {}),
  })) };
  await page.evaluate(data => window.renderPlanning(data), savedFixture(legacyDraft, 'legacy-original-digest'));
  await page.waitForFunction(() => document.querySelector('#currency-limit')?.value === '12.345678901');
  assert.equal(await page.locator('#scoped-currency-limit').count(), 0);
  await page.locator('#currency-0-input').fill('1.234568');
  await page.locator('#currency-0-input').fill('1.234567');
  assert.deepEqual((await saveConfiguration()).currencyBudget, legacyDraft.currencyBudget);
  passed('Existing schema1 still renders its original editor and resaves unchanged without adding scoped fields');

  await page.evaluate(workflow => window.renderApproval(workflow), workflowFixture(restoredDraft.currencyBudget));
  const beforeApproval = await commandCount();
  await page.getByRole('button', { name: '审阅执行方案', exact: true }).click();
  await expectReview(restoredDraft.currencyBudget);
  assert.equal(await commandCount(), beforeApproval, 'Opening a review has no command side effect');
  await shot('scoped-execution-review-desktop.png');
  await page.setViewportSize({ width: 412, height: 915 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  const dialogBounds = await page.getByRole('dialog').boundingBox();
  assert.ok(dialogBounds && dialogBounds.x >= 0 && dialogBounds.x + dialogBounds.width <= 412, 'The actual review dialog must fit inside the viewport');
  await shot('scoped-execution-review-narrow.png');
  const review = await expectReview(restoredDraft.currencyBudget);
  await writeFile(join(dir, 'review-text.txt'), await review.innerText(), { encoding: 'utf8', flag: 'wx' });
  await page.getByRole('button', { name: '确认执行授权', exact: true }).click();
  await page.waitForFunction(count => window.commands.length === count, beforeApproval + 1);
  assert.deepEqual(await lastCommand(), { type: 'approveRevision', projectId: 'project', digest: 'b'.repeat(64) });
  passed('Execution review displays every band/tier/source/count term, fits a narrow viewport and confirms the original revision digest exactly once');

  recordedCommands = await page.evaluate(() => window.commands);
  assert.ok(recordedCommands.every(command => ['configureProject', 'authorizePlanningBudget', 'approveRevision'].includes(command.type)), 'No model or execution command may be produced');
  assert.deepEqual(errors, []);
  assert.equal(networkRequests, 0);
} catch (error) {
  failure = error;
  if (page) {
    await page.screenshot({ path: join(dir, 'failure.png'), fullPage: true }).catch(() => {});
    recordedCommands = await page.evaluate(() => window.commands ?? []).catch(() => []);
  }
} finally {
  if (browser) {
    try { await browser.close(); browserClosed = true; }
    catch (error) { failure ??= error; }
  }
}
const sourceAfter = await sourceHashes();
try { assert.deepEqual(sourceAfter, sourceBefore, 'Product sources changed during UI verification'); }
catch (error) { failure ??= error; }
await writeFile(join(dir, 'commands.json'), JSON.stringify(recordedCommands, null, 2), { encoding: 'utf8', flag: 'wx' });
const result = {
  status: failure ? 'failed' : 'passed', scope: 'Production React configuration and approval components with a recording sink; no native IPC or supplier calls',
  cases, networkRequests, modelCalls: 0, nativeIpcCalls: 0, browserClosed, pageErrors: errors,
  sourceBefore, sourceAfter, artifact: dir,
  screenshots: await Promise.all(screenshots.map(async path => ({ path, sha256: hash(await readFile(path)) }))),
  ...(failure ? { error: String(failure), stack: failure.stack } : {}),
};
await writeFile(join(dir, 'result.json'), JSON.stringify(result, null, 2), { encoding: 'utf8', flag: 'wx' });
console.log(JSON.stringify({ status: result.status, cases: cases.length, artifact: dir, browserClosed }));
if (failure) throw failure;
