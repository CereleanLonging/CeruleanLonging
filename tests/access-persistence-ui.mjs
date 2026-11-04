import {openConfiguredConnections,openConnectionMethod} from "./connection-ui-helpers.mjs";
// Offline production components. Storage survives real page reloads; IPC only records.
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','Use owned runner');
const dir=await mkdtemp(resolve('reports/access-persistence-ui-787-'));
const snapshot={dataDirectory:'isolated-access-a',workflows:['one','two'].map(id=>({project:{id,root:'D:/synthetic/'+id},state:{tasks:{}},approval:null})),projectSetups:[],workerAccountSetups:[],validators:[{id:'tests',name:'Fixture checks'}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:'D:/synthetic/codex.exe'},providerConnections:[{provider:'a6api',target:{kind:'a6ApiVip'},state:'listed',keyConfigured:true,models:['original-fixture-model'],listLimited:false,checkedAt:'2026-11-04T00:00:00Z',endpoint:'https://api.a6api.vip/v1'}],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[]};
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {flushSync} from 'react-dom';import {ProjectPlanning} from '../../apps/desktop/src/ProjectPlanning';import {IntegrationHub} from '../../apps/desktop/src/IntegrationHub';import {saveAccessDraft,loadAccessDraft} from '../../apps/desktop/src/accessDraft';
const root=createRoot(document.getElementById('root'));window.calls=[];window.routes=[];window.snapshot=${JSON.stringify(snapshot)};window.storeTest=saveAccessDraft;window.readTest=loadAccessDraft;
const send=async command=>{window.calls.push(command);return true;};
window.showPlanning=(data=window.snapshot,mode='connections')=>{window.snapshot=data;root.render(<ProjectPlanning data={data} busy={false} send={send} mode={mode} onContinue={()=>window.showPlanning(window.snapshot,'planning')} onConnections={()=>window.showPlanning(window.snapshot,'connections')}/>);};
window.showHub=(data=window.snapshot)=>{window.snapshot=data;flushSync(()=>root.render(<IntegrationHub data={data} busy={false} send={send} configure={(role,method)=>window.routes.push({role,method})}/>));};
window.showPlanning();`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'AccessPersistence',formats:['iife'],fileName:()=> 'ui.js'}}});
let browser,page,externalRequests=0;const cases=[],errors=[];
try{
  async function open(){
  browser=await chromium.launchPersistentContext(join(dir,'browser-profile'),{executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true,viewport:{width:1200,height:900}});
  page=await browser.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',r=>r.request().url()==='http://localhost/access-persistence-fixture'?r.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><main style="margin:0;padding:24px"><div id="root"></div></main></html>'}):(externalRequests++,r.abort()));
  }
  await open();
  async function load(){await page.goto('http://localhost/access-persistence-fixture');await page.addStyleTag({path:'apps/desktop/src/style.css'});await page.addScriptTag({path:join(dir,'bundle/ui.js')});await page.locator('#planning-project').waitFor();}
  const check=()=>page.getByRole('button',{name:'检查并保存接入配置',exact:true});
  const next=()=>page.getByRole('button',{name:'下一步：设置需求与计划',exact:true});
  const draftKey=(id='one',scope='isolated-access-a')=>JSON.stringify(['cerulean-access-draft',1,scope,id]);
  const stored=key=>page.evaluate(key=>JSON.parse(localStorage.getItem(key)),key);
  await load();
  await openConfiguredConnections(page);
  await openConnectionMethod(page,'主指挥','desktop');await page.locator('#planning-chief-session').fill('session_fixture_1');await page.locator('#planning-deputy').fill('deputy-fixture');await page.locator('#planning-worker').fill('worker-fixture');
  await page.waitForFunction(key=>JSON.parse(localStorage.getItem(key))?.selection.workerModel==='worker-fixture',draftKey());
  assert.equal((await stored(draftKey())).fieldsChecked,false);await check().click();await page.getByRole('status').filter({hasText:'格式检查通过，接入配置已自动保存'}).waitFor();
  assert.equal((await stored(draftKey())).fieldsChecked,true);assert.equal(await page.evaluate(()=>window.calls.length),0);
  await browser.close();await open();await load();assert.equal(await page.locator('#planning-chief-source').inputValue(),'codexSession');assert.equal(await page.locator('#planning-chief-session').inputValue(),'session_fixture_1');assert.equal(await page.locator('#planning-worker').inputValue(),'worker-fixture');await page.getByText('已恢复自动保存的接入配置，格式检查已通过；可直接修改。',{exact:true}).waitFor();
  await page.screenshot({path:join(dir,'restored-project-access.png'),fullPage:true});
  await page.locator('.planning-actions').screenshot({path:join(dir,'access-save-actions.png')});
  cases.push('Checked Codex session, role models and paths survive browser process restart without commands');

  await openConfiguredConnections(page);
  await page.locator('#planning-chief-session').fill('custom_session_2');await page.waitForFunction(key=>JSON.parse(localStorage.getItem(key))?.fieldsChecked===false,draftKey());
  await load();assert.equal(await page.locator('#planning-chief-session').inputValue(),'custom_session_2');await page.getByText('已恢复自动保存的接入草稿，请检查后继续。',{exact:true}).waitFor();
  await openConfiguredConnections(page);
  await next().click();await page.locator('#planning-requirement').fill('saved access with a new requirement');await page.locator('#planning-artifacts').fill('result.txt');await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();await page.waitForFunction(()=>window.calls.length===1);
  const submitted=await page.evaluate(()=>window.calls[0]);assert.equal(submitted.type,'configureProject');assert.equal(submitted.draft.managementSources.chief.sessionId,'custom_session_2');assert.equal(submitted.draft.requirement,'saved access with a new requirement');
  cases.push('Custom edits auto-save as unchecked; Next checks them and final save submits one full configuration');

  await page.getByRole('button',{name:'修改接入配置',exact:true}).click();await page.locator('#planning-project').selectOption('two');await page.waitForFunction(()=>document.querySelector('#planning-chief').value==='');
  await openConfiguredConnections(page);
  await page.locator('#planning-chief').fill('chief-two');await page.locator('#planning-deputy').fill('deputy-two');await page.locator('#planning-worker').fill('worker-two');await check().click();
  await load();assert.equal(await page.locator('#planning-project').inputValue(),'two');assert.equal(await page.locator('#planning-chief').inputValue(),'chief-two');
  await page.locator('#planning-project').selectOption('one');await page.locator('#planning-chief-session').waitFor({state:'attached'});assert.equal(await page.locator('#planning-chief-session').inputValue(),'custom_session_2');
  await page.evaluate(data=>window.showPlanning({...data,dataDirectory:'isolated-access-b'}),snapshot);await page.waitForFunction(()=>document.querySelector('#planning-chief').value==='');assert.equal(await page.locator('#planning-chief-session').count(),0);assert.equal((await stored(draftKey())).selection.managementSources.chief.sessionId,'custom_session_2');
  cases.push('Last selected project is remembered; project/workbench drafts never overwrite each other');

  const base={...submitted.draft,managementSources:{chief:{kind:'openAi'},deputy:{kind:'openAi'}},chiefModel:'backend-chief',version:1,worker:{executable:{path:submitted.draft.codexExecutable},model:submitted.draft.workerModel},check:{id:'project-check',args:[],timeoutSeconds:60},policy:{maxRequests:3,maxSeconds:180},acceptance:{schemaVersion:1,stages:submitted.draft.acceptance.stages.map(s=>({validatorId:s.validatorId,check:s}))}};
  const configured={...snapshot,dataDirectory:'isolated-version',projectSetups:[{digest:'revision-1',setup:base}]};
  await openConfiguredConnections(page);
  await page.evaluate(data=>window.showPlanning(data),configured);await page.locator('#planning-chief').fill('local-custom-chief');await check().click();const original=await stored(draftKey('one','isolated-version'));
  await page.evaluate(data=>window.showPlanning(data),{...configured,projectSetups:[{digest:'revision-2',setup:{...base,version:2,chiefModel:'new-backend-chief'}}]});await page.waitForFunction(()=>document.querySelector('#planning-chief').value==='new-backend-chief');assert.deepEqual(await stored(draftKey('one','isolated-version')),original);
  cases.push('A new backend configuration wins over a stale local draft; viewing it does not overwrite the old draft');

  await page.evaluate(key=>localStorage.setItem(key,'{"schema":999,"selection":null}'),draftKey());await load();await page.getByRole('alert').filter({hasText:'本机接入草稿无法读取'}).waitFor();assert.equal(await page.evaluate(key=>localStorage.getItem(key),draftKey()),'{"schema":999,"selection":null}');
  await openConfiguredConnections(page);
  await page.locator('#planning-chief').fill('repaired-chief');await page.locator('#planning-deputy').fill('repaired-deputy');await page.locator('#planning-worker').fill('repaired-worker');
  await page.evaluate(()=>{window.originalSet=Storage.prototype.setItem;Storage.prototype.setItem=function(){throw new DOMException('fixture quota','QuotaExceededError');};});
  await openConfiguredConnections(page);
  await page.locator('#planning-worker').fill('not-yet-persisted');await check().click();await page.getByRole('alert').filter({hasText:'接入配置自动保存失败'}).waitFor();await next().click();assert.equal(await next().isVisible(),true);assert.equal(await page.locator('#planning-worker').inputValue(),'not-yet-persisted');
  await page.evaluate(()=>Storage.prototype.setItem=window.originalSet);await check().click();assert.equal((await stored(draftKey())).selection.workerModel,'not-yet-persisted');assert.equal((await stored(draftKey())).fieldsChecked,true);
  cases.push('Corrupt storage is preserved until an edit; quota failure never claims saved or advances, retry succeeds');

  await page.evaluate(()=>window.showHub());await openConnectionMethod(page,'主指挥','api');const chief=page.getByRole('region',{name:'主指挥',exact:true});
  await openConnectionMethod(page,'主指挥','api');await chief.getByLabel('API 服务',{exact:true}).selectOption('a6api');assert.equal(await chief.getByText('列表已读取；执行待验证',{exact:true}).count(),1);
  await page.waitForFunction(()=>JSON.parse(localStorage.getItem(JSON.stringify(['cerulean-access-preference',1,'isolated-access-a','endpoint:a6api'])))==='a6ApiVip');
  await load();await page.evaluate(()=>window.showHub({...window.snapshot,providerConnections:window.snapshot.providerConnections.map(s=>({...s,state:'unchecked',target:null,checkedAt:null,models:[]}))}));await openConnectionMethod(page,'主指挥','api');
  assert.equal(await chief.getByLabel('中转站地址（须与 Key 来源一致）').inputValue(),'a6ApiVip');assert.equal(await chief.getByText('列表已读取；执行待验证',{exact:true}).count(),0);
  await page.evaluate(data=>window.showHub(data),snapshot);
  await chief.getByLabel('中转站地址（须与 Key 来源一致）').selectOption('a6Api');assert.equal(await chief.getByText('接入选项已更改，需重新检查',{exact:true}).count(),1);assert.equal(await chief.getByText('original-fixture-model',{exact:true}).count(),0);
  await load();await page.evaluate(()=>window.showHub());await openConnectionMethod(page,'主指挥','api');assert.equal(await chief.getByLabel('API 服务',{exact:true}).inputValue(),'a6api');assert.equal(await chief.getByLabel('中转站地址（须与 Key 来源一致）').inputValue(),'a6Api');assert.equal(await chief.getByText('列表已读取；执行待验证',{exact:true}).count(),0);
  await chief.getByLabel('API 服务',{exact:true}).selectOption('qwen');await chief.getByLabel('北京业务空间 ID').fill('workspace-fixture');await chief.getByLabel('百炼地域（须与 Key 一致）').selectOption('qwenSingapore');await load();await page.evaluate(()=>window.showHub());await openConnectionMethod(page,'主指挥','api');assert.equal(await chief.getByLabel('API 服务',{exact:true}).inputValue(),'qwen');assert.equal(await chief.getByLabel('百炼地域（须与 Key 一致）').inputValue(),'qwenSingapore');await chief.getByLabel('百炼地域（须与 Key 一致）').selectOption('qwenBeijing');assert.equal(await chief.getByLabel('北京业务空间 ID').inputValue(),'workspace-fixture');
  cases.push('Provider, region and workspace choices persist; changed endpoint cannot display old model-list success');

  await openConnectionMethod(page,'主指挥','key');await chief.getByLabel('凭证供应商',{exact:true}).selectOption('openai');await chief.getByLabel('OpenAI API Key',{exact:true}).fill('never-persist-secret-fixture');
  assert.equal(await page.evaluate(()=>Object.values(localStorage).some(v=>v.includes('never-persist-secret-fixture'))),false);await load();await page.evaluate(()=>window.showHub());assert.equal(await chief.locator('details[open]').count(),0);await openConnectionMethod(page,'主指挥','key');assert.equal(await chief.getByLabel('OpenAI API Key',{exact:true}).inputValue(),'');
  await openConnectionMethod(page,'主指挥','desktop');await chief.getByRole('button',{name:'配置主指挥模型',exact:true}).click();assert.deepEqual(await page.evaluate(()=>window.routes),[{role:'chief',method:undefined}]);
  const worker=page.getByRole('region',{name:'执行端',exact:true});await worker.locator('summary').filter({hasText:/^CLI 接入$/}).click();await worker.getByLabel('Codex 程序完整路径').fill('D:/synthetic/custom-codex.exe');await load();await page.evaluate(()=>window.showHub({...window.snapshot,codexConnection:null}));await openConnectionMethod(page,'主指挥','api');await worker.locator('summary').filter({hasText:/^CLI 接入$/}).click();assert.equal(await worker.getByLabel('Codex 程序完整路径').inputValue(),'D:/synthetic/custom-codex.exe');assert.equal(await worker.getByText('连接检查通过',{exact:true}).count(),0);assert.equal(await page.evaluate(()=>window.calls.length),0);
  cases.push('Passwords never enter UI storage; custom CLI path restores without stale status or implicit source replacement');

  const structured={...submitted.draft,projectId:'one',apiWorker:{target:{kind:'provider',endpoint:{kind:'qwenBeijing',workspaceId:'roundtrip'}},model:'api-model',fileTools:['read-files'],maxRounds:2,maxToolCalls:3,maxOutputTokens:512,maxTotalTokens:4096,requestTimeoutSeconds:30},workerAdapter:'qwen',workerModel:'api-model',resourceKeys:['repo'],worktree:{baseCommit:'a'.repeat(40),poolRoot:'D:/synthetic/pool',gitExecutable:'D:/synthetic/git.exe'},profiles:[{id:'extra',description:'extra model',workerAdapter:'openai',workerModel:'extra-model',codexExecutable:'',artifactPaths:['extra.txt'],acceptance:{schemaVersion:1,stages:[{id:'check',name:'check',validatorId:'tests',args:[],timeoutSeconds:30}]},apiWorker:{target:{kind:'openAi'},model:'extra-model',fileTools:['read-files'],maxRounds:2,maxToolCalls:3,maxOutputTokens:512,maxTotalTokens:4096,requestTimeoutSeconds:30}}]};
  const result=await page.evaluate(({data,draft})=>{const ok=window.storeTest(data.dataDirectory,'D:/synthetic/one',null,draft,true);return {ok,restored:window.readTest(data,'one','D:/synthetic/one',null)};},{data:{...snapshot,dataDirectory:'structured-scope'},draft:structured});assert.equal(result.ok,true);assert.equal(result.restored.state,'restored');assert.deepEqual(result.restored.values.apiWorker,structured.apiWorker);assert.deepEqual(result.restored.values.profiles,structured.profiles.map(p=>({...p,desktopWorker:undefined})));assert.deepEqual(result.restored.values.worktree,structured.worktree);assert.deepEqual(result.restored.values.resourceKeys,['repo']);
  const secretNested={...structured,apiWorker:{...structured.apiWorker,key:'must-be-rejected'}};assert.equal(await page.evaluate(draft=>window.storeTest('rejected-scope','D:/synthetic/one',null,draft,true),secretNested),false);
  cases.push('API settings, extra models, resources and worktree round-trip; unsupported secret fields are rejected');

  const binding={id:'binding-fixture',projectId:'one',label:'fixture',bridgeSha256:'bridge',observation:{modelLabel:'desktop-model'}};
  const desktopDraft={...submitted.draft,desktopWorker:{binding,projectRoot:'D:/synthetic/one',modelLabel:'desktop-model',workspaceConfirmed:true,externalToolsAcknowledged:true},workerAdapter:'qoder'};
  const desktopResult=await page.evaluate(({data,draft})=>{const scope={...data,dataDirectory:'desktop-scope'};window.storeTest(scope.dataDirectory,'D:/synthetic/one',null,draft,true);return {live:window.readTest({...scope,desktopAccess:{bindings:[draft.desktopWorker.binding],bridgeSha256:'bridge'}},'one','D:/synthetic/one',null),stale:window.readTest(scope,'one','D:/synthetic/one',null),raw:localStorage.getItem(JSON.stringify(['cerulean-access-draft',1,scope.dataDirectory,'one']))};},{data:snapshot,draft:desktopDraft});
  assert.equal(desktopResult.live.fieldsChecked,false);assert.equal(desktopResult.live.values.desktopWorker.workspaceConfirmed,false);assert.equal(desktopResult.live.values.desktopWorker.externalToolsAcknowledged,false);assert.equal(desktopResult.stale.values.desktopWorker,undefined);assert.equal(desktopResult.raw.includes('observation'),false);
  cases.push('Desktop persistence stores selection references only; restore requires current binding and fresh confirmations');
  await page.screenshot({path:join(dir,'persisted-connections.png'),fullPage:true});
  assert.deepEqual(errors,[]);assert.equal(externalRequests,0);
  await writeFile(join(dir,'result.json'),JSON.stringify({passed:true,cases,errors,externalRequests,modelCalls:0,nativeActions:0,scope:'Production React, real localStorage/reloads, synthetic snapshots and recording IPC'},null,2));console.log(JSON.stringify({passed:true,cases:cases.length,report:dir}));
}catch(error){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,errors},null,2));throw error;}
finally{await browser?.close();}
