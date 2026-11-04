import {openConfiguredConnections,openConnectionMethod} from "./connection-ui-helpers.mjs";
// Production UI, isolated snapshots and recording IPC. Never contacts a provider.
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','Use the owned UI runner');
const dir=await mkdtemp(resolve('reports/connection-choice-ui-789-'));
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {App} from '../../apps/desktop/src/App';import {HelpHeading} from '../../apps/desktop/src/HelpHeading';const root=createRoot(document.getElementById('root'));window.showApp=()=>root.render(<App/>);window.showHelp=()=>root.render(<form onSubmit={e=>{e.preventDefault();window.formSubmits++;}}><HelpHeading as="h1">接入管理</HelpHeading><details><summary><HelpHeading as="h3">主指挥</HelpHeading></summary><p>detail</p></details><dialog><HelpHeading as="h2">确认退出工作台？</HelpHeading><button type="button" id="other">返回</button></dialog><button type="button" id="outside">外部</button></form>);window.showApp();`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'AccessHelpUI',formats:['iife'],fileName:()=> 'ui.js'}}});
const snapshot={dataDirectory:'isolated-ui-786',events:[],workflows:[{project:{id:'project',root:'D:/synthetic/project',timezone:'UTC'},state:{tasks:{}},tasks:[],approval:null}],projectSetups:[],workerAccountSetups:[],validators:[{id:'tests',name:'Fixture checks',args:[]}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:'D:/synthetic/codex.exe'},providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],ompJobs:[],verificationJobs:[],desktopBindings:[],adapters:[]};
let browser,page,externalRequests=0;const cases=[],errors=[];
try {
  browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  page=await browser.newPage({viewport:{width:1360,height:960}});page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',route=>route.request().url()==='http://localhost/ui-access-fixture'?route.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>'}):(externalRequests++,route.abort()));
  await page.goto('http://localhost/ui-access-fixture');
  await page.evaluate(snapshot=>{
    window.snapshot=snapshot;window.calls=[];window.deferred=[];window.readMode='active';window.formSubmits=0;
    window.snapshot.validators[0].tool={executable:{path:'D:/synthetic/check.exe',sha256:'a'.repeat(64)},inputs:[],environment:{}};
    window.snapshot.validators[0].registeredAt='2026-11-04T00:00:00Z';
    window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
    window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'}},transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(name,args)=>{
      window.calls.push({name,args});
      if(name==='plugin:event|listen')return 1;
      if(name==='plugin:event|unlisten')return;
      if(name==='independent_verifier_status')return {ready:false,message:'offline fixture'};
      if(name==='maintenance_info')return {version:'local-ui-fixture',dataDirectory:'isolated-ui-786',installationDirectory:'isolated',holdPresent:false,bundleSchema:1};
      if(name==='core_request'){
        if(window.readMode==='deferred')return new Promise((resolve,reject)=>window.deferred.push({resolve,reject}));
        if(window.readMode==='error')throw Error('fixture read failure');
        return structuredClone(window.snapshot);
      }
      if(name==='core_submit'){
        if(args.command.type!=='configureProject')throw Error('Unexpected mutation '+args.command.type);
        return {type:'receipt',value:{requestId:'fixture-config'}};
      }
      if(name==='workspace_read'){
        const q=args.query;
        if(q.type==='overview')return {type:'overview',value:{product:'fixture',protocolVersion:1,dataDirectory:'isolated-ui-786',mode:'ready',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:window.snapshot.workflows.length}};
        if(q.type==='projects'||q.type==='plans')return {type:q.type,value:{revision:'fixture',offset:0,total:0,nextOffset:null,entries:[]}};
      }
      throw Error('Unexpected IPC '+name);
    }};
  },snapshot);
  await page.addStyleTag({path:'apps/desktop/src/style.css'});await page.addScriptTag({path:join(dir,'bundle/ui.js')});
  const nav = name => page.locator('.sidebar nav').getByRole('button',{name,exact:false});
  const role = name => page.getByRole('region',{name,exact:true});
  const topMethods = name => role(name).locator(':scope > details.connection-method');
  const submitted = () => page.evaluate(()=>window.calls.filter(call=>call.name==='core_submit').map(call=>call.args.command));
  await page.getByText('本地核心已连接',{exact:true}).waitFor();
  await nav('接入管理').click();
  const next = page.getByRole('button',{name:'下一步：设置需求与计划',exact:true});
  await next.waitFor();
  for (const name of ['主指挥','副指挥','执行端']) {
    assert.equal(await topMethods(name).filter({has:page.locator(':scope[open]')}).count(),0);
    assert.equal(await topMethods(name).evaluateAll(elements=>elements.some(element=>element.open)),false);
  }
  assert.equal(await page.locator('#planning-chief').isVisible(),false);
  assert.equal(await page.locator('#planning-deputy').isVisible(),false);
  assert.equal(await page.locator('#planning-worker').isVisible(),false);
  assert.equal((await submitted()).length,0);
  await page.screenshot({path:join(dir,'collapsed-default.png'),fullPage:true});
  cases.push('All methods start collapsed and no model form or command is opened implicitly');

  await openConnectionMethod(page,'主指挥','api');
  await page.locator('#planning-chief').fill('initial-chief');
  await openConnectionMethod(page,'副指挥','api');
  await page.locator('#planning-deputy-source').selectOption('deepSeek');
  await page.locator('#planning-deputy').fill('deputy-one');
  await openConnectionMethod(page,'主指挥','key');
  assert.equal(await role('主指挥').locator(':scope > details[open]').count(),1);
  assert.equal(await page.locator('#planning-chief').isVisible(),false);
  await openConnectionMethod(page,'主指挥','desktop');
  await page.locator('#planning-chief-session').fill('synthetic-chief-session');
  assert.equal(await role('主指挥').locator(':scope > details[open]').count(),1);
  assert.equal(await page.locator('#planning-chief').inputValue(),'external-codex-session');
  assert.equal(await page.locator('#planning-deputy').inputValue(),'deputy-one');
  await openConnectionMethod(page,'主指挥','api');
  assert.equal(await page.locator('#planning-chief-session').count(),0);
  await page.locator('#planning-chief').fill('chief-one');
  await openConnectionMethod(page,'副指挥','key');
  assert.equal(await role('副指挥').locator(':scope > details[open]').count(),1);
  await openConnectionMethod(page,'副指挥','api');
  assert.equal(await page.locator('#planning-deputy').inputValue(),'deputy-one');
  assert.equal((await submitted()).length,0);
  cases.push('Each manager has one open method and one current model; switching the chief leaves the deputy untouched');

  await openConnectionMethod(page,'执行端','cli');
  await page.locator('#planning-worker').fill('default-codex');
  await openConnectionMethod(page,'执行端','api');
  await role('执行端').getByRole('button',{name:'添加API 连接执行配置',exact:true}).click();
  await page.locator('#profile-0-description').fill('API tasks');
  await page.locator('#profile-0-model').fill('api-worker');
  await page.locator('#profile-0-artifacts').fill('api.txt');
  await role('执行端').getByRole('button',{name:'添加CLI 接入执行配置',exact:true}).click();
  await page.locator('#profile-1-description').fill('CLI tasks');
  await page.locator('#profile-1-exe').fill('D:/synthetic/codex.exe');
  await page.locator('#profile-1-model').fill('extra-codex');
  await page.locator('#profile-1-artifacts').fill('cli.txt');
  await openConnectionMethod(page,'执行端','desktop');
  assert.equal(await role('执行端').locator(':scope > details[open]').count(),3);
  assert.equal(await page.locator('#planning-worker').inputValue(),'default-codex');
  assert.equal(await page.locator('#profile-0-model').inputValue(),'api-worker');
  assert.equal(await page.locator('#profile-1-model').inputValue(),'extra-codex');
  const ids = await page.locator('[id]').evaluateAll(elements=>elements.map(element=>element.id));
  assert.equal(new Set(ids).size,ids.length);
  assert.equal(await page.locator('form form').count(),0);
  await page.screenshot({path:join(dir,'worker-multiple.png'),fullPage:true});
  cases.push('Worker API, CLI and desktop panels coexist and added API/CLI configurations preserve the default model with unique controls');

  const methodSummary = role('执行端').locator(':scope > details[data-method="desktop"] > summary');
  await methodSummary.focus();await page.keyboard.press('Enter');
  assert.equal(await role('执行端').locator(':scope > details[data-method="desktop"]').evaluate(element=>element.open),false);
  assert.equal(await role('执行端').locator(':scope > details[data-method="api"]').evaluate(element=>element.open),true);
  assert.equal(await role('执行端').locator(':scope > details[data-method="cli"]').evaluate(element=>element.open),true);
  cases.push('Keyboard collapse of one worker method keeps the other selected panels and configurations');

  await page.locator('#planning-chief').fill('');
  await openConnectionMethod(page,'主指挥','key');
  await next.click();
  assert.equal(await role('主指挥').locator(':scope > details[data-method="api"]').evaluate(element=>element.open),true);
  assert.equal(await role('主指挥').locator(':scope > details[open]').count(),1);
  assert.equal(await page.locator('#planning-chief').evaluate(element=>element.validity.valueMissing),true);
  assert.equal((await submitted()).length,0);
  await page.locator('#planning-chief').fill('chief-one');
  await next.click();await page.locator('#planning-requirement').waitFor();
  await page.locator('#planning-requirement').fill('Validate independent executor configurations');
  await page.locator('#planning-artifacts').fill('result.txt');
  await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  await page.waitForFunction(()=>window.calls.some(call=>call.args?.command?.type==='configureProject'));
  const commands=await submitted();assert.equal(commands.length,1);
  const saved=commands[0].draft;
  assert.deepEqual([saved.chiefModel,saved.deputyModel,saved.workerModel],['chief-one','deputy-one','default-codex']);
  assert.deepEqual(saved.profiles.map(profile=>[profile.workerAdapter,profile.workerModel]),[['deepseek','api-worker'],['codex','extra-codex']]);
  assert.equal(saved.managementSources.chief.kind,'openAi');
  assert.equal(saved.managementSources.deputy.kind,'deepSeek');
  cases.push('Validation reveals a missing field in a collapsed manager panel; final submission retains one model per manager and all worker selections');

  await nav('接入管理').click();await next.waitFor();
  assert.equal(await page.locator('.connection-method[open]').count(),0);
  assert.equal(await page.locator('#planning-chief').inputValue(),'chief-one');
  assert.equal(await page.locator('#profile-0-model').inputValue(),'api-worker');
  assert.equal(await page.locator('#profile-1-model').inputValue(),'extra-codex');
  await page.setViewportSize({width:1000,height:780});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.screenshot({path:join(dir,'collapsed-narrow.png'),fullPage:false});
  cases.push('Returning to access starts collapsed while restoring the existing selections and fitting a narrow window');
  assert.deepEqual(errors,[]);
  assert.equal(externalRequests,0);
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,pageErrors:errors,networkRequests:externalRequests,modelCalls:0,scope:'Production App and CSS, synthetic desktop observations and recording native IPC'},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,report:dir}));
}catch(error){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,errors},null,2));throw error;}
finally{await browser?.close();}
