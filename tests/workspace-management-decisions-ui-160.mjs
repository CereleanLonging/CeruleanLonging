import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';

if (process.env.CERULEAN_NATIVE_OWNED_JOB !== '1') throw Error('Use scripts/run-workspace-management-decisions-ui-160.ps1 owned runner');
const dir = await mkdtemp('reports/workspace-management-decisions-ui-160-');
const cases = [], errors = [];
let browser, page, networkRequests = 0;
const files = [
  'apps/desktop/src/WorkspaceBrowser.tsx',
  'apps/desktop/src/WorkspaceManagementDecisions.tsx',
  'apps/desktop/src/ManagementReview.css',
  'apps/desktop/src/contracts.ts',
  'tests/workspace-management-decisions-ui-160.mjs',
  'scripts/run-workspace-management-decisions-ui-160.ps1',
];
const hashes = async () => Object.fromEntries(await Promise.all(files.map(async path => [path, createHash('sha256').update(await readFile(path)).digest('hex')])));
const before = await hashes(), entry = join(dir, 'entry.tsx');
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {WorkspaceRecordsHarness as WorkspaceBrowser} from '../../tests/fixtures/WorkspaceRecordsHarness';
const root=createRoot(document.getElementById('root')!);window.epoch=0;
window.render=()=>root.render(<React.StrictMode><div data-epoch={window.epoch}><WorkspaceBrowser key={window.epoch} enabled={true} busy={!!window.busy} showPlans={false} send={async c=>{window.sends.push(c);throw Error('unexpected mutation')}} onOverview={()=>{}} read={async q=>{
window.reads.push(q);
if(q.type==='overview')return {type:q.type,value:{product:'local',protocolVersion:1,dataDirectory:'original',mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:2,message:null}};
if(q.type==='plans')return {type:q.type,value:{revision:'plans-v1',offset:0,total:0,nextOffset:null,entries:[]}};
if(q.type==='projects')return {type:q.type,value:{revision:'projects-v1',offset:0,total:2,nextOffset:null,entries:['p','other'].map(id=>({project:{id,root:id,timezone:'UTC'},revisionDigest:'plan',sequence:1,approved:false,taskCount:1,resourceHoldingTasks:1,unknownTasks:1}))}};
if(!['managementDecisions','managementDecision','managementGatewayEvidence'].includes(q.type))throw Error('unexpected read '+q.type);
window.active++;window.maxActive=Math.max(window.maxActive,window.active);
try{
if(q.type==='managementGatewayEvidence'){
if(window.gatewayFail)throw Error('DO_NOT_DISPLAY_RAW_GATEWAY_ERROR: unsupported query or local read failed');
const value={projectId:q.projectId,operationId:q.operationId,revision:q.revision,evidenceDigest:window.detail.evidenceDigest,observation:window.gateway,...window.gatewayPatch};
const result={type:window.gatewayType,value};
if(window.gatewayHold){window.gatewayHold=false;return await new Promise((resolve,reject)=>{window.gatewayFinish=()=>resolve(result);window.gatewayReject=()=>reject(Error('DO_NOT_DISPLAY_RAW_GATEWAY_ERROR: late failure'))})}return result;
}
if(window.fail)throw Error('original management decision unavailable');
const projectId=q.projectId, rows=window.rows.map(s=>({...s,projectId}));
let value=q.type==='managementDecisions'?{projectId,revision:window.revision,offset:q.offset,total:rows.length,nextOffset:q.offset+2<rows.length?q.offset+2:null,entries:rows.slice(q.offset,q.offset+2)}:{projectId,revision:window.revision,record:{...window.detail,projectId,operationId:q.operationId}};
if(window.patch)value={...value,...window.patch};
const result={type:q.type,value};if(window.hold){window.hold=false;return await new Promise(resolve=>{window.finish=()=>resolve(result)})}return result;
}finally{window.active--}
}}/></div></React.StrictMode>);
window.set=s=>{window.epoch++;Object.assign(window,{reads:[],sends:[],active:0,maxActive:0,fail:false,patch:null,hold:false,finish:null,busy:false,gateway:null,gatewayFail:false,gatewayPatch:null,gatewayType:'managementGatewayEvidence',gatewayHold:false,gatewayFinish:null,gatewayReject:null},s);window.render()};`);
await build({ configFile: false, logLevel: 'error', define: { 'process.env.NODE_ENV': JSON.stringify('production') }, oxc: { jsx: { runtime: 'automatic' } }, build: { outDir: join(dir, 'bundle'), emptyOutDir: false, lib: { entry, name: 'ManagementDecisionUI', formats: ['iife'], fileName: () => 'ui.js' } } });
const rows = Array.from({ length: 3 }, (_, index) => ({ operationId: 'operation-' + index, projectId: 'p', purpose: index ? 'advise' : 'plan', state: index === 0 ? 'unknown' : 'ready', message: index === 0 ? 'ORIGINAL DETAIL MESSAGE' : 'ORIGINAL SUMMARY ' + index, evidenceDigest: 'evidence-' + index, canDiscard: index === 0, canReply: index !== 0, hasWait: index === 0, hasReply: index === 0 || index === 1, hasDiscard: index === 0 || index === 2 }));
const detail = { operationId: rows[0].operationId, projectId: 'p', purpose: 'plan', state: 'unknown', message: 'ORIGINAL DETAIL MESSAGE', evidenceDigest: 'evidence-0', waitReason: 'ORIGINAL WAIT REASON', userReply: 'ORIGINAL USER REPLY', discardReason: 'ORIGINAL DISCARD REASON', canDiscard: true, canReply: false,
  supervisionAdvice: { incidentId: 'incident-original', taskId: 'task-original', attemptId: 'attempt-original', threadId: 'thread-original', turnId: 'turn-original', observationDigest: 'observation-original', suggestion: 'ORIGINAL SUPERVISION SUGGESTION', rationale: 'ORIGINAL SUPERVISION RATIONALE', current: false, delivered: false, deliveryState: 'unknown' },
  applicationRecovery: { evidenceDigest: 'application-original', targetKind: 'propose', taskId: 'application-task', attemptId: 'application-attempt', sessionId: 'session-original', workflowSequence: 17 },
  adviceTrackingReview: { evidenceDigest: 'tracking-original', workflowSequence: 18 }, adviceTrackingClosure: { reason: 'ORIGINAL TRACKING CLOSURE', at: '2026-01-01T00:00:00Z' } };
const gatewayObservation = { provider: 'a6api', localId: 'operation-0', bodyResponseId: 'body-329:reply/1', status: 'linked', headers: { status: 200, requestId: { kind: 'accepted', value: 'gateway-329.request:1' } } };
try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  page = await browser.newPage(); page.setDefaultTimeout(10000); await page.clock.install();
  page.on('pageerror', error => errors.push(error.message)); await page.route('**/*', route => { networkRequests++; return route.abort(); });
  await page.setContent('<html lang="zh-CN"><div id="root"></div></html>'); await page.addScriptTag({ content: await readFile(join(dir, 'bundle/ui.js'), 'utf8') });
  const region = () => page.getByRole('region', { name: '项目管理决策记录', exact: true });
  const nav = () => region().getByRole('navigation', { name: '管理决策分页', exact: true });
  const detailRegion = () => region().getByRole('region', { name: '管理决策原详情', exact: true });
  const pageReady = async (total = 3) => { await region().getByText(`原项目共 ${total} 条管理决策`, { exact: true }).waitFor(); await page.waitForFunction(() => !Array.from(document.querySelectorAll('button')).find(button => button.textContent === '刷新管理决策首页')?.disabled); };
  const reset = async extra => {
    await page.evaluate(value => window.set(value), { rows, detail, revision: 'management-v1', gateway: gatewayObservation, ...extra });
    await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch') === String(window.epoch));
    await page.getByRole('button', { name: '查看项目 p 的管理决策', exact: true }).click(); await pageReady();
  };
  const inspect = async () => { await region().getByRole('button', { name: '查看管理决策 operation-0 的原详情', exact: true }).click(); await detailRegion().getByText('ORIGINAL DETAIL MESSAGE', { exact: true }).waitFor(); };
  await reset();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'managementDecisions', projectId: 'p', offset: 0, revision: null });
  assert.deepEqual(await page.evaluate(() => window.reads.filter(q => q.type.startsWith('management'))), [{ type: 'managementDecisions', projectId: 'p', offset: 0, revision: null }]);
  await inspect();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'managementDecision', projectId: 'p', operationId: 'operation-0', revision: 'management-v1' });
  for (const text of ['ORIGINAL DETAIL MESSAGE', 'ORIGINAL WAIT REASON', 'ORIGINAL USER REPLY', 'ORIGINAL DISCARD REASON', 'ORIGINAL SUPERVISION SUGGESTION', 'ORIGINAL SUPERVISION RATIONALE', 'ORIGINAL TRACKING CLOSURE']) await detailRegion().getByText(text, { exact: true }).waitFor();
  await detailRegion().getByText('查看原管理决策全部字段', { exact: true }).click();
  const rendered = JSON.parse(await detailRegion().locator('pre').innerText()); assert.deepEqual(rendered, detail);
  assert.match(await region().innerText(), /请求结果待核对/); assert.match(await region().innerText(), /不会自动重发或释放原请求/);
  assert.equal(await region().getByRole('button', { name: /答复|批准|放弃|恢复/ }).count(), 0);
  cases.push('test-only record navigation host selects project decision collection; exact bounded read query, unknown state, full original text/evidence and all detail JSON fields read-only');

  await nav().getByRole('button', { name: '下一页' }).click(); await region().getByRole('button', { name: '查看管理决策 operation-2 的原详情' }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'managementDecisions', projectId: 'p', offset: 2, revision: 'management-v1' });
  assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), true);
  await nav().getByRole('button', { name: '上一页' }).click(); await pageReady();
  cases.push('page bounds and forward/back cursors preserve selected project and pinned revision');

  await inspect(); await page.evaluate(() => window.fail = true); await nav().getByRole('button', { name: '下一页' }).click(); await region().getByRole('alert').waitFor();
  assert.match(await region().innerText(), /ORIGINAL DETAIL MESSAGE/); assert.match(await region().innerText(), /ORIGINAL WAIT REASON/); assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), true);
  const failed = await page.evaluate(() => window.reads.length); await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.reads.length), failed);
  await page.evaluate(() => window.fail = false); await region().getByRole('button', { name: '刷新管理决策首页' }).click(); await pageReady(); assert.equal(await region().getByRole('alert').count(), 0);
  cases.push('failed next page preserves prior list/detail, marks stale and blocks continuation until explicit refresh');

  await inspect(); await page.evaluate(() => { window.revision = 'management-v2'; window.rows = window.rows.map(row => ({ ...row, message: 'VERSION TWO' })); });
  await region().getByRole('button', { name: '刷新管理决策首页' }).click(); await pageReady();
  assert.equal(await region().getByRole('region', { name: '管理决策原详情' }).count(), 0); assert.match(await region().innerText(), /VERSION TWO/);
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'managementDecisions', projectId: 'p', offset: 0, revision: null });
  cases.push('new revision replaces visible page and invalidates old detail');

  await reset(); await inspect(); await page.evaluate(() => window.patch = { revision: 'foreign' });
  await nav().getByRole('button', { name: '下一页' }).click(); await region().getByRole('alert').waitFor();
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'managementDecisions', projectId: 'p', offset: 2, revision: 'management-v1' });
  assert.match(await detailRegion().innerText(), /ORIGINAL WAIT REASON/); assert.match(await region().innerText(), /ORIGINAL DETAIL MESSAGE/);
  await page.evaluate(() => window.patch = null); await region().getByRole('button', { name: '刷新管理决策首页' }).click(); await pageReady();
  cases.push('foreign revision response rejected while previous original summary and detail remain visible');

  await reset(); await page.evaluate(() => window.hold = true);
  await region().getByRole('button', { name: '查看管理决策 operation-0 的原详情', exact: true }).evaluate(button => { button.click(); button.click(); });
  await page.waitForFunction(() => typeof window.finish === 'function');
  assert.equal(await page.evaluate(() => window.reads.filter(q => q.type === 'managementDecision').length), 1);
  await page.clock.runFor(10000); assert.equal(await page.evaluate(() => window.maxActive), 1);
  await page.getByRole('button', { name: '查看项目 other 的管理决策', exact: true }).click(); await page.getByRole('region', { name: '项目管理决策记录' }).getByRole('heading', { name: '项目 other 的管理决策' }).waitFor();
  await page.evaluate(() => window.finish()); assert.equal(await page.getByRole('region', { name: '项目管理决策记录' }).getByRole('region', { name: '管理决策原详情' }).count(), 0);
  cases.push('repeated detail click is locked to one read; late response cannot cross project-keyed component switch');

  await reset(); await inspect(); await page.evaluate(() => window.hold = true);
  await region().getByRole('button', { name: '查看管理决策 operation-0 的原详情', exact: true }).click(); await page.waitForFunction(() => typeof window.finish === 'function');
  await region().getByRole('button', { name: '关闭管理决策详情' }).click(); await page.evaluate(() => window.finish());
  assert.equal(await region().getByRole('region', { name: '管理决策原详情' }).count(), 0);
  cases.push('late detail cannot reopen explicitly closed evidence');

  await reset(); await page.evaluate(() => window.set({ rows: [], detail, revision: 'empty' }));
  await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch') === String(window.epoch));
  await page.getByRole('button', { name: '查看项目 p 的管理决策', exact: true }).click(); await region().getByText('原项目尚无管理决策；可刷新核对最新记录。', { exact: true }).waitFor();
  assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), true);
  await page.evaluate(() => window.set({ rows, detail, revision: 'initial-error', fail: true }));
  await page.waitForFunction(() => document.querySelector('[data-epoch]')?.getAttribute('data-epoch') === String(window.epoch));
  await page.getByRole('button', { name: '查看项目 p 的管理决策', exact: true }).click(); await region().getByRole('alert').waitFor();
  assert.match(await region().innerText(), /不能据此判断没有请求/);
  cases.push('explicit zero is distinguishable from initial read failure; errors never render as empty');

  const apiDetail = structuredClone(detail);
  apiDetail.supervisionAdvice = { ...apiDetail.supervisionAdvice, api: { requestId: 'original-api-request', round: 3, toolHead: 'original-tool-head' }, threadId: '', turnId: '', deliveryState: 'recordOnly' };
  await reset({ detail: apiDetail }); await inspect();
  await detailRegion().getByText('API 请求 original-api-request · 第 3 轮', { exact: true }).waitFor();
  await detailRegion().getByText('仅保存供审阅，尚未自动投送；不代表执行器已经纠偏。', { exact: true }).waitFor();
  await detailRegion().getByText('ORIGINAL SUPERVISION SUGGESTION', { exact: true }).waitFor();
  await page.screenshot({ path: join(dir, 'api-advice-record-only.png'), fullPage: true });
  cases.push('API advice displays actual request and round, preserves suggestion and explicitly does not claim delivery or correction');

  const gatewayRegion = () => detailRegion().getByRole('region', { name: '管理网关留证', exact: true });
  const gatewayRead = async () => gatewayRegion().getByRole('button', { name: '查看网关留证', exact: true }).click();
  const gatewayCount = () => page.evaluate(() => window.reads.filter(q => q.type === 'managementGatewayEvidence').length);
  const gatewayState = async status => gatewayRegion().locator(`[data-gateway-state="${status}"]`).waitFor();

  await reset(); await inspect();
  assert.equal(await gatewayCount(), 0);
  await page.evaluate(() => window.gatewayHold = true);
  await gatewayRegion().getByRole('button', { name: '查看网关留证', exact: true }).evaluate(button => { button.click(); button.click(); });
  await page.waitForFunction(() => typeof window.gatewayFinish === 'function');
  assert.equal(await gatewayCount(), 1);
  await page.clock.runFor(10000); assert.equal(await gatewayCount(), 1);
  await page.evaluate(() => window.gatewayFinish()); await gatewayState('linked');
  assert.deepEqual(await page.evaluate(() => window.reads.at(-1)), { type: 'managementGatewayEvidence', projectId: 'p', operationId: 'operation-0', revision: 'management-v1' });
  const idTable = gatewayRegion().getByRole('table', { name: '网关留证标识', exact: true });
  assert.deepEqual(await idTable.getByRole('columnheader').allTextContents(), ['本地管理操作 ID', '响应正文 ID', '网关请求 ID']);
  assert.deepEqual(await idTable.getByRole('cell').allTextContents(), ['operation-0', 'body-329:reply/1', 'gateway-329.request:1']);
  assert.match(await gatewayRegion().innerText(), /不等于供应商账单核销/);
  assert.match(await gatewayRegion().innerText(), /不包含在原管理记录版本内/);
  assert.match(await detailRegion().innerText(), /ORIGINAL WAIT REASON/);
  await page.screenshot({ path: join(dir, 'gateway-evidence-linked.png'), fullPage: true });
  cases.push('329 gateway query is explicit, one read under rapid clicks with no polling; exact original identity query and three distinct ID columns retain original detail');

  const observations = [
    { observation: { ...gatewayObservation, headers: { status: 429, requestId: { kind: 'accepted', value: 'gateway-http-429' } } }, state: 'linked', text: 'HTTP 响应状态 429' },
    { observation: { ...gatewayObservation, status: 'headerOnly' }, state: 'headerOnly', text: '尚无完整结果关联' },
    ...['missing', 'noHeadersObserved', 'unavailable'].map(state => ({ observation: { ...gatewayObservation, status: state, headers: null }, state, text: '本次观察没有可显示的响应头请求 ID' })),
    { observation: { ...gatewayObservation, headers: { status: 100, requestId: { kind: 'missing' } } }, state: 'linked', text: '响应头中的请求 ID 缺失' },
    ...['invalidLength', 'invalidCharacters', 'repeatedHeader', 'credentialReflection'].map(reason => ({ observation: { ...gatewayObservation, headers: { status: 599, requestId: { kind: 'rejected', reason } } }, state: 'linked', text: '响应头请求 ID 已拒绝' })),
    { observation: { ...gatewayObservation, bodyResponseId: null, headers: { status: 200, requestId: { kind: 'accepted', value: 'a'.repeat(200) } } }, state: 'linked', text: '未保存' },
    { observation: { ...gatewayObservation, bodyResponseId: 'b'.repeat(200) }, state: 'linked', text: '响应头请求 ID 已通过本地格式检查' },
  ];
  for (const sample of observations) {
    await reset({ gateway: sample.observation }); await inspect(); await gatewayRead(); await gatewayState(sample.state);
    assert.match(await gatewayRegion().innerText(), new RegExp(sample.text));
    const cells = await gatewayRegion().getByRole('cell').allTextContents();
    assert.equal(cells[1], sample.observation.bodyResponseId ?? '未保存');
    assert.equal(cells[2], sample.observation.headers?.requestId.kind === 'accepted' ? sample.observation.headers.requestId.value : '未取得有效 ID');
    assert.equal(await gatewayRegion().getByRole('alert').count(), 0);
  }
  await reset({ gateway: null }); await inspect(); await gatewayRead();
  await gatewayRegion().getByText('该管理请求没有 A6API 网关留证路线。', { exact: true }).waitFor();
  assert.deepEqual(await gatewayRegion().getByRole('cell').allTextContents(), ['operation-0', '不适用', '不适用']);
  assert.equal(await gatewayRegion().locator('[data-gateway-state]').count(), 0);
  cases.push('329 all reader states and header observations stay distinct, including linked without accepted ID, HTTP failure, null non-A6 route and valid ID/status boundaries');

  await reset({ gateway: { ...gatewayObservation, status: 'headerOnly' } }); await inspect(); await gatewayRead(); await gatewayState('headerOnly');
  await page.evaluate(value => window.gateway = value, gatewayObservation);
  await page.clock.runFor(10000); assert.equal(await gatewayCount(), 1); await gatewayState('headerOnly');
  await gatewayRead(); await gatewayState('linked'); assert.equal(await gatewayCount(), 2);
  assert.equal(await page.evaluate(() => window.reads.at(-1).revision), 'management-v1');
  cases.push('329 explicit reread observes completion published under unchanged management revision without changing original detail or polling');

  await reset({ gatewayFail: true }); await inspect(); await gatewayRead(); await gatewayRegion().getByRole('alert').waitFor();
  assert.match(await detailRegion().innerText(), /ORIGINAL WAIT REASON/);
  assert.doesNotMatch(await region().innerText(), /DO_NOT_DISPLAY_RAW_GATEWAY_ERROR/);
  assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), false);
  await page.clock.runFor(10000); assert.equal(await gatewayCount(), 1);
  await page.evaluate(() => window.gatewayFail = false); await gatewayRead(); await gatewayState('linked');
  assert.equal(await gatewayRegion().getByRole('alert').count(), 0);
  cases.push('329 unsupported/failed optional query only shows fixed diagnostic in gateway region, never missing or stale original detail, with manual retry only');

  const invalidGatewaySamples = [
    { gatewayPatch: { projectId: 'other' } }, { gatewayPatch: { operationId: 'operation-1' } },
    { gatewayPatch: { revision: 'foreign' } }, { gatewayPatch: { evidenceDigest: 'foreign' } },
    { gatewayPatch: { observation: undefined } }, { gatewayPatch: { extra: 'unexpected' } },
    { gatewayType: 'managementDecision' },
    ...[
      { provider: 'openai' }, { localId: 'operation-1' }, { status: 'toString' }, { status: 'accepted' },
      { headers: null }, { status: 'unavailable' }, { status: 'missing' }, { status: 'noHeadersObserved' },
      { bodyResponseId: '' }, { bodyResponseId: 'b'.repeat(201) }, { bodyResponseId: 'body bad' },
      { bodyResponseId: '正文' }, { bodyResponseId: 'body\ninvalid' }, { bodyResponseId: 'body\n' }, { bodyResponseId: 'x'.repeat(17000) },
      { extra: 'not-in-contract' },
      ...[99, 600, 200.5, '200'].map(status => ({ headers: { status, requestId: { kind: 'missing' } } })),
      ...['', 'x'.repeat(201), 'id/path', 'id space', 'id\nline', 'id\n', '网关', 'sk-synthetic', 'BeArErsynthetic'].map(value => ({ headers: { status: 200, requestId: { kind: 'accepted', value } } })),
      { headers: { status: 200, requestId: { kind: 'missing', value: 'hidden' } } },
      { headers: { status: 200, requestId: { kind: 'accepted' } } },
      { headers: { status: 200, requestId: { kind: 'rejected', reason: 'toString' } } },
      { headers: { status: 200, requestId: { kind: 'rejected', reason: 'invalidLength', raw: 'hidden' } } },
    ].map(patch => ({ gateway: { ...gatewayObservation, ...patch } })),
  ];
  for (const sample of invalidGatewaySamples) {
    await reset(sample); await inspect(); await gatewayRead(); await gatewayRegion().getByRole('alert').waitFor();
    assert.equal(await gatewayRegion().getByRole('table').count(), 0);
    assert.equal(await gatewayRegion().locator('[data-gateway-state]').count(), 0);
    assert.match(await detailRegion().innerText(), /ORIGINAL WAIT REASON/);
    assert.equal(await nav().getByRole('button', { name: '下一页' }).isDisabled(), false);
    assert.equal(await gatewayCount(), 1);
  }
  cases.push(`329 rejects ${invalidGatewaySamples.length} foreign, malformed, oversized, extra-field, impossible-state and unsafe-ID responses without leaking evidence into original detail`);

  for (const invalidate of ['project', 'close', 'refresh', 'detail', 'lateError']) {
    await reset({ gatewayHold: true }); await inspect(); await gatewayRead();
    await page.waitForFunction(() => typeof window.gatewayFinish === 'function');
    if (invalidate === 'project') {
      await page.getByRole('button', { name: '查看项目 other 的管理决策', exact: true }).click();
      await region().getByRole('heading', { name: '项目 other 的管理决策', exact: true }).waitFor();
    } else if (invalidate === 'close') {
      await detailRegion().getByRole('button', { name: '关闭管理决策详情', exact: true }).click();
    } else if (invalidate === 'detail') {
      await page.evaluate(() => {
        const row = window.rows[1];
        window.detail = { ...window.detail, ...row, waitReason: null, userReply: 'SECOND ORIGINAL REPLY', discardReason: null };
      });
      await region().getByRole('button', { name: '查看管理决策 operation-1 的原详情', exact: true }).click();
      await detailRegion().getByText('SECOND ORIGINAL REPLY', { exact: true }).waitFor();
    } else {
      await region().getByRole('button', { name: '刷新管理决策首页', exact: true }).click(); await pageReady();
    }
    await page.evaluate(reject => reject ? window.gatewayReject() : window.gatewayFinish(), invalidate === 'lateError');
    await page.clock.runFor(1);
    assert.equal(await region().locator('[data-gateway-state]').count(), 0);
    assert.equal(await region().getByRole('table', { name: '网关留证标识', exact: true }).count(), 0);
    assert.equal(await region().getByRole('alert').count(), 0);
    assert.equal(await gatewayCount(), 1);
  }
  cases.push('329 late success/error discarded after project change, close, same-revision refresh and new detail; original navigation remains responsive during optional query');

  await reset(); await inspect(); await page.evaluate(() => { window.busy = true; window.render(); });
  await page.waitForFunction(() => Array.from(document.querySelectorAll('button')).find(button => button.textContent === '查看网关留证')?.disabled);
  assert.equal(await gatewayCount(), 0);
  await page.evaluate(() => { window.busy = false; window.render(); });
  await gatewayRead(); await gatewayState('linked'); assert.equal(await gatewayCount(), 1);
  cases.push('329 shared busy guard blocks optional read until explicitly requested after it clears; all gateway tests use offline controlled reads');


  assert.equal(await page.evaluate(() => window.sends.length), 0);
  assert.equal(await page.evaluate(() => window.reads.some(q => !['overview', 'plans', 'projects', 'managementDecisions', 'managementDecision', 'managementGatewayEvidence'].includes(q.type))), false);
  assert.deepEqual(errors, []); assert.equal(networkRequests, 0); assert.deepEqual(await hashes(), before);
  const sourceFiles = await Promise.all(files.map(async path => ({ path, sha256: createHash('sha256').update(await readFile(path)).digest('hex') })));
  const result = { status: 'passed', cases, sourceBefore: before, sourceAfter: await hashes(), sourceFiles, pageErrors: errors, networkRequests, modelCalls: 0, mutations: 0, schema: { pageQuery: { type: 'managementDecisions', projectId: 'p', offset: 0, revision: null }, detailQuery: { type: 'managementDecision', projectId: 'p', operationId: 'operation-0', revision: 'management-v1' }, pageReply: '{type,value:WorkspaceManagementDecisionPage}', detailReply: '{type,value:WorkspaceManagementDecisionDetail}', gatewayQuery: { type: 'managementGatewayEvidence', projectId: 'p', operationId: 'operation-0', revision: 'management-v1' }, gatewayReply: '{type,value:WorkspaceManagementGatewayEvidenceDetail}', fields: ['projectId','revision','offset','nextOffset','total','entries','record'] }, scope: 'Test-only record navigation host and WorkspaceManagementDecisions with controlled offline reads; not installed Tauri' };
  await writeFile(join(dir, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ status: 'passed', cases: cases.length, artifact: dir }));
} catch (error) {
  await writeFile(join(dir, 'result.json'), JSON.stringify({ status: 'failed', cases, error: String(error), pageErrors: errors, networkRequests }, null, 2));
  throw error;
} finally { await browser?.close(); }
