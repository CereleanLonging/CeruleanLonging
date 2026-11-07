import {confirmConnectionContinue} from './connection-ui-helpers.mjs';
import {openConfiguredConnections,openConnectionMethod} from "./connection-ui-helpers.mjs";
// Production UI, isolated snapshots and recording IPC. Never contacts a provider.
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','Use the owned UI runner');
const dir=await mkdtemp(resolve('reports/ui-access-help-tests-786-'));
assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'ui-access-help','bundle');
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {App} from '../../apps/desktop/src/App';import {HelpHeading} from '../../apps/desktop/src/HelpHeading';const root=createRoot(document.getElementById('root'));window.showApp=()=>root.render(<App/>);window.showHelp=()=>root.render(<form onSubmit={e=>{e.preventDefault();window.formSubmits++;}}><HelpHeading as="h1">接入管理</HelpHeading><details><summary><HelpHeading as="h3">主指挥</HelpHeading></summary><p>detail</p></details><dialog><HelpHeading as="h2">确认退出工作台？</HelpHeading><button type="button" id="other">返回</button></dialog><button type="button" id="outside">外部</button></form>);window.showApp();`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'AccessHelpUI',formats:['iife'],fileName:()=> 'ui.js'}}});
const snapshot={dataDirectory:'isolated-ui-786',events:[],workflows:[{project:{id:'project',root:'D:/synthetic/project',timezone:'UTC'},state:{tasks:{}},tasks:[],approval:null}],projectSetups:[],workerAccountSetups:[],validators:[{id:'cerulean-objective-checker',name:'Fixture checks',args:[]}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:'D:/synthetic/codex.exe'},providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],ompJobs:[],verificationJobs:[],desktopBindings:[],adapters:[]};
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
      if(name==='independent_verifier_status'||name==='objective_checker_status')return {ready:false,message:'offline fixture'};
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
  await page.addStyleTag({path:'apps/desktop/src/style.css'});await page.addScriptTag({path:join(bundle,'ui.js')});
  const nav=name=>page.locator('.sidebar nav').getByRole('button',{name,exact:false});
  const submits=()=>page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit'));
  const next=page.getByRole('button',{name:'下一步：设置需求与计划',exact:true});
  const requirement=page.locator('#planning-requirement');
  const save=page.getByRole('button',{name:'保存需求与执行配置',exact:true});
  await page.getByText('本地核心已连接',{exact:true}).waitFor();
  const names=await page.locator('.sidebar nav button').allTextContents();assert(names.findIndex(n=>n.includes('接入管理'))<names.findIndex(n=>n.includes('需求与计划')));
  await page.getByRole('button',{name:'创建第一份计划',exact:false}).click();await next.waitFor();
  assert.equal(await nav('接入管理').getAttribute('aria-current'),'page');assert.equal(await requirement.isVisible(),false);
  await nav('工作总览').click();await page.getByRole('button',{name:'＋ 新建计划',exact:true}).click();await next.waitFor();
  assert.equal(await nav('接入管理').getAttribute('aria-current'),'page');assert.equal((await submits()).length,0);
  cases.push('Both new-plan entries open access management above planning without commands');

  await next.click();assert.equal(await nav('接入管理').getAttribute('aria-current'),'page');assert.equal(await page.locator('#planning-chief').evaluate(e=>e.validity.valueMissing),true);
  await openConfiguredConnections(page);
  await page.locator('#planning-chief').fill('chief-fixture');await page.locator('#planning-deputy').fill('deputy-fixture');await page.locator('#planning-worker').fill('worker-fixture');
  await next.click();await confirmConnectionContinue(page);await requirement.waitFor();await page.waitForFunction(()=>window.scrollY===0);assert.equal(await page.locator('#planning-chief').isVisible(),false);assert.equal(await page.locator('#planning-adapter').isVisible(),false);assert.equal((await submits()).length,0);
  const manual=page.locator('details[aria-label="手工计划与历史记录"]');
  assert.equal(await manual.evaluate(e=>e.open),false);
  assert.equal(await page.getByRole('heading',{name:'创建手工计划',exact:true}).isVisible(),false);
  await page.getByText('接入已保留 · 需求待保存',{exact:true}).waitFor();
  for(const model of ['chief-fixture','deputy-fixture','worker-fixture'])assert((await page.getByRole('region',{name:'已选角色模型'}).innerText()).includes(model));
  assert.ok(await page.locator('.project-planning').evaluate(e=>!!(e.compareDocumentPosition(document.querySelector('details[aria-label="手工计划与历史记录"]'))&Node.DOCUMENT_POSITION_FOLLOWING)));
  const top=await page.getByRole('heading',{name:'向主指挥提交需求',exact:true}).boundingBox();assert.ok(top.y<500);
  await page.screenshot({path:join(dir,'planning-primary-entry.png')});
  cases.push('Next lands on the retained role draft above the collapsed optional manual-plan form, before a complete setup exists');
  await openConfiguredConnections(page);
  await requirement.fill('保留需求与角色接入草稿');await page.locator('#planning-artifacts').fill('result.txt');
  cases.push('Required connection fields block Next; valid Next hides role options and sends no command');

  await page.evaluate(()=>window.readMode='deferred');await page.getByRole('button',{name:'修改接入配置',exact:true}).click();await page.waitForFunction(()=>window.deferred.length===1);
  await page.waitForFunction(()=>document.querySelector('#planning-project')?.matches(':disabled'));
  assert.equal(await next.isDisabled(),true);assert.equal(await page.locator('#planning-project').isDisabled(),true);assert.equal(await page.locator('#planning-chief').inputValue(),'chief-fixture');
  await page.evaluate(()=>{window.readMode='active';window.deferred[0].resolve(structuredClone(window.snapshot));});await page.waitForFunction(()=>!document.querySelector('#planning-chief').matches(':disabled'));
  await openConfiguredConnections(page);
  await page.locator('#planning-worker').fill('worker-changed');await next.click();await confirmConnectionContinue(page);await requirement.waitFor();assert.equal(await requirement.inputValue(),'保留需求与角色接入草稿');
  await nav('维护与升级').click();await page.getByText('local-ui-fixture',{exact:true}).waitFor();await nav('需求与计划').click();await requirement.waitFor();assert.equal(await requirement.inputValue(),'保留需求与角色接入草稿');
  assert.equal(await page.locator('#planning-worker').inputValue(),'worker-changed');
  cases.push('Draft survives access/planning, slow snapshots and maintenance; cached controls stay disabled until fresh');

  await page.evaluate(()=>window.readMode='error');await nav('接入管理').click();await page.getByRole('status').filter({hasText:'完整记录尚不可用'}).waitFor();assert.equal(await next.isDisabled(),true);
  await page.evaluate(()=>window.readMode='active');await page.getByRole('button',{name:'读取完整记录',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('#planning-chief').matches(':disabled'));
  assert.equal(await page.locator('#planning-chief').inputValue(),'chief-fixture');await next.click();await confirmConnectionContinue(page);await requirement.waitFor();assert.equal(await requirement.inputValue(),'保留需求与角色接入草稿');
  await save.click();await page.waitForFunction(()=>window.calls.some(c=>c.name==='core_submit'));
  const submitted=(await submits());assert.equal(submitted.length,1);const draft=submitted[0].args.command.draft;
  assert.deepEqual([draft.chiefModel,draft.deputyModel,draft.workerModel,draft.requirement],['chief-fixture','deputy-fixture','worker-changed','保留需求与角色接入草稿']);assert.deepEqual(draft.artifactPaths,['result.txt']);
  cases.push('Failed snapshot preserves draft; explicit retry then final save submits one complete configuration');

  const saved={digest:'a'.repeat(64),setup:{...draft,version:1,worker:{executable:{path:draft.codexExecutable},model:draft.workerModel},check:{id:'project-check',args:[],timeoutSeconds:60},acceptance:{schemaVersion:1,stages:draft.acceptance.stages.map(s=>({validatorId:s.validatorId,check:s}))},policy:{maxRequests:draft.workerMaxRequests,maxSeconds:draft.maxSeconds}}};
  await page.evaluate(saved=>{window.snapshot.projectSetups=[saved];window.snapshot.workflows[0].approval={};},saved);
  await openConfiguredConnections(page);
  await nav('接入管理').click();await next.waitFor();await page.getByRole('heading',{name:'Windows 执行账户',exact:true}).waitFor();await openConfiguredConnections(page);await page.locator('#planning-worker').fill('future-worker');await next.click();await confirmConnectionContinue(page);
  assert.equal(await page.getByRole('heading',{name:'Windows 执行账户',exact:true}).count(),0);
  await page.getByRole('button',{name:'保存为下一版草稿',exact:true}).click();await page.getByText('下一版草稿已保存在本机；当前任务保持原模型和目录。',{exact:true}).waitFor();assert.equal((await submits()).length,1);
  assert.equal(await page.evaluate(()=>JSON.parse(localStorage.getItem(JSON.stringify(['cerulean-next-execution','isolated-ui-786','project']))).draft.workerModel),'future-worker');
  cases.push('Approved project saves future draft locally and never replaces current execution configuration');

  await nav('接入管理').click();await next.waitFor();await openConfiguredConnections(page);
  await page.locator('#planning-chief').fill('replacement-chief');await next.click();await confirmConnectionContinue(page);await requirement.waitFor();
  const projectImport=page.locator('.project-planning .markdown-import-compact');
  await projectImport.getByRole('button',{name:'导入 Markdown 文档',exact:true}).waitFor();
  assert.equal(await projectImport.locator('input[type=file]').isDisabled(),false);assert.equal((await submits()).length,1);
  cases.push('A changed chief does not disable local import or cause a model submission');
  await nav('接入管理').click();await next.waitFor();await openConnectionMethod(page,'主指挥','desktop');
  await page.locator('#planning-chief-session').fill('retained-chief-session');await next.click();await confirmConnectionContinue(page);await requirement.waitFor();
  await page.getByText('Codex 主指挥会话：retained-chief-session',{exact:true}).waitFor();
  assert.equal(await projectImport.getByRole('button',{name:'导入 Markdown 文档',exact:true}).isEnabled(),true);
  assert.equal(await projectImport.locator('input[type=file]').isDisabled(),false);assert.equal(await requirement.isDisabled(),false);
  assert.equal((await submits()).length,1);assert.equal(await manual.evaluate(e=>e.open),false);
  await page.screenshot({path:join(dir,'planning-codex-session.png')});
  cases.push('Codex session and editable project requirements survive Next; local document import is available without substituting an API model');

  await nav('接入管理').click();await next.waitFor();await page.evaluate(()=>window.scrollTo(0,0));
  const heading=page.getByRole('heading',{name:'接入管理',exact:true});assert.equal(await heading.count(),1);
  const help=heading.getByRole('button',{name:'解释：接入管理',exact:true});await help.hover();await page.getByRole('tooltip').waitFor();await help.click();assert.match(await page.getByRole('tooltip').innerText(),/主指挥、副指挥和执行端/);
  await page.keyboard.press('Escape');assert.equal(await page.getByRole('tooltip').count(),0);
  assert.equal(await page.locator('h1:visible,h2:visible,h3:visible,h4:visible,h5:visible,h6:visible,legend:visible').evaluateAll(es=>es.every(e=>e.querySelector('button.heading-help'))),true);
  await nav('接入管理').focus();await help.focus();await page.getByRole('tooltip').waitFor();await page.screenshot({path:join(dir,'access-heading-help.png'),fullPage:false});
  await nav('需求与计划').click();assert.equal(await page.getByRole('tooltip').count(),0);assert.equal((await submits()).length,1);
  cases.push('Visible headings retain accessible names, show explanations by hover/focus/click, close on Escape/navigation');

  await page.setViewportSize({width:1000,height:520});const sidebar=page.locator('.sidebar nav');
  const scrollbar=await sidebar.evaluate(e=>({style:getComputedStyle(e).scrollbarWidth,webkit:getComputedStyle(e,'::-webkit-scrollbar').display,overflow:e.scrollHeight>e.clientHeight}));assert.equal(scrollbar.style,'none');assert.equal(scrollbar.webkit,'none');assert.equal(scrollbar.overflow,true);
  await sidebar.evaluate(e=>e.scrollTop=0);await sidebar.hover();await page.mouse.wheel(0,800);await page.waitForFunction(()=>document.querySelector('.sidebar nav').scrollTop>0);
  await sidebar.evaluate(e=>e.scrollTop=0);await nav('工作总览').focus();for(let i=0;i<6;i++)await page.keyboard.press('Tab');assert.equal(await nav('维护与升级').evaluate(e=>e===document.activeElement),true);assert.ok(await sidebar.evaluate(e=>e.scrollTop)>0);
  await page.screenshot({path:join(dir,'sidebar-no-scrollbar.png')});
  cases.push('Short-window sidebar hides scrollbar while wheel and keyboard reveal final navigation entry');

  await page.evaluate(()=>window.snapshot.workflows=[]);await nav('工作总览').click();await page.getByRole('button',{name:'＋ 新建计划',exact:true}).click();await page.getByRole('button',{name:'关联项目并配置主指挥',exact:true}).click();
  assert.equal(await nav('项目设置').getAttribute('aria-current'),'page');assert.equal((await submits()).length,1);
  cases.push('No-project access view leads to project registration without requesting or sending a model task');

  await page.evaluate(()=>window.showHelp());await page.setViewportSize({width:390,height:600});
  const summary=page.locator('summary');await summary.getByRole('button').click();assert.equal(await page.locator('details').getAttribute('open'),null);assert.equal(await page.evaluate(()=>window.formSubmits),0);
  await page.getByRole('button',{name:'外部',exact:true}).click();assert.equal(await page.getByRole('tooltip').count(),0);
  await page.evaluate(()=>document.querySelector('dialog').showModal());await page.locator('dialog').getByRole('button',{name:'解释：确认退出工作台？',exact:true}).click();
  const tip=page.getByRole('tooltip');await tip.waitFor();assert.equal(await page.locator('dialog [role=tooltip]').count(),1);
  const box=await tip.boundingBox();assert.ok(box.x>=0&&box.x+box.width<=390&&box.y>=0&&box.y+box.height<=600);
  assert.equal(await tip.evaluate(e=>{const r=e.getBoundingClientRect();return e.contains(document.elementFromPoint(r.left+10,r.top+10));}),true);
  await page.screenshot({path:join(dir,'native-dialog-help.png')});await page.keyboard.press('Escape');assert.equal(await page.locator('dialog').evaluate(e=>e.open),true);assert.equal(await tip.count(),0);await page.keyboard.press('Escape');assert.equal(await page.locator('dialog').evaluate(e=>e.open),false);
  assert.equal(await page.evaluate(()=>window.formSubmits),0);
  cases.push('Help never submits forms or toggles summaries; native modal tooltip stays visible and Escape first closes help');
  assert.deepEqual(errors,[]);assert.equal(externalRequests,0);
  await writeFile(join(dir,'result.json'),JSON.stringify({passed:true,cases,errors,externalRequests,modelCalls:0,nativeActions:0,scope:'Production React and CSS with synthetic snapshots and recording IPC; Edge headless'},null,2));
  console.log(JSON.stringify({passed:true,cases:cases.length,report:dir}));
} catch(error) {await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,cases,errors},null,2));throw error;}
finally {await browser?.close();}
