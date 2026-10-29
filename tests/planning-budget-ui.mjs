// Interactive production React components with an isolated recording sink.
// No native IPC, provider request, user application or installed product action.
import { readFile, writeFile, mkdtemp } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB, '1', 'Run in the dedicated owned Job wrapper');
const dir = await mkdtemp(resolve('reports/planning-budget-ui-132-'));
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
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';import {ProjectPlanning} from '../../apps/desktop/src/ProjectPlanning';import {WorkflowPlans} from '../../apps/desktop/src/WorkflowPlans';const root=createRoot(document.getElementById('root')!);window.commands=[];let epoch=0;const send=async c=>{window.commands.push(c);return true;};window.renderPlanning=data=>root.render(<ProjectPlanning data={data} busy={false} send={send}/>);window.renderApproval=workflows=>root.render(<WorkflowPlans key={++epoch} workflows={workflows} revisions={[]} busy={false} send={send}/>);window.renderPlanning(${JSON.stringify(snapshot)});`);
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
const draft = {
  projectId:'project',workerAdapter:'openai',requirement:'主指挥规划金额授权验证',chiefModel:'fixture-chief',deputyModel:'fixture-deputy',workerModel:'fixture-worker',codexExecutable:'',
  validatorId:'tests',checkArgs:[],artifactPaths:['sum.mjs'],workerMaxRequests:3,managementMaxRequests:8,maxSeconds:180,
  acceptance:{schemaVersion:1,stages:[{id:'check',name:'独立验收',validatorId:'tests',args:[],timeoutSeconds:60}]},
  apiWorker:{target:{kind:'openAi'},model:'fixture-worker',fileTools:['read-files','write-files'],maxRounds:3,maxToolCalls:16,maxOutputTokens:512,maxTotalTokens:4096,requestTimeoutSeconds:30},
  currencyBudget:{schema:1,limitUsdNanos:5123456789,quotes:['fixture-chief','fixture-deputy','fixture-worker'].map(model=>({schema:1,target:{kind:'openAi'},requestedModel:model,reportedModels:[model],rates:{input:1000000,output:2000000,cacheRead:null,cacheWrite:0},validFrom:'2026-10-01T00:00:00Z',validUntil:'2026-11-01T00:00:00Z',source:'受控界面验证报价，不代表真实供应商价格'}))},
};
try {
  browser = await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  page = await browser.newPage({viewport:{width:1200,height:1100}});page.setDefaultTimeout(10000);
  page.on('pageerror',error=>errors.push(error.message));await page.route('**/*',route=>{networkRequests++;return route.abort();});
  await page.setContent(`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>`);await page.addScriptTag({content:script});
  const saved = savedFixture(draft,'a'.repeat(64));
  await page.evaluate(value=>window.renderPlanning(value),saved);
  await page.waitForFunction(()=>document.querySelector('#planning-chief')?.value==='fixture-chief');
  const review=page.getByRole('button',{name:'审阅规划预算',exact:true});const request=page.getByRole('button',{name:'请求主指挥生成方案',exact:true});
  assert.equal(await review.isEnabled(),true);assert.equal(await request.isDisabled(),true);assert.equal(await page.evaluate(()=>window.commands.length),0);
  passed('saved monetary configuration permits explicit local review without credentials, not model requests');
  await review.click();let dialog=page.getByRole('dialog',{name:'确认主指挥规划预算',exact:true});
  const text=await dialog.innerText();for(const expected of ['fixture-chief','$5.123456789','不批准执行方案或派发编程任务','确认后不会发送模型请求','受控界面验证报价'])assert.ok(text.includes(expected),expected);
  assert.equal(await page.evaluate(()=>window.commands.length),0);
  await dialog.getByRole('button',{name:'返回配置',exact:true}).click();assert.equal(await page.evaluate(()=>window.commands.length),0);
  passed('review and cancellation expose scope and full pricing without issuing any command');
  await review.click();await page.getByRole('dialog',{name:'确认主指挥规划预算'}).screenshot({path:join(dir,'planning-budget-review.png')});
  await page.getByRole('button',{name:'确认规划预算',exact:true}).click();
  await page.waitForFunction(()=>window.commands.length===1);
  assert.deepEqual(await lastCommand(),{type:'authorizePlanningBudget',projectId:'project',setupDigest:'a'.repeat(64)});
  assert.equal(await request.isDisabled(),true);passed('confirmation submits only configuration-bound planning authorization, no plan request or execution approval');
  const authorized=structuredClone(saved);authorized.currencyBudgets=[{projectId:'project',planningSetupDigest:'a'.repeat(64),revisionDigest:'f'.repeat(64),ledgerHead:'c'.repeat(64),limitUsdNanos:5123456789,chargedUsdNanos:0,reservedUsdNanos:0,availableUsdNanos:5123456789,approvalLinked:true,healthy:true,blocked:false,unknownRequests:0,unpricedRequests:0}];
  await page.evaluate(value=>window.renderPlanning(value),authorized);
  await page.getByText('当前配置的规划预算许可已记录。',{exact:true}).waitFor();assert.equal(await page.evaluate(()=>window.commands.length),1);passed('permission snapshot is shown without automatically requesting a model');
  await review.click();const changed=structuredClone(draft);changed.currencyBudget.limitUsdNanos=6123456789;changed.requirement+=' changed';
  await page.evaluate(value=>window.renderPlanning(value),savedFixture(changed,'b'.repeat(64)));
  dialog=page.getByRole('dialog',{name:'确认主指挥规划预算',exact:true});
  await dialog.getByRole('alert').waitFor();assert.equal(await dialog.getByRole('button',{name:'确认规划预算',exact:true}).isDisabled(),true);assert.match(await dialog.innerText(),/\$5\.123456789/);
  assert.equal(await page.evaluate(()=>window.commands.length),1);
  await dialog.getByRole('button',{name:'返回配置',exact:true}).click();await review.click();dialog=page.getByRole('dialog',{name:'确认主指挥规划预算',exact:true});
  assert.match(await dialog.innerText(),/\$6\.123456789/);assert.equal(await dialog.getByRole('button',{name:'确认规划预算',exact:true}).isEnabled(),true);
  await dialog.getByRole('button',{name:'返回配置',exact:true}).click();
  await page.locator('#planning-requirement').fill('尚未保存的新配置');assert.equal(await review.isDisabled(),true);passed('configuration change invalidates an open review and unsaved edits cannot authorize stale scope');
  assert.deepEqual(errors,[]);assert.equal(networkRequests,0);const sourceAfter=await sourceHashes();assert.deepEqual(sourceAfter,sourceBefore);
  await writeFile(join(dir,'commands.json'),JSON.stringify(await page.evaluate(()=>window.commands),null,2));
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,sourceBefore,sourceAfter,pageErrors:errors,networkRequests,modelCalls:0,scope:'production planning React with recording sink; no native IPC or actual provider calls',artifact:dir},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
} catch(error) {
  await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,errors,networkRequests},null,2));throw error;
} finally {await browser?.close();}
