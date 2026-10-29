// Production native IPC and UI over 1,000 offline management records. No key/model calls.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';

const reports = resolve('reports');
await mkdir(reports, { recursive: true });
const dir = await mkdtemp(join(reports, 'management-history-native-'));
const data = join(dir, 'data');
const runtime = resolve('target/debug/cerulean-core.exe');
const desktop = resolve('target/debug/cerulean-desktop.exe');
const frameLimit = 4 * 1024 * 1024;
const maxPage = 50;
const errors = [];
const measures = [];
const fullFrameMeasures = [];
const rejected = [];
let app, browser, page, fixture, initial, result;
let coreWasLaunched = false;
let sequence = 0;
const sha = (value) => createHash('sha256').update(value).digest('hex');
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

async function launch() {
  const listener = createServer();
  listener.listen(0, '127.0.0.1');
  await once(listener, 'listening');
  const port = listener.address().port;
  await new Promise((done) => listener.close(done));
  const env = { ...process.env, CERULEAN_TEST_DATA_DIR: data,
    CERULEAN_TEST_WEBVIEW_ROOT: join(dir, 'webview-cache'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1` };
  delete env.WEBVIEW2_USER_DATA_FOLDER;
  app = spawn(desktop, [], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env });
  coreWasLaunched = true;
  app.stdout.resume();
  app.on('error', (error) => errors.push(`native spawn: ${error.message}`));
  app.stderr.on('data', (chunk) => {
    if (/panicked|0x8007139F|failed to create webview/i.test(chunk.toString())) errors.push(chunk.toString());
  });
  for (let attempt = 0; attempt < 100; attempt++) {
    assert.equal(app.exitCode, null, 'native desktop remains running');
    assert.deepEqual(errors, []);
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1000 });
      break;
    } catch { await sleep(250); }
  }
  assert.ok(browser, 'native WebView2 CDP connection');
  for (let attempt = 0; attempt < 50; attempt++) {
    page = browser.contexts().flatMap((context) => context.pages())
      .find((candidate) => candidate.url().includes('tauri.localhost'));
    if (page) break;
    await sleep(200);
  }
  assert.ok(page, 'production Tauri page');
  page.on('pageerror', (error) => errors.push(error.message));
  await page.getByText('本地核心已连接', { exact: true }).waitFor({ timeout: 60000 });
  await page.getByRole('button', { name: '需求与计划' }).click();
  await page.getByRole('region', { name: '主副指挥请求记录' }).waitFor();
}

async function closeWindow() {
  if (!app) return;
  if (app.exitCode === null) {
    const exiting = once(app, 'exit');
    await page?.evaluate(() => window.__TAURI_INTERNALS__.invoke('plugin:window|close', { label: 'main' })).catch(() => {});
    await Promise.race([exiting,
      new Promise((_, reject) => setTimeout(() => reject(Error('normal native close timed out')), 10000).unref())]);
  }
  await browser?.close();
  browser = null; page = null; app = null;
}

function stopOwnCore() {
  return spawnSync(runtime, ['--stop', '--data-dir', data], {
    windowsHide: true, encoding: 'utf8', timeout: 30000, maxBuffer: frameLimit * 2,
  });
}

async function invoke(command) {
  const started = performance.now();
  const value = await page.evaluate((command) =>
    window.__TAURI_INTERNALS__.invoke('core_request', { command }), command);
  const bytes = Buffer.byteLength(JSON.stringify(value));
  measures.push({ command: command.type, projectId: command.projectId ?? null,
    offset: command.offset ?? null, operationId: command.operationId ?? null,
    milliseconds: Math.round((performance.now() - started) * 100) / 100, snapshotBytes: bytes });
  assert.ok(bytes < frameLimit, 'native Snapshot fits below framed response limit');
  return value;
}

async function expectRejected(command, pattern) {
  const response = await page.evaluate(async (command) => {
    try { await window.__TAURI_INTERNALS__.invoke('core_request', { command }); return { accepted: true }; }
    catch (error) { return { accepted: false, error: String(error) }; }
  }, command);
  assert.equal(response.accepted, false, `${command.type} must be rejected`);
  if (pattern) assert.match(response.error, pattern);
  rejected.push({ command: command.type, reason: response.error });
}

function measureFullServiceReply(command) {
  const id = `history-capacity-${++sequence}`;
  const answer = spawnSync(runtime, ['--request', '--data-dir', data], {
    windowsHide: true, encoding: 'utf8', timeout: 30000, maxBuffer: frameLimit * 2,
    input: JSON.stringify({ id, protocolVersion: 1, command }),
  });
  assert.equal(answer.status, 0, answer.error?.message ?? answer.stderr);
  // CLI prints the same ServiceReply serde representation written by the pipe.
  const serialized = answer.stdout.trimEnd();
  const bytes = Buffer.byteLength(serialized);
  const reply = JSON.parse(serialized);
  assert.equal(reply.response.id, id);
  assert.equal(reply.response.error, null);
  assert.ok(bytes < frameLimit, 'complete ServiceReply, not just its history array, fits 4 MiB');
  fullFrameMeasures.push({ command: command.type, operationId: command.operationId ?? null, bytes });
  return reply.response.result;
}

function assertNoExecution(snap) {
  assert.equal(snap.managementHistory.projects.reduce((sum, entry) => sum + entry.total, 0), fixture.total);
  assert.equal(snap.openaiConnection.keyConfigured, false);
  assert.equal(snap.plans.length, 0);
  assert.equal(snap.workflows.length, 2);
  for (const workflow of snap.workflows) {
    assert.equal(workflow.revision, null);
    assert.equal(workflow.approval, null);
    assert.deepEqual(workflow.state.tasks, {});
  }
  assert.equal(snap.codexJobs.length, 0);
  assert.equal(snap.verificationJobs.length, 0);
  assert.equal(snap.projectRuns.length, 0);
  if (initial) assert.deepEqual(snap.workflows, initial.workflows);
}

function assertSummaryPage(snap, projectId = null, offset = 0, total = fixture.total) {
  assertNoExecution(snap);
  assert.equal(snap.managementDetail, null, 'detail is request-local');
  assert.equal(snap.managementHistory.projectId, projectId);
  assert.equal(snap.managementHistory.offset, offset);
  assert.equal(snap.managementHistory.total, total);
  assert.equal(snap.managementDecisions.length, Math.min(maxPage, total - offset));
  assert.match(snap.managementHistory.revision, /^[a-f0-9]{64}$/);
  for (const row of snap.managementDecisions) {
    if (projectId !== null) assert.equal(row.projectId, projectId);
    assert.ok(Array.from(row.message ?? '').length <= 160, 'preview includes ellipsis within 160 characters');
    for (const omitted of ['waitReason', 'userReply', 'discardReason', 'context', 'observation']) {
      assert.equal(Object.hasOwn(row, omitted), false, `${omitted} is absent from summary`);
    }
  }
}

async function walk(projectId, total) {
  let offset = 0, revision = null;
  const ids = [];
  do {
    const snap = await invoke({ type: 'readManagementPage', projectId, offset, revision });
    assertSummaryPage(snap, projectId, offset, total);
    if (revision !== null) assert.equal(snap.managementHistory.revision, revision);
    revision = snap.managementHistory.revision;
    ids.push(...snap.managementDecisions.map((entry) => entry.operationId));
    offset = snap.managementHistory.nextOffset;
  } while (offset !== null);
  assert.equal(ids.length, total);
  assert.equal(new Set(ids).size, total, 'no duplicated or omitted history rows across pages');
  return { ids, revision };
}

async function compareFullDetail(operationId) {
  const snap = await invoke({ type: 'readManagementDetail', operationId });
  assertNoExecution(snap);
  assert.equal(snap.managementDetail.operationId, operationId);
  if (operationId !== fixture.unknownOperation) {
    assert.equal(snap.managementDetail.waitReason, String.fromCodePoint(fixture.waitCharacter).repeat(fixture.textLength));
    assert.equal(snap.managementDetail.discardReason, String.fromCodePoint(fixture.discardCharacter).repeat(fixture.textLength));
    const record = JSON.parse(await readFile(join(data, 'management-decisions', `${operationId}.4.json`), 'utf8'));
    assert.equal(snap.managementDetail.discardReason, record.body.evidence.resolution.reason);
    assert.equal(snap.managementDetail.evidenceDigest, record.hash);
  }
  return snap.managementDetail;
}

try {
  const seeded = spawnSync('cargo', ['test', '-p', 'cerulean-core', 'seed_native_management_history_fixture', '--locked', '--', '--ignored', '--nocapture'], {
    windowsHide: true, encoding: 'utf8', timeout: 180000, maxBuffer: 4 * 1024 * 1024,
    env: { ...process.env, CERULEAN_HISTORY_NATIVE_DIR: dir },
  });
  await writeFile(join(dir, 'seed.log'), `${seeded.stdout ?? ''}\n${seeded.stderr ?? ''}`);
  assert.equal(seeded.status, 0, seeded.error?.message ?? seeded.stderr);
  fixture = JSON.parse(await readFile(join(dir, 'fixture.json'), 'utf8'));
  assert.equal(fixture.total, 1000);
  assert.equal(fixture.realModelCalls, 0);
  const evidenceBefore = (await readdir(join(data, 'management-decisions'))).sort();
  assert.equal(evidenceBefore.filter((name) => name.endsWith('.0.json')).length, fixture.total);
  await launch();
  initial = await invoke({ type: 'snapshot' });
  assertSummaryPage(initial);
  assert.equal(initial.managementDecisions[0].operationId, fixture.unknownOperation);
  assert.equal(initial.managementHistory.projects.find((entry) => entry.projectId === 'project-two').pending, 1);
  assert.equal(initial.managementHistory.projects.find((entry) => entry.projectId === 'project').pending, 0);
  assert.ok(initial.managementHistory.projects.every((entry) => entry.running === 0 && entry.waiting === 0));
  await writeFile(join(dir, 'initial-snapshot.json'), JSON.stringify(initial, null, 2));

  const all = await walk(null, fixture.total);
  assert.equal(all.ids[0], fixture.unknownOperation);
  assert.equal(all.ids.at(-1), fixture.oldestOperation);
  const filtered = {};
  for (const [projectId, total] of Object.entries(fixture.projectCounts)) filtered[projectId] = await walk(projectId, total);
  assert.deepEqual(new Set(Object.values(filtered).flatMap((page) => page.ids)), new Set(all.ids));
  await writeFile(join(dir, 'page-index.json'), JSON.stringify({ all, filtered }, null, 2));
  const oldest = await compareFullDetail(fixture.oldestOperation);
  const newest = await compareFullDetail(fixture.unknownOperation);
  assert.equal(newest.state, 'unknown');
  assert.equal(newest.canDiscard, true);
  for (let attempt = 0; attempt < 3; attempt++) assertSummaryPage(await invoke({ type: 'snapshot' }));
  assertSummaryPage(measureFullServiceReply({ type: 'snapshot' }));
  const maximumDetail = measureFullServiceReply({ type: 'readManagementDetail', operationId: fixture.oldestOperation });
  assert.equal(maximumDetail.managementDetail.waitReason, oldest.waitReason);
  assert.equal(maximumDetail.managementDetail.discardReason, oldest.discardReason);
  await expectRejected({ type: 'readManagementPage', projectId: null, offset: 50, revision: null }, /续页需要/);
  await expectRejected({ type: 'readManagementPage', projectId: null, offset: fixture.total + 1, revision: all.revision }, /偏移/);
  await expectRejected({ type: 'readManagementDetail', operationId: 'missing-operation' }, /不存在/);
  await expectRejected({ type: 'discardManagementDecision', operationId: fixture.unknownOperation,
    evidenceDigest: all.revision, reason: 'history cursor is not authority to resolve an individual request' }, /stale/);
  assert.deepEqual((await readdir(join(data, 'management-decisions'))).sort(), evidenceBefore, 'all page/detail reads are read-only');

  const section = page.getByRole('region', { name: '主副指挥请求记录' });
  const pagination = section.getByRole('navigation', { name: '指挥历史分页' });
  await pagination.getByText('共 1000 条，当前 1–50 条', { exact: true }).waitFor();
  await pagination.getByRole('button', { name: '下一页', exact: true }).click();
  await pagination.getByText('共 1000 条，当前 51–100 条', { exact: true }).waitFor();
  assert.equal(await section.locator('[data-management-operation]').count(), maxPage);
  assert.equal(await section.locator('[data-management-operation]').first().getAttribute('data-management-operation'), all.ids[50]);
  await pagination.screenshot({ path: join(dir, 'native-next-page.png') });
  await section.getByLabel('筛选请求项目', { exact: true }).selectOption('project');
  await pagination.getByText('共 500 条，当前 1–50 条', { exact: true }).waitFor();
  const regularId = filtered.project.ids[0];
  const regularRow = section.locator(`[data-management-operation="${regularId}"]`);
  await regularRow.getByRole('button', { name: '查看完整记录', exact: true }).click();
  const detail = section.locator(`[data-management-detail="${regularId}"]`);
  await detail.waitFor();
  const shown = await detail.locator('p.management-text').allTextContents();
  assert.ok(shown.includes(oldest.waitReason), 'native detail displays complete saved question');
  assert.ok(shown.includes(oldest.discardReason), 'native detail displays complete saved reason');
  await detail.screenshot({ path: join(dir, 'native-full-record.png') });
  assertSummaryPage(await invoke({ type: 'snapshot' }));
  assert.equal(await detail.count(), 1, 'request-local Snapshot omits detail without clearing the viewed native panel');
  await section.getByLabel('筛选请求项目', { exact: true }).selectOption('project-two');
  await pagination.getByText('共 500 条，当前 1–50 条', { exact: true }).waitFor();
  await detail.waitFor({ state: 'detached' });
  const unknownRow = section.locator(`[data-management-operation="${fixture.unknownOperation}"]`);
  await unknownRow.getByRole('button', { name: '放弃本地请求…', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '确认放弃本地指挥请求', exact: true });
  await dialog.waitFor();
  const discardReason = '容量验收完成，明确关闭这一条本地未知请求，保留原证据。';
  await dialog.getByLabel('放弃理由', { exact: true }).fill(discardReason);
  await dialog.getByRole('button', { name: '确认放弃本地请求', exact: true }).click();
  await dialog.waitFor({ state: 'hidden' });
  await unknownRow.getByText('已放弃本地请求', { exact: true }).waitFor();
  await section.getByText('操作已保存，当前筛选已刷新至首页，完整记录已更新。调度状态保持不变。', { exact: true }).waitFor();
  const after = await invoke({ type: 'snapshot' });
  assertSummaryPage(after);
  assert.notEqual(after.managementHistory.revision, all.revision);
  assert.ok(after.managementHistory.projects.every((entry) => entry.pending === 0));
  const savedDetail = (await invoke({ type: 'readManagementDetail', operationId: fixture.unknownOperation })).managementDetail;
  assert.equal(savedDetail.discardReason, discardReason);
  assert.notEqual(savedDetail.evidenceDigest, newest.evidenceDigest);
  await expectRejected({ type: 'readManagementPage', projectId: null, offset: 50, revision: all.revision }, /记录已变化/);
  await expectRejected({ type: 'discardManagementDecision', operationId: fixture.unknownOperation,
    evidenceDigest: newest.evidenceDigest, reason: 'stale duplicate must never be saved' });
  const resolution = JSON.parse(await readFile(join(data, 'management-decisions', `${fixture.unknownOperation}.4.json`), 'utf8'));
  assert.equal(resolution.body.previous, newest.evidenceDigest);
  assert.equal(resolution.body.evidence.resolution.reason, discardReason);
  assert.equal(resolution.hash, savedDetail.evidenceDigest);
  const evidenceAfter = (await readdir(join(data, 'management-decisions'))).sort();
  assert.deepEqual(evidenceAfter.filter((name) => !evidenceBefore.includes(name)), [`${fixture.unknownOperation}.4.json`]);
  assert.equal(evidenceAfter.filter((name) => name.endsWith('.0.json')).length, fixture.total);
  await section.locator(`[data-management-detail="${fixture.unknownOperation}"]`).screenshot({ path: join(dir, 'native-disposition.png') });

  await closeWindow();
  const stopped = stopOwnCore();
  await writeFile(join(dir, 'restart-stop.log'), `${stopped.stdout ?? ''}\n${stopped.stderr ?? ''}`);
  assert.equal(stopped.status, 0, stopped.error?.message ?? stopped.stderr);
  await launch();
  const reopened = await invoke({ type: 'snapshot' });
  assertSummaryPage(reopened);
  assert.equal(reopened.managementHistory.revision, after.managementHistory.revision);
  const reopenedDetail = (await invoke({ type: 'readManagementDetail', operationId: fixture.unknownOperation })).managementDetail;
  assert.equal(reopenedDetail.evidenceDigest, savedDetail.evidenceDigest);
  assert.equal(reopenedDetail.discardReason, discardReason);
  const last = await invoke({ type: 'readManagementPage', projectId: null, offset: 950, revision: reopened.managementHistory.revision });
  assertSummaryPage(last, null, 950);
  assert.equal(last.managementDecisions.at(-1).operationId, fixture.oldestOperation);
  await compareFullDetail(fixture.oldestOperation);
  assertSummaryPage(measureFullServiceReply({ type: 'snapshot' }));
  assert.deepEqual(errors, []);
  await closeWindow();
  result = {
    at: new Date().toISOString(), status: 'passed', scope: 'production native UI and core IPC, 1000-request synthetic offline ledger, full core restart',
    requests: fixture.total, newManagementRequests: 0, realModelCalls: 0,
    checks: { boundedDefaultSnapshot: true, completePaginationWithoutDuplicates: true,
      exactPerProjectCounts: true, fullOldestAndNewestDetails: true, detailNotRetainedInOrdinarySnapshots: true,
      completeServiceReplyBelow4MiB: true, nativeNextPageAndProjectFilter: true, nativeFullRecord: true,
      explicitNativeDisposition: true, staleRevisionRejected: true, staleDispositionRejected: true,
      readsDoNotWriteLedger: true, onlyOneExplicitResolutionWritten: true, coreAndWindowRestartReadable: true,
      noKeyOrModelOrWorkflowExecution: true },
    textEvidence: { characters: fixture.textLength, oldestWaitSha256: sha(oldest.waitReason), oldestDiscardSha256: sha(oldest.discardReason) },
    fullFrameMeasures, measures, rejected, errors,
  };
} catch (error) {
  await page?.screenshot({ path: join(dir, 'failure.png'), fullPage: false }).catch(() => {});
  await writeFile(join(dir, 'failure.json'), JSON.stringify({ at: new Date().toISOString(), error: String(error), errors, measures, fullFrameMeasures, rejected }, null, 2));
  throw error;
} finally {
  await browser?.close().catch(() => {});
  if (app?.exitCode === null) app.kill();
  if (coreWasLaunched) {
    const cleanup = stopOwnCore();
    await writeFile(join(dir, 'cleanup.json'), JSON.stringify({ status: cleanup.status, signal: cleanup.signal, error: cleanup.error?.message, stderr: cleanup.stderr }, null, 2));
    if (result) assert.equal(cleanup.status, 0, cleanup.error?.message ?? cleanup.stderr);
    else if (cleanup.status !== 0) console.error('Isolated test cleanup:', cleanup.error?.message ?? cleanup.stderr);
  }
}
await writeFile(join(dir, 'result.json'), JSON.stringify(result, null, 2));
console.log(JSON.stringify({ dir, status: result.status, requests: result.requests, newManagementRequests: 0,
  realModelCalls: 0, maximumFullReplyBytes: Math.max(...fullFrameMeasures.map((entry) => entry.bytes)), checks: result.checks }, null, 2));
