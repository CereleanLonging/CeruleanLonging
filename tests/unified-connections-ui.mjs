import {confirmConnectionContinue} from './connection-ui-helpers.mjs';
import {openConfiguredConnections,openConnectionMethod,deferModelChoice} from "./connection-ui-helpers.mjs";
// Production UI, isolated snapshots and recording IPC. Never contacts a provider.
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','Use the owned UI runner');
const dir=await mkdtemp(resolve('reports/unified-connections-ui-788-'));
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
  const nav = name => page.locator('.sidebar nav').getByRole('button', {name, exact:false});
  const commands = () => page.evaluate(() => window.calls.filter(call => call.name === 'core_submit').map(call => call.args.command));
  await page.getByText('本地核心已连接', {exact:true}).waitFor();
  await page.evaluate(() => {
    const first = structuredClone(window.snapshot.workflows[0]);
    first.project = {...first.project, id:'second', root:'D:/synthetic/second'};
    window.snapshot.workflows.push(first);
    window.snapshot.desktopAccess = {windows:[],bindings:[],trials:[],bridgeSha256:'a'.repeat(64),checking:false};
    window.snapshot.providerConnections = [{provider:'a6api',target:{kind:'a6ApiVip'},state:'listed',keyConfigured:true,models:['relay-model']}];
    const originalInvoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (name, args) => {
      if (name === 'cancel_workbench_exit') return;
      if (name === 'scan_codex_installations') {
        window.calls.push({name,args});
        return {candidates:[{executable:'D:/synthetic/scanned/codex.exe',directory:'D:/synthetic/scanned',source:'fixture'}],warnings:[],limited:false};
      }
      const command = args?.command;
      if (name !== 'core_submit' || !['discoverDesktop','inspectDesktop','bindDesktop','bindDesktopSession','checkProviderModels','setProviderKey','checkCodex'].includes(command.type)) {
        return originalInvoke(name,args);
      }
      window.calls.push({name,args});
      const access = window.snapshot.desktopAccess;
      if (command.type === 'discoverDesktop') {
        access.windows = [{app:'qoder',title:'Synthetic Qoder',version:'0.4.3.0',hwnd:'100',pid:123,startTicks:456,exe:'D:/synthetic/Qoder.exe'}];
      }
      if (command.type === 'inspectDesktop') {
        access.observation = {window:command.window,modelLabel:'fixture-model',sessionId:'fixture-session',capturedAt:'2026-11-04T01:00:00Z',documentRuntimeId:'fixture-runtime',contentDigest:'b'.repeat(64),layoutDigest:'c'.repeat(64),nodeCount:1,draftEmpty:true,busy:false};
        access.observationDigest = 'd'.repeat(64);
      }
      if (command.type === 'bindDesktopSession') {
        const id = access.bindings.length ? `binding-second-${access.bindings.length+1}` : 'binding-second';
        const observation={window:{app:'qoder',title:'Synthetic Qoder',version:'0.4.3.0',hwnd:'100',pid:123,startTicks:'456',exe:'D:/synthetic/Qoder.exe'},modelLabel:'fixture-model',sessionId:command.sessionId,capturedAt:'2026-11-04T01:00:00Z',documentRuntimeId:'fixture-runtime',contentDigest:'b'.repeat(64),layoutDigest:'c'.repeat(64),nodeCount:1,draftEmpty:true,busy:false};
        access.bindings.push({id,label:'Qoder '+command.sessionId,projectId:command.projectId,observation,bridgeSha256:access.bridgeSha256,confirmedAt:'2026-11-04T01:00:00Z'});
      }
      return {type:'receipt',value:{requestId:'fixture-'+window.calls.length}};
    };
  });
  await nav('接入管理').click();
  const next = page.getByRole('button',{name:'下一步：设置需求与计划',exact:true});
  await next.waitFor();
  assert.equal(await page.getByRole('heading',{name:'凭证与连接检查',exact:true}).count(),1);
  for (const name of ['主指挥','副指挥','执行端']) {
    assert.equal(await page.getByRole('heading',{name,exact:true}).count(),1);
  }
  assert.equal(await page.locator('.project-directories').getAttribute('open'),null);
  assert.equal(await page.locator('#integration-chief-method').count(),0);
  assert.equal(await page.locator('form form').count(),0);
  assert.equal(await page.locator('#planning-executable').count(),1);
  cases.push('One top-level credential and connection section, three unique role cards and no duplicate provider/path inputs or nested forms');

  const deputy = page.getByRole('region',{name:'副指挥',exact:true});
  await openConnectionMethod(page,'主指挥','desktop');
  await openConfiguredConnections(page);
  await page.locator('#planning-chief-session').fill('fixture-codex-session');
  assert.equal(await page.locator('#planning-chief').inputValue(),'external-codex-session');
  await page.locator('#planning-deputy-source').selectOption('provider:a6ApiVip');
  await openConfiguredConnections(page);
  await page.locator('#planning-deputy').fill('relay-model');
  await openConnectionMethod(page,'副指挥','api');
  await deputy.getByRole('button',{name:'测试连通',exact:true}).click();
  await deferModelChoice(page);
  await page.waitForFunction(() => window.calls.some(call => call.args?.command?.type === 'checkProviderModels'));
  assert.deepEqual((await commands()).filter(command=>command.type==='checkProviderModels'),[{type:'checkProviderModels',target:{kind:'a6ApiVip'}}]);
  assert.equal(await deputy.locator('select[id$="-origin"]').count(),0);
  await deputy.locator('summary').filter({hasText:/^Key 管理$/}).click();
  await deputy.getByLabel('A6API API Key',{exact:true}).fill('synthetic-offline-credential-only');
  await deputy.getByRole('button',{name:'加密保存 A6API Key',exact:true}).click();
  await deferModelChoice(page);
  await page.waitForFunction(() => window.calls.some(call => call.args?.command?.type === 'setProviderKey'));
  assert.equal((await commands()).some(command=>command.type==='configureProject'),false);
  assert.equal(await deputy.getByLabel('A6API API Key',{exact:true}).inputValue(),'');
  cases.push('Connection check uses the project-selected relay endpoint; credential form submits independently of project configuration');

  const worker = page.getByRole('region',{name:'执行端',exact:true});
  await openConnectionMethod(page,'执行端','cli');
  await worker.getByRole('button',{name:'扫描本地 Codex',exact:true}).click();
  await worker.getByRole('button',{name:'使用此路径',exact:true}).click();
  assert.equal(await page.locator('#planning-executable').inputValue(),'D:/synthetic/scanned/codex.exe');
  await openConfiguredConnections(page);
  await page.locator('#planning-worker').fill('fixture-codex-model');
  await worker.getByRole('region',{name:'Codex 连接检查',exact:true}).getByRole('button',{name:'测试连通',exact:true}).click();
  await page.waitForFunction(() => window.calls.some(call => call.args?.command?.type === 'checkCodex'));
  assert.equal((await commands()).find(command=>command.type==='checkCodex').executable,'D:/synthetic/scanned/codex.exe');
  await next.click();await confirmConnectionContinue(page);
  await page.locator('#planning-requirement').waitFor({state:'visible'});
  assert.equal((await commands()).some(command=>command.type==='configureProject'),false);
  cases.push('Scanned Codex path is the task path; empty credential fields do not block Next and checks do not save or start a project');

  await nav('接入管理').click();
  await page.locator('#planning-project').selectOption('second');
  await openConfiguredConnections(page);
  await page.locator('#planning-chief').fill('second-chief');
  await openConfiguredConnections(page);
  await page.locator('#planning-deputy').fill('second-deputy');
  await openConnectionMethod(page,'执行端','desktop');
  await worker.getByRole('button',{name:'将桌面端接入设为默认执行端',exact:true}).click();
  const session = page.locator('#planning-desktop-session-id');
  await next.click();
  assert.equal(await session.evaluate(element=>element.validity.valueMissing),true);
  assert.equal(await nav('接入管理').getAttribute('aria-current'),'page');
  await session.fill('12345678-1234-1234-1234-123456789abc');
  assert.equal(await session.evaluate(element=>element.checkValidity()),false);
  await worker.getByRole('button',{name:'测试并绑定会话',exact:true}).click();
  await worker.getByRole('status').filter({hasText:'会话已核对并绑定'}).waitFor();
  assert.equal((await commands()).find(command=>command.type==='bindDesktopSession').projectId,'second');
  await worker.getByText('已绑定模型 · fixture-model',{exact:true}).waitFor();
  assert.equal(await session.evaluate(element=>element.checkValidity()),true);
  assert.equal(await worker.getByRole('button',{name:'发现已打开窗口',exact:true}).count(),0);
  assert.equal((await commands()).some(command=>/Trial|sendDesktop|start|requestChief/i.test(command.type)),false);
  cases.push('Qoder requires a verified session ID; one binding command uses the actual second project and fills the model without messages');
  await page.screenshot({path:join(dir,'unified-qoder.png'),fullPage:true});

  const scopedDirectory = () => page.waitForFunction(() => {
    const label = [...document.querySelectorAll('[aria-label="桌面执行授权范围"] dt')].find(element=>element.textContent==='项目目录');
    return /executors\\[a-f0-9]{64}$/.test(label?.nextElementSibling?.textContent??'');
  });
  await scopedDirectory();
  await session.fill('12345678-1234-1234-1234-123456789def');
  await worker.getByRole('button',{name:'测试并绑定会话',exact:true}).click();
  await worker.getByRole('status').filter({hasText:'会话已核对并绑定'}).waitFor();
  await scopedDirectory();
  cases.push('Rebinding another chat for the same model retains its assigned executor directory');

  await page.locator('#planning-project').selectOption('project');
  assert.equal(await page.locator('#planning-executable').inputValue(),'D:/synthetic/scanned/codex.exe');
  assert.equal(await page.locator('#planning-deputy-source').inputValue(),'provider:a6ApiVip');
  assert.equal(await page.locator('#planning-chief-session').inputValue(),'fixture-codex-session');
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:join(dir,'unified-connections.png'),fullPage:false});
  cases.push('Switching projects restores each project model, endpoint and scanned path in the unified controls');

  await nav('维护与升级').click();
  await page.getByRole('region',{name:'当前安装信息',exact:true}).waitFor();
  await page.setViewportSize({width:1440,height:1000});
  const maintenance = page.getByRole('region',{name:'维护与升级',exact:true});
  const upgrade = maintenance.getByRole('region',{name:'升级与恢复入口',exact:true});
  const logs = maintenance.getByRole('region',{name:'只读日志入口',exact:true});
  let a = await upgrade.boundingBox(), b = await logs.boundingBox();
  assert(b.x >= a.x + a.width + 20);
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:join(dir,'maintenance-wide.png'),fullPage:false});
  await page.setViewportSize({width:1000,height:800});
  a = await upgrade.boundingBox(); b = await logs.boundingBox();
  assert(b.y >= a.y + a.height + 20);
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.screenshot({path:join(dir,'maintenance-narrow.png'),fullPage:false});
  cases.push('Maintenance cards have spaced columns on wide windows, stack on narrow windows and keep paths within the page');

  await page.getByRole('button',{name:'退出工作台',exact:true}).click();
  const exit = page.getByRole('dialog',{name:'确认退出工作台？',exact:true});
  await exit.waitFor();
  assert.equal(await page.getByRole('tooltip').count(),0);
  assert.equal(await exit.getByRole('button',{name:'返回工作台',exact:true}).evaluate(button=>button===document.activeElement),true);
  const help = exit.getByRole('button',{name:'解释：确认退出工作台？',exact:true});
  await help.focus();
  assert.equal(await page.getByRole('tooltip').count(),0);
  await help.hover();await page.getByRole('tooltip').waitFor();
  await help.click();await page.keyboard.press('Escape');
  assert.equal(await page.getByRole('tooltip').count(),0);
  assert.equal(await exit.isVisible(),true);
  await exit.getByRole('button',{name:'返回工作台',exact:true}).focus();
  await page.mouse.move(0,0);
  await page.screenshot({path:join(dir,'exit-help-idle.png')});
  await exit.getByRole('button',{name:'返回工作台',exact:true}).click();
  cases.push('Exit help remains closed on open and focus, opens on pointer interaction, and Escape dismisses the hint before the dialog');

  assert.deepEqual(errors,[]);
  assert.equal(externalRequests,0);
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,pageErrors:errors,networkRequests:externalRequests,modelCalls:0,scope:'Production App and CSS, synthetic desktop observations and recording native IPC'},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,report:dir}));
}catch(error){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,errors},null,2));throw error;}
finally{await browser?.close();}
