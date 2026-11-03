// Real App navigation with an explicit offline IPC fixture. No local accounts or model calls.
import {mkdtemp,writeFile,readFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
const dir=await mkdtemp(resolve('reports/desktop-configuration-ui-780-'));
const fixture={product:'CeruleanLonging',protocolVersion:1,dataDirectory:'D:\\fixture-data',
  managementHistory:{projects:[]},openaiConnection:{state:'unconfigured',keyConfigured:false,models:[]},
  codexConnection:{state:'available',executable:'D:\\fixture-codex.exe',models:[]},
  validators:[{id:'tests',name:'项目测试程序',tool:{executable:{path:'D:\\fixture-validator.exe',sha256:'a'.repeat(64)},inputs:[],environment:{}}}]};
for(const name of ['blindReviews','workerAccountSetups','baselines','teamTimelines','apiPublicationReviews','apiJobs','managementRecordReviews','startupClosureReviews','startupReservations','plans','events','adapters','workflows','verificationJobs','ompJobs','ompStopIssues','codexJobs','codexCancellations','codexStartups','unsentAssignments','providerCredentials','providerConnections','projectSetups','currencyBudgets','projectRuns','projectRevisions','managementDecisions','desktopBindings'])fixture[name]=[];
for(const name of ['teamImport','baselineReview','desktopAccess','apiRecovery','coreDiagnostic','verificationDetail','ompFiles','workerObservation','codexStartupDetail'])fixture[name]=null;
const entry=join(dir,'entry.tsx');
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {App} from '../../apps/desktop/src/App';createRoot(document.getElementById('root')).render(<App/>);`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'DesktopConfiguration',formats:['iife'],fileName:()=> 'ui.js'}}});
const cases=[],errors=[];let browser,page,network=0;
try{
  browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  page=await browser.newPage({viewport:{width:1280,height:900}});page.setDefaultTimeout(15000);page.on('pageerror',e=>errors.push(String(e)));
  await page.route('**/*',route=>route.request().url()==='http://localhost/configuration-fixture'?route.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>'}):(network++,route.abort()));
  await page.goto('http://localhost/configuration-fixture');
  await page.evaluate(fixture=>{
    window.fixture=fixture;window.commands=[];window.unexpected=[];window.pendingSnapshots=[];window.delaySnapshots=false;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
    window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},transformCallback:()=>1,invoke:async(name,args)=>{
      if(name==='independent_verifier_status')return window.verifierStatus??{ready:true,version:'0.1.39',path:'D:\\fixture-program\\cerulean-verifier.exe',sha256:'a'.repeat(64),error:null};
      if(name==='plugin:event|listen')return 1;
      if(name==='plugin:event|unlisten'||name==='cancel_workbench_exit')return;
      if(name==='experiment_workbenches')return {currentLabel:'main',currentDirectory:'D:\\fixture-data',catalogRoot:'D:\\fixture-experiments',entries:[]};
      if(name==='workspace_read'){
        const q=args.query;
        if(q.type==='overview')return {type:'overview',value:{mode:'ready',dataDirectory:'D:\\fixture-data',legacyPlanCount:0,pendingLegacyPlanCount:0,projectCount:window.fixture.workflows.length}};
        if(q.type==='plans'||q.type==='projects')return {type:q.type,value:{revision:'fixture',offset:0,nextOffset:null,total:0,entries:[]}};
      }
      if(name==='core_request'){
        if(window.delaySnapshots)await new Promise(resolve=>window.pendingSnapshots.push(resolve));
        return structuredClone(window.fixture);
      }
      if(name==='core_submit'){
        const c=args.command;window.commands.push(c);
        if(c.type==='registerProject')window.fixture.workflows=[{project:c.project,state:{tasks:{},revisionRequested:false},approval:null,revision:null,revisionDigest:null,history:[]}];
        else if(c.type==='configureProject'){
          const d=c.draft;window.fixture.projectSetups=[{digest:'a'.repeat(64),setup:{...d,version:1,worker:{executable:{path:d.codexExecutable},model:d.workerModel},check:{id:'project-check',args:d.checkArgs,timeoutSeconds:60},acceptance:{schemaVersion:1,stages:d.acceptance.stages.map(s=>({validatorId:s.validatorId,check:{...s}}))},policy:{maxRequests:d.workerMaxRequests,maxSeconds:d.maxSeconds}}}];
        }else throw Error('Unexpected mutation '+c.type);
        return {type:'receipt',value:{requestId:'fixture',sequence:window.commands.length}};
      }
      window.unexpected.push(name);throw Error('Unexpected fixture request '+name);
    }};
  },fixture);
  await page.addStyleTag({path:resolve('apps/desktop/src/style.css')});await page.addScriptTag({content:await readFile(join(dir,'bundle/ui.js'),'utf8')});
  await page.getByText('本地核心已连接',{exact:true}).waitFor();
  const footer=page.locator('.sidebar-footer'),button=footer.getByRole('button',{name:'退出工作台',exact:true});
  const box=await button.boundingBox(),version=await footer.locator('small').boundingBox();
  assert.ok(box.height<=32&&box.width<=100);assert.ok(box.y-(version.y+version.height)>=15);
  await footer.screenshot({path:join(dir,'footer.png')});cases.push('Exit is compact, keyboard focusable and separated from the version by 16px');
  await page.getByRole('navigation').getByRole('button',{name:'接入管理'}).click();
  await page.getByLabel('主指挥接入方式').selectOption('desktop');
  assert.equal(await page.getByRole('button',{name:'配置主指挥模型',exact:true}).isDisabled(),false);
  await page.getByRole('button',{name:'绑定 Codex 聊天',exact:true}).click();
  await page.getByRole('region',{name:'模型配置引导'}).waitFor();
  assert.equal(await page.getByRole('button',{name:'继续配置模型',exact:true}).isDisabled(),true);
  assert.deepEqual(await page.evaluate(()=>window.commands),[]);
  assert.equal(await page.getByLabel('项目目录',{exact:true}).inputValue(),'D:\\test\\coding');
  await page.getByLabel('项目目录',{exact:true}).fill('D:\\fixture-project');
  cases.push('Project directory defaults to D:\\test\\coding, remains editable and registers only after confirmation');
  await page.getByRole('button',{name:'关联项目目录',exact:true}).click();
  await page.waitForFunction(()=>window.commands.length===1);
  await page.waitForFunction(()=>!Array.from(document.querySelectorAll('button')).find(e=>e.textContent==='继续配置模型')?.disabled);
  cases.push('A desktop-chief request without projects guides registration without saving a model or dispatching tasks');
  await page.evaluate(()=>window.delaySnapshots=true);
  await page.getByRole('button',{name:'继续配置模型',exact:true}).click();
  await page.waitForFunction(()=>window.pendingSnapshots.length>0);
  assert.equal(await page.locator('#planning-chief-source').count(),0);
  await page.evaluate(()=>{window.delaySnapshots=false;window.pendingSnapshots.splice(0).forEach(resolve=>resolve());});
  await page.waitForFunction(()=>document.querySelector('#planning-chief-source')?.value==='codexSession'&&document.activeElement?.id==='planning-chief-source');
  assert.equal(await page.getByLabel('主指挥模型 ID',{exact:true}).inputValue(),'external-codex-session');
  assert.equal(await page.getByLabel('绑定的 Codex 会话标识').isVisible(),true);
  assert.equal(await page.evaluate(()=>window.commands.length),1);
  await page.locator('[aria-label="主指挥模型配置"]').screenshot({path:join(dir,'desktop-chief.png')});
  cases.push('Desktop selection and focus survive delayed native snapshot loading and draft initialization');
  await page.getByLabel('绑定的 Codex 会话标识').fill('fixture-session-780');
  await page.getByLabel('项目需求',{exact:true}).fill('实现并验证本地 CSV 工具');
  await page.getByLabel('编程模型 ID',{exact:true}).fill('fixture-worker');
  await page.getByLabel('副指挥接入',{exact:true}).selectOption('deepSeek');
  await page.getByLabel('必须交付的文件（项目相对路径，每行一个）').fill('sum.mjs');
  await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  await page.waitForFunction(()=>window.commands.length===2);
  const saved=await page.evaluate(()=>window.commands[1]);
  assert.deepEqual(saved.draft.managementSources,{chief:{kind:'codexSession',sessionId:'fixture-session-780'},deputy:{kind:'deepSeek'}});
  assert.equal(saved.draft.deputyModel,'deepseek-flash');
  cases.push('Only explicit Save submits the chosen desktop binding, preserving deputy configuration');
  await page.getByRole('navigation').getByRole('button',{name:'接入管理'}).click();
  await page.getByLabel('主指挥接入方式').selectOption('desktop');
  await page.getByRole('button',{name:'配置主指挥模型',exact:true}).click();
  await page.waitForFunction(()=>document.activeElement?.id==='planning-chief-source');
  assert.equal(await page.getByLabel('绑定的 Codex 会话标识').inputValue(),'fixture-session-780');
  assert.equal(await page.getByLabel('副指挥模型 ID',{exact:true}).inputValue(),'deepseek-flash');
  assert.equal(await page.evaluate(()=>window.commands.length),2);
  cases.push('Revisiting an existing desktop chief retains the session and other roles without resubmission');
  const select=page.locator('#document-refinement-project');
  assert.equal(await select.locator('option').count(),2);
  await page.getByRole('navigation').getByRole('button',{name:'工作总览'}).click();
  await page.evaluate(()=>{window.fixture.workflows=[];window.fixture.projectSetups=[];});
  await page.getByRole('navigation').getByRole('button',{name:'需求与计划'}).click();
  await page.waitForFunction(()=>document.querySelector('#document-refinement-project')?.disabled);
  await page.locator('.document-chief-configuration').screenshot({path:join(dir,'empty-project-guide.png')});
  await page.getByRole('button',{name:'配置项目主指挥',exact:true}).click();
  await page.getByRole('region',{name:'模型配置引导'}).waitFor();
  assert.equal(await page.evaluate(()=>window.commands.length),2);
  cases.push('Empty document-chief selector explains the prerequisite and opens setup instead of a dead dropdown');
  await page.evaluate(()=>{window.fixture.workflows=[{project:{id:'later-project',root:'D:\\later-project'},state:{tasks:{},revisionRequested:false},approval:null,revision:null,history:[]}];});
  await page.getByRole('navigation').getByRole('button',{name:'接入管理'}).click();
  await page.getByLabel('主指挥接入方式').selectOption('desktop');
  await page.evaluate(()=>window.delaySnapshots=true);
  await page.getByRole('button',{name:'绑定 Codex 聊天',exact:true}).click();
  await page.waitForFunction(()=>window.pendingSnapshots.length>0);
  await page.locator('.brand').click();
  await page.evaluate(()=>{window.delaySnapshots=false;window.pendingSnapshots.splice(0).forEach(resolve=>resolve());});
  await page.getByRole('navigation').getByRole('button',{name:'需求与计划'}).click();
  await page.waitForFunction(()=>document.querySelector('#planning-chief-source')?.value==='openAi');
  assert.equal(await page.evaluate(()=>window.commands.length),2);
  cases.push('Leaving during a delayed read cancels the configuration intent; late results cannot switch a later draft');
  await page.getByRole('navigation').getByRole('button',{name:'项目设置'}).click();
  const verifier=page.getByRole('region',{name:'验收工具登记'});
  await verifier.getByText('已绑定 · 0.1.39 · 程序摘要匹配',{exact:true}).waitFor();
  assert.equal(await page.getByLabel('默认独立验收工具',{exact:true}).isDisabled(),true);
  assert.equal(await page.getByLabel('程序完整路径',{exact:true}).isVisible(),false);
  assert.equal(await page.evaluate(()=>window.commands.length),2);
  await verifier.screenshot({path:join(dir,'bound-verifier.png')});
  cases.push('The independent tool has a fixed binding; project-specific checks are advanced settings and no model task is started');
  await page.evaluate(()=>window.verifierStatus={ready:false,version:'0.1.39',path:'D:\\fixture-program\\cerulean-verifier.exe',sha256:null,error:'独立验收工具与工作台构建摘要不符；未启动'});
  await page.getByRole('button',{name:'重新核对绑定',exact:true}).click();
  await page.getByText('独立验收工具未就绪，不能开始盲评。',{exact:true}).waitFor();
  assert.equal(await page.getByLabel('默认独立验收工具',{exact:true}).isDisabled(),true);
  assert.equal(await page.evaluate(()=>window.commands.length),2);
  await verifier.screenshot({path:join(dir,'verifier-unavailable.png')});
  cases.push('A verifier integrity error remains unavailable with a locked binding and no model or fallback command');
  await page.evaluate(()=>window.verifierStatus=null);
  await page.getByRole('button',{name:'重新核对绑定',exact:true}).click();
  await verifier.getByText('已绑定 · 0.1.39 · 程序摘要匹配',{exact:true}).waitFor();
  // Match the minimum native window size; this desktop app does not expose a 390px window.
  const windowConfig=JSON.parse(await readFile('apps/desktop/src-tauri/tauri.conf.json','utf8')).app.windows[0];
  await page.setViewportSize({width:windowConfig.minWidth,height:windowConfig.minHeight});
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  const compactExit=await page.getByRole('button',{name:'退出工作台',exact:true}).boundingBox();
  assert.ok(compactExit.y>=0&&compactExit.y+compactExit.height<=windowConfig.minHeight);
  await page.screenshot({path:join(dir,'minimum-window.png')});
  await page.screenshot({path:join(dir,'narrow-setup.png'),fullPage:true});
  assert.equal(network,0);assert.deepEqual(errors,[]);assert.deepEqual(await page.evaluate(()=>window.unexpected),[]);
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,errors,networkRequests:network,providerCalls:0},null,2));
  console.log(JSON.stringify({status:'passed',cases:cases.length,report:dir}));
}catch(e){await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),cases,errors},null,2));await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});throw e;}
finally{await browser?.close();}
