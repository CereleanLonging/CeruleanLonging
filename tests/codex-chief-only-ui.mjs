import {confirmConnectionContinue} from './connection-ui-helpers.mjs';
import assert from 'node:assert/strict';
import {mkdtemp,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';
import {openConnectionMethod} from './connection-ui-helpers.mjs';

assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const run=await mkdtemp(resolve('reports/codex-chief-only-ui-'));
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'codex-chief-only-ui');
const data={dataDirectory:'synthetic-chief-only',workflows:[{project:{id:'one',root:'D:/fixture/one'},state:{tasks:{}},approval:null}],projectSetups:[],workerAccountSetups:[],validators:[{id:'cerulean-objective-checker',name:'Fixture'}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:['chief-only-model'],executable:'D:/fixture/chief-codex.exe'},providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[]};
const legacy={projectId:'one',workerAdapter:'codex',codexExecutable:'D:/fixture/chief-codex.exe',workerModel:'',chiefModel:'external-codex-session',deputyModel:'deputy',managementSources:{chief:{kind:'codexSession',sessionId:'chief-session'},deputy:{kind:'openAi'}},validatorId:'checks',checkArgs:[],artifactPaths:['result.txt'],workerMaxRequests:3,managementMaxRequests:10,maxSeconds:180};
const entry=join(run,'entry.tsx');
await writeFile(entry,`import React from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';
import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';import{IntegrationHub}from'../../apps/desktop/src/IntegrationHub';import{saveAccessDraft,accessDraftKey}from'../../apps/desktop/src/accessDraft';import{newApiWorker}from'../../apps/desktop/src/ApiWorkerFields';import{preferenceKey,writePreference}from'../../apps/desktop/src/accessPreferences';
const root=createRoot(document.getElementById('root'));const initial=${JSON.stringify(data)};window.calls=[];window.native=[];window.generation=0;window.api=newApiWorker;
window.__TAURI_INTERNALS__={invoke:async(name,args)=>{window.native.push({name,args});if(name==='objective_checker_status')return{ready:true,version:'fixture',path:'D:/synthetic/check.exe',sha256:'a'.repeat(64),error:null};throw Error('Unexpected native call '+name);}};
window.render=()=>flushSync(()=>root.render(<ProjectPlanning key={window.generation} data={window.data} mode={window.mode} busy={false} send={async c=>{window.calls.push(c);return true;}} onContinue={()=>{window.mode='planning';window.render();}} onConnections={()=>{window.mode='connections';window.render();}}/>));
window.reset=(draft,preference)=>{window.generation++;window.calls=[];window.native=[];window.mode='connections';window.data={...initial,dataDirectory:'synthetic-chief-only-'+window.generation};if(preference)writePreference(preferenceKey(window.data.dataDirectory,'model-default:worker'),JSON.stringify(preference));if(draft)saveAccessDraft(window.data.dataDirectory,'D:/fixture/one',null,draft,true);window.render();};
window.stored=()=>JSON.parse(localStorage.getItem(accessDraftKey(window.data.dataDirectory,'one')));window.hub=()=>flushSync(()=>root.render(<IntegrationHub data={window.data} busy={false} send={async c=>{window.calls.push(c);return true;}} configure={()=>{}}/>));window.reset();`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'ChiefOnly',formats:['iife'],fileName:()=> 'ui.js'}}});
const browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
const page=await browser.newPage({viewport:{width:1280,height:1000}});
const cases=[],errors=[];let externalRequests=0;
try{
  page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',r=>r.request().url()==='https://cerulean.test/'?r.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>'}):(externalRequests++,r.abort()));
  await page.goto('https://cerulean.test/');await page.addStyleTag({path:'apps/desktop/src/style.css'});await page.addScriptTag({path:join(bundle,'ui.js')});
  const worker=page.getByRole('region',{name:'执行端',exact:true});
  const next=page.getByRole('button',{name:'下一步：设置需求与计划',exact:true});
  await page.locator('#planning-adapter').waitFor({state:'attached'});
  assert.equal(await page.locator('#planning-adapter').inputValue(),'deepseek');
  assert.equal(await worker.locator('details[data-method="cli"]').count(),1);
  assert.equal(await worker.locator('option[value="codex"]:not(:disabled)').count(),0);
  assert.equal(await worker.getByRole('button',{name:'扫描本地 Codex',exact:true}).count(),0);
  assert.equal(await worker.getByLabel('Codex 程序完整路径',{exact:true}).count(),0);
  cases.push('Fresh projects use API by default and expose optional CLI without requiring its path');

  await openConnectionMethod(page,'主指挥','desktop');await page.locator('#planning-chief-session').fill('chief-session');
  await openConnectionMethod(page,'副指挥','api');await page.locator('#planning-deputy').fill('deputy');
  await openConnectionMethod(page,'执行端','key');await page.locator('#planning-worker').fill('worker-api');
  await next.click();await confirmConnectionContinue(page);await page.locator('#planning-requirement').waitFor();
  await page.locator('#planning-requirement').fill('Synthetic requirement');
  await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  const command=await page.evaluate(()=>window.calls.find(c=>c.type==='configureProject'));
  assert(command);assert.equal(command.draft.workerAdapter,'deepseek');assert.equal(command.draft.codexExecutable,'');
  assert.equal(command.draft.managementSources.chief.sessionId,'chief-session');assert((await page.evaluate(()=>window.native)).every(call=>call.name==='objective_checker_status'));
  cases.push('Chief desktop session plus API worker reaches planning and saves without a second Codex process');

  await page.evaluate(draft=>window.reset(draft),legacy);await openConnectionMethod(page,'执行端','key');
  assert.equal(await page.locator('#planning-adapter').inputValue(),'deepseek');assert.equal(await page.locator('#planning-worker').inputValue(),'');
  assert.equal(await page.locator('#planning-chief-session').inputValue(),'chief-session');
  assert(await next.isEnabled());await next.click();assert.equal(await page.evaluate(()=>document.activeElement.id),'planning-worker');
  await page.locator('#planning-worker').fill('replacement-api');await next.click();await confirmConnectionContinue(page);await page.locator('#planning-requirement').waitFor();
  assert.equal((await page.evaluate(()=>window.stored())).selection.codexExecutable,'');
  cases.push('An old empty Codex default migrates to Key configuration; validation requests the chosen worker model, never a Codex path');

  const preference={target:{kind:'provider',endpoint:{kind:'a6ApiVip'}},model:'preferred-worker'};
  await page.evaluate(({draft,preference})=>window.reset(draft,preference),{draft:legacy,preference});
  await page.waitForFunction(()=>document.getElementById('planning-adapter')?.value==='a6api');
  assert.equal(await page.locator('#planning-worker').inputValue(),'preferred-worker');
  await openConnectionMethod(page,'执行端','key');await next.click();await confirmConnectionContinue(page);await page.locator('#planning-requirement').waitFor();
  assert.deepEqual((await page.evaluate(()=>window.stored())).selection.apiWorker.target,preference.target);
  cases.push('Migration honors an existing explicit API worker preference and its exact relay endpoint');

  await page.evaluate(draft=>window.reset({...draft,workerModel:'historical-codex'}),legacy);
  await openConnectionMethod(page,'执行端','cli');
  assert.equal(await page.locator('#planning-executable').inputValue(),'D:/fixture/chief-codex.exe');
  assert(await next.isEnabled());assert.equal(await worker.getByLabel('Codex 程序完整路径',{exact:true}).count(),0);
  assert.equal((await page.evaluate(()=>window.stored())).selection.workerModel,'historical-codex');
  await openConnectionMethod(page,'执行端','key');await worker.getByRole('button',{name:'将Key 管理设为默认执行端',exact:true}).click();
  await page.locator('#planning-worker').fill('replacement-api');await next.click();await confirmConnectionContinue(page);await page.locator('#planning-requirement').waitFor();
  assert.equal((await page.evaluate(()=>window.stored())).selection.managementSources.chief.sessionId,'chief-session');
  cases.push('Configured legacy default remains unchanged until explicit replacement; chief session is retained');

  await page.evaluate(draft=>window.reset({...draft,workerAdapter:'deepseek',codexExecutable:'',workerModel:'worker-api',apiWorker:{...window.api('deepseek'),model:'worker-api'},profiles:[{id:'old-extra',description:'Retain this task',workerAdapter:'codex',codexExecutable:'D:/fixture/old-extra.exe',workerModel:'old-extra-model',artifactPaths:['extra.txt'],acceptance:{schemaVersion:1,stages:[{id:'check',name:'Check',validatorId:'checks',args:[],timeoutSeconds:30}]}}]}),legacy);
  await openConnectionMethod(page,'执行端','cli');
  assert(await next.isEnabled());assert.equal(await page.locator('#profile-0-exe').inputValue(),'D:/fixture/old-extra.exe');
  assert.equal(await worker.getByRole('button',{name:'添加CLI 接入执行配置',exact:true}).count(),1);
  await page.locator('#profile-0-adapter').selectOption('deepseek');await openConnectionMethod(page,'执行端','key');
  await page.locator('#profile-0-model').fill('extra-api');await page.locator('#profile-0-id').click();
  await next.click();await confirmConnectionContinue(page);await page.locator('#planning-requirement').waitFor();
  const profile=(await page.evaluate(()=>window.stored())).selection.profiles[0];
  assert.equal(profile.id,'old-extra');assert.equal(profile.description,'Retain this task');assert.deepEqual(profile.artifactPaths,['extra.txt']);assert.equal(profile.workerAdapter,'deepseek');assert.equal(profile.codexExecutable,'');
  cases.push('Additional legacy Codex profiles can be replaced while preserving task identity and deliverables');

  await page.evaluate(()=>{window.generation++;window.mode='connections';window.render();});await openConnectionMethod(page,'执行端','key');
  assert.equal(await page.locator('#planning-adapter').inputValue(),'deepseek');assert.equal(await page.locator('#profile-0-adapter').inputValue(),'deepseek');
  assert.equal(await worker.locator('details[data-method="cli"]').count(),1);
  await page.screenshot({path:join(run,'worker-without-codex-cli.png'),fullPage:true});
  cases.push('Remount restores selected API workers and keeps CLI available as an explicit choice');

  await page.evaluate(()=>window.hub());assert.equal(await worker.locator('summary').filter({hasText:/^CLI 接入$/}).count(),1);
  await openConnectionMethod(page,'主指挥','desktop');assert(await page.locator('#integration-chief-test-session').isVisible());
  assert((await page.evaluate(()=>window.native)).every(call=>call.name==='objective_checker_status'));assert.deepEqual(await page.evaluate(()=>window.calls),[]);
  assert.equal(externalRequests,0);assert.deepEqual(errors,[]);
  cases.push('Unbound access hub exposes optional CLI and preserves chief session binding without implicit calls');
  await writeFile(join(run,'result.json'),JSON.stringify({passed:true,cases,errors,externalRequests},null,2));console.log(JSON.stringify({passed:true,cases:cases.length,run}));
}catch(error){await page.screenshot({path:join(run,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),cases,errors},null,2));throw error;}
finally{await browser.close();}
