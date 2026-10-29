// Screenshot supplement from the already-tested production bundle. No replay of form scenarios.
import { readFile, writeFile, mkdtemp } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB, '1');
const priorDir = resolve('reports/worker-supervision-ui-0RfcND');
const prior = JSON.parse(await readFile(join(priorDir, 'result.json'), 'utf8'));
assert.equal(prior.status, 'passed');
const bundle = await readFile(join(priorDir, 'bundle/ui.js'), 'utf8');
const css = await readFile('apps/desktop/src/style.css', 'utf8');
const dir = await mkdtemp(resolve('reports/worker-supervision-approval-images-'));
const policy = { schemaVersion: 1, repeatedFailureThreshold: 3, maxAdvicePerAttempt: 1 };
const tasks = [
  { id: 'default', goal: '默认继承监督任务', adapterId: 'codex' },
  { id: 'custom', goal: '单独投送监督任务', adapterId: 'codex', profileId: 'profile-custom', supervisionOverride: { mode: 'enabled', policy: { schemaVersion: 1, repeatedFailureThreshold: 4, maxAdvicePerAttempt: 2, deliveryEnabled: true } } },
  { id: 'disabled', goal: 'API关闭监督任务', adapterId: 'deepseek', profileId: 'profile-api', supervisionOverride: { mode: 'disabled' } },
  { id: 'record', goal: '只记录不请求建议任务', adapterId: 'codex', profileId: 'profile-record', supervisionOverride: { mode: 'enabled', policy: { schemaVersion: 1, repeatedFailureThreshold: 5, maxAdvicePerAttempt: 0 } } },
];
const project = { id: 'project', root: 'D:\\fixture-project' };
const workflow = { project, state: { tasks: {} }, revisionDigest: 'b'.repeat(64), approval: null, revision: {
  project, policy: { tools: ['read-files'], maxRequests: 9, maxSeconds: 300, budgetUsdCents: null, supervision: policy },
  workflow: { version: 1, tasks: tasks.map(t => ({ id: t.id, goal: t.goal, dependencies: [] })) },
  execution: tasks.map(t => ({ taskId: t.id, adapterId: t.adapterId, profileId: t.profileId, supervisionOverride: t.supervisionOverride, artifactPaths: [`${t.id}.txt`], checks: [] })),
} };
let browser; const errors = []; let networkRequests = 0;
try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  const page = await browser.newPage({ viewport: { width: 1200, height: 980 } });
  page.on('pageerror', e => errors.push(e.message));
  await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent(`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>`);
  await page.addScriptTag({ content: bundle });
  await page.evaluate(value => window.renderApproval([value]), workflow);
  await page.getByRole('button', { name: '审阅执行方案', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '审阅项目执行方案', exact: true });
  for (const task of tasks) await dialog.getByRole('heading', { name: task.goal, exact: true }).locator('..').screenshot({ path: join(dir, `task-policy-${task.id}.png`) });
  assert.deepEqual(await page.evaluate(() => window.commands), []);
  assert.deepEqual(errors, []); assert.equal(networkRequests, 0);
  const result = { status: 'passed', scope: 'four task-policy screenshots only; supplement to existing interactive fixture, not additional product execution acceptance', priorResult: join(priorDir, 'result.json'), bundleSha256: createHash('sha256').update(bundle).digest('hex'), screenshots: tasks.map(t => `task-policy-${t.id}.png`), modelCalls: 0, networkRequests, pageErrors: errors, recordedCommands: 0, artifact: dir };
  await writeFile(join(dir, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} finally { await browser?.close(); }
