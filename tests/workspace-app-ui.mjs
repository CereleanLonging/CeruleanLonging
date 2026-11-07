import {mkdtemp,readFile,writeFile} from 'node:fs/promises';import {join} from 'node:path';import {createHash} from 'node:crypto';import assert from 'node:assert/strict';import {build} from 'vite';import {chromium} from 'playwright-core';
if(process.env.CERULEAN_NATIVE_OWNED_JOB!=='1')throw Error('Use owned runner');
const dir=await mkdtemp('reports/workspace-app-ui-151-'),cases=[],errors=[];let browser,page,networkRequests=0;
assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'workspace-app','bundle');
const files=['App.tsx','WorkspaceBrowser.tsx','commandSubmission.ts','contracts.ts','style.css'].map(n=>'apps/desktop/src/'+n);const hashes=async()=>Object.fromEntries(await Promise.all(files.map(async n=>[n,createHash('sha256').update(await readFile(n)).digest('hex')])));const before=await hashes();
const entry=join(dir,'entry.tsx');await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {App} from '../../apps/desktop/src/App';createRoot(document.getElementById('root')!).render(<App/>);`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'WorkspaceApp',formats:['iife'],fileName:()=> 'ui.js'}}});
try{
 browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});page=await browser.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));await page.route('**/*',r=>{if(r.request().url()==='http://localhost/workspace-ui-fixture')return r.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><div id="root"></div></html>'});networkRequests++;return r.abort();});await page.goto('http://localhost/workspace-ui-fixture');assert.equal(await page.evaluate(()=>window.isSecureContext&&typeof crypto.randomUUID==='function'),true);
 await page.evaluate(()=>{
  window.calls=[];window.overviewMessage=null;window.plan=null;window.failSubmit=false;window.fullMode='error';window.deferred=[];
  window.stubSnapshot={dataDirectory:'isolated',events:[],workflows:[],projectSetups:[],validators:[],managementHistory:{projects:[]},verificationJobs:[],apiJobs:[{state:'executing'}]};
  window.stubSnapshot.openaiConnection={state:'unconfigured',keyConfigured:false,models:[]};
  for(const key of ['providerConnections','adapters','projectRuns','codexJobs','ompJobs','workerAccountSetups'])window.stubSnapshot[key]=[];
  window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
  window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'}},transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(name,args)=>{
   window.calls.push({name,args});
   if(name==='plugin:event|listen')return 1;
   if(name==='plugin:event|unlisten')return;
   if(name==='experiment_workbenches')return {currentLabel:'main',currentDirectory:'isolated',catalogRoot:'experiments',entries:[]};
   if(['independent_verifier_status','objective_checker_status'].includes(name))return {ready:true,version:'fixture',path:'D:/synthetic/'+name+'.exe',sha256:'a'.repeat(64),error:null};
   if(name==='count_resolution_request'){
    const keys={readManagementCounts:'counts',readApiCounts:'apiCounts',readApiGenerations:'apiGenerations'};
    const key=keys[args.command.type];if(!key)throw Error('Unexpected count mutation');
    return {[key]:{revision:'count-fixture',offset:0,total:0,nextOffset:null,entries:[]}};
   }
   if(name==='historical_request'){
    if(!['readHistoricalSources','readHistoricalAccounting'].includes(args.command.type))throw Error('Unexpected historical mutation');
    return {historicalSources:{revision:'history-fixture',offset:0,total:0,pendingTotal:0,nextOffset:null,entries:[]},historicalAccounting:{revision:'history-fixture',offset:0,total:0,nextOffset:null,entries:[]},currencyBudgets:[]};
   }
   if(name==='core_submit'){
    if(args.command.type==='snapshot')throw Error('responseTooLarge: original Snapshot');
    if(args.command.type!=='createPlan')throw Error('unexpected command');
    if(window.failSubmit)throw Error('result unknown; not resent');
    window.plan={id:'original-id',revision:1,title:args.command.title,digest:'a'.repeat(64),createdAt:'2026-10-14',approved:false,taskCount:args.command.tasks.length};
    return {type:'receipt',value:{requestId:'app-single-create'}};
   }
   if(name==='core_request'){
    if(window.fullMode==='deferred')return new Promise((resolve,reject)=>window.deferred.push({resolve,reject}));
    if(window.fullMode==='active')return structuredClone(window.stubSnapshot);
    throw Error('responseTooLarge: refresh');
   }
   if(name==='maintenance_info')return {version:'controlled-ui-only',dataDirectory:'isolated',installationDirectory:'isolated',holdPresent:false,bundleSchema:1};
   if(name!=='workspace_read')throw Error('unexpected invoke '+name);
   const q=args.query;
   if(q.type==='projects')return {type:'projects',value:{revision:'v1',offset:0,total:1,nextOffset:null,entries:[{project:{id:'sample-project',root:'D:/synthetic/project',timezone:'UTC'},taskCount:3,resourceHoldingTasks:1,unknownTasks:1,revisionDigest:null}]}};
   if(q.type==='overview')return {type:'overview',value:{product:'local',protocolVersion:1,dataDirectory:'isolated',mode:'ready',message:window.overviewMessage,legacyPlanCount:window.plan?1:0,pendingLegacyPlanCount:window.plan?1:0,projectCount:0}};
   if(q.type==='plans'||q.type==='projects')return {type:q.type,value:{revision:'v1',offset:0,total:q.type==='plans'&&window.plan?1:0,nextOffset:null,entries:q.type==='plans'&&window.plan?[window.plan]:[]}};
   throw Error('unexpected query');
  }};
 });
 await page.addStyleTag({path:'apps/desktop/src/style.css'});await page.addScriptTag({content:await readFile(join(bundle,'ui.js'),'utf8')});
 const revealErrors=async()=>{await page.getByRole('alert').first().waitFor();for(const button of await page.getByRole('alert').getByRole('button',{name:'技术详情',exact:true}).all())await button.click();};
 assert.equal(await page.getByRole('button',{name:/查看全局.*复核记录/}).count(),0);
 await page.getByText('本地核心已连接',{exact:true}).waitFor();await page.getByText('尚未读取操作记录。请打开“交接记录”查看。',{exact:true}).waitFor();assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request'||c.name==='core_submit').length),0);assert.equal(await page.getByRole('alert').count(),0);cases.push('actual App startup uses only bounded workspace reads; unrequested events are not presented as empty');
 await page.getByRole('heading',{name:'sample-project',exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:/查看项目 .* 的|审阅项目 .* 的执行授权|查看已保存项目配置/}).count(),0);
 assert.equal(await page.evaluate(()=>window.calls.some(c=>c.name==='workspace_read'&&!['overview','plans','projects'].includes(c.args.query.type))),false);
 cases.push('overview shows only project summaries; removed record links neither render nor query backend details');
 await page.evaluate(()=>window.overviewMessage='1 个项目的日报或查询索引更新失败；已提交任务不会自动重发。');
 await page.getByRole('button',{name:'刷新工作区记录',exact:true}).click();
 await revealErrors();
 await page.getByRole('alert').filter({hasText:'日报或查询索引更新失败'}).waitFor();
 assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit').length),0);
 await page.evaluate(()=>window.overviewMessage=null);
 await page.getByRole('button',{name:'刷新工作区记录',exact:true}).click();
 await page.getByRole('alert').filter({hasText:'日报或查询索引更新失败'}).waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit').length),0);
 cases.push('ready mode displays derived-file degradation and clears after repair without command submission');
 await page.getByRole('button',{name:'需求与计划',exact:false}).click();await revealErrors();await page.getByRole('alert').filter({hasText:'responseTooLarge'}).waitFor();await page.getByRole('status').filter({hasText:'不能据此判断没有任务或事件'}).waitFor();await page.locator('details[aria-label="手工计划与历史记录"] > summary').click();await page.getByText('已绑定本地 Markdown 提炼工具。',{exact:false}).waitFor();assert.equal(await page.locator('input[type=file][accept=".md"]').isEnabled(),true);await page.getByLabel('计划名称',{exact:true}).fill('App original');await page.getByLabel('具体任务，每行一项').fill('one local task');await page.getByRole('button',{name:'保存计划',exact:true}).click();
 await page.getByRole('heading',{name:'App original',exact:true}).waitFor();await page.getByRole('alert').filter({hasText:'操作确认已收到'}).waitFor();assert.equal(await page.getByLabel('计划名称',{exact:true}).inputValue(),'');assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit'&&c.args.command.type==='createPlan').length),1);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length),2);cases.push('legacy page overflow is explicit; one create ACK survives separate refresh overflow');
 await page.getByRole('button',{name:'重新读取记录',exact:true}).click();await revealErrors();await page.getByRole('alert').filter({hasText:'responseTooLarge'}).waitFor();assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit'&&c.args.command.type==='createPlan').length),1);assert.equal(await page.getByRole('heading',{name:'App original',exact:true}).count(),1);cases.push('App retry banner only reads; original accepted mutation and independent list are retained');
 await page.evaluate(()=>window.failSubmit=true);await page.getByLabel('计划名称',{exact:true}).fill('Uncertain App input');await page.getByLabel('具体任务，每行一项').fill('preserve me');await page.getByRole('button',{name:'保存计划',exact:true}).click();await page.getByText('原操作待核对，输入已保留。',{exact:true}).waitFor();assert.equal(await page.getByLabel('计划名称',{exact:true}).inputValue(),'Uncertain App input');await page.getByRole('button',{name:'读取原记录核对',exact:true}).click();await page.getByRole('status').filter({hasText:'名称相同本身不能证明'}).waitFor();assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit'&&c.args.command.type==='createPlan').length),2);cases.push('App submit failure freezes original input; narrow readback cannot resend or falsely identify create');
 await page.evaluate(()=>window.fullMode='deferred');await page.getByRole('button',{name:'项目设置',exact:false}).click();await page.waitForFunction(()=>window.deferred.length===1);await page.getByRole('button',{name:'维护与升级',exact:false}).click();await page.getByText('controlled-ui-only',{exact:true}).waitFor();const maintenanceReads=await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length);await page.evaluate(()=>window.deferred[0].resolve({...window.stubSnapshot,events:[{sequence:99,at:'2026-10-14',summary:'STALE PAGE MUST NOT APPEAR'}]}));await page.getByRole('button',{name:'工作总览',exact:false}).click();await page.getByText('尚未读取操作记录。请打开“交接记录”查看。',{exact:true}).waitFor();assert.equal(await page.getByText('STALE PAGE MUST NOT APPEAR',{exact:true}).count(),0);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length),maintenanceReads);cases.push('leaving a pending full view drops its late response; maintenance and overview do not initiate full snapshots');
 await page.evaluate(()=>window.fullMode='active');await page.getByRole('button',{name:'项目设置',exact:false}).click();await page.waitForFunction(n=>window.calls.filter(c=>c.name==='core_request').length>=n+2,maintenanceReads);await page.getByRole('button',{name:'工作总览',exact:false}).click();const afterReturn=await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length);await page.waitForTimeout(1300);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length),afterReturn);cases.push('legacy active polling operates on requested pages and stops when returning to bounded overview');
 await page.evaluate(()=>{window.stubSnapshot.apiJobs=[];});
 const beforeLabs=await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length);
 const navNames=await page.locator('.sidebar nav button').allTextContents();assert(navNames.findIndex(n=>n.includes('独立实验工作台'))+1===navNames.findIndex(n=>n.includes('维护与升级')));
 await page.getByRole('button',{name:'独立实验工作台',exact:false}).click();const toggle=page.getByRole('checkbox',{name:'开启独立实验功能'});assert.equal(await toggle.isChecked(),false);
 assert.equal(await page.getByRole('region',{name:'独立实验工作台管理',exact:true}).count(),0);assert.equal(await page.getByRole('heading',{name:'单执行者效率验收',exact:true}).count(),0);
 assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length),beforeLabs);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='experiment_workbenches').length),0);
 cases.push('experiments have their own navigation item above maintenance, default off without native catalog or snapshot reads');
 await toggle.check();await page.getByRole('region',{name:'独立实验工作台管理',exact:true}).waitFor();await page.getByRole('heading',{name:'单执行者效率验收',exact:true}).waitFor();await page.getByRole('heading',{name:'团队效率验收',exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:'实验工作台',exact:true}).count(),0);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='experiment_workbenches').length),1);
 await page.screenshot({path:join(dir,'experiments-enabled.png'),fullPage:true});
 await toggle.uncheck();assert.equal(await page.getByRole('region',{name:'独立实验工作台管理',exact:true}).count(),0);assert.equal(await page.getByRole('heading',{name:'单执行者效率验收',exact:true}).count(),0);
 const afterLabs=await page.evaluate(()=>window.calls.length);await page.waitForTimeout(1200);assert.equal(await page.evaluate(()=>window.calls.length),afterLabs);
 assert.equal(await page.evaluate(()=>window.calls.some(c=>['create_experiment_workbench','open_experiment_workbench','close_workbench'].includes(c.name))),false);
 cases.push('opt-in reveals inline experiment management and both efficiency tools; disabling unmounts them without starting or stopping experiments');
 await page.evaluate(()=>{window.stubSnapshot.managementDecisions=[];window.stubSnapshot.managementHistory={revision:'management-fixture',projects:[],total:0,offset:0,nextOffset:null,projectId:null};});
 await page.getByRole('button',{name:'需求与计划',exact:false}).click();
 const statistics=page.locator('details.project-data-statistics'),summary=statistics.locator(':scope > summary');
 await summary.waitFor();assert.equal(await statistics.evaluate(e=>e.open),false);
 const statTitles=['项目执行方案','历史费用来源','已确认历史金额','计数费用与续发核对','API 计数费用核对','API 生成费用核对'];
 for(const title of statTitles)assert.equal(await statistics.getByRole('heading',{name:title,exact:true,includeHidden:true}).count(),1);
 assert(await statistics.getByRole('button',{name:'读取计数清单',exact:true,includeHidden:true}).isHidden());
 assert.deepEqual(await statistics.evaluate(e=>[e.previousElementSibling.getAttribute('aria-label'),e.nextElementSibling.getAttribute('aria-label')]),['项目盲评','主副指挥请求记录']);
 await page.setViewportSize({width:1280,height:960});await statistics.screenshot({path:join(dir,'statistics-collapsed.png')});
 const beforeExpand=await page.evaluate(()=>window.calls.length);await summary.click();assert(await statistics.evaluate(e=>e.open));
 assert.equal(await page.evaluate(()=>window.calls.length),beforeExpand);
 for(const title of statTitles)assert(await statistics.getByRole('heading',{name:title,exact:true}).isVisible());
 cases.push('All six statistics sections share one default-collapsed group between blind review and management history; expanding makes no request');
 for(const name of ['读取计数清单','读取 API 计数清单','读取 API 生成清单'])await statistics.getByRole('button',{name,exact:true}).click();
 for(const button of await statistics.getByRole('button',{name:'刷新首页',exact:true}).all())await button.click();
 const reads=await page.evaluate(()=>window.calls.filter(c=>['count_resolution_request','historical_request'].includes(c.name)).map(c=>c.args.command.type));
 assert.deepEqual(reads,['readManagementCounts','readApiCounts','readApiGenerations','readHistoricalSources','readHistoricalAccounting']);
 const readCount=reads.length;await summary.click();await summary.click();
 assert.equal(await page.evaluate(()=>window.calls.filter(c=>['count_resolution_request','historical_request'].includes(c.name)).length),readCount);
 assert.equal(await statistics.getByText('共 0 条；当前从第 0 条显示。',{exact:true}).count(),3);
 cases.push('Expanded panels retain original read commands and results across collapse/reopen; no billing or workflow mutation');
 for(const width of [940,640]){await page.setViewportSize({width,height:960});assert(await statistics.evaluate(e=>e.scrollWidth<=e.clientWidth+1));await statistics.screenshot({path:join(dir,`statistics-open-${width}.png`)});}
 await page.getByRole('button',{name:'工作总览',exact:false}).click();await page.getByRole('button',{name:'需求与计划',exact:false}).click();await summary.waitFor();assert.equal(await statistics.evaluate(e=>e.open),false);
 cases.push('Statistics fit desktop/narrow widths and return to collapsed on a new page visit');
 assert.equal(await page.locator('#planning-review-tool,#planning-functional-chief,#workbench-checker,#workbench-verifier').count(),0);
 await page.getByRole('button',{name:'项目设置',exact:false}).click();
 const tools=page.getByRole('region',{name:'固定检查与验收工具',exact:true});await tools.waitFor();
 assert.equal(await tools.locator('#workbench-checker').evaluate(e=>e.readOnly),true);
 assert.equal(await tools.locator('#workbench-verifier').evaluate(e=>e.readOnly),true);
 assert.equal(await tools.getByRole('button',{name:'重新核对绑定',exact:true}).count(),2);
 cases.push('Planning contains neither duplicated binding card; project settings keep exactly one read-only checker and one verifier');
 await page.setViewportSize({width:1280,height:900});
 const sidebar=page.locator('.sidebar');
 assert.equal(await page.locator('.shell').getAttribute('data-workbench'),'daily');
 assert.equal(await sidebar.evaluate(e=>getComputedStyle(e).backgroundColor),'rgb(231, 239, 243)');
 await sidebar.locator('.workspace').filter({hasText:'日常工作台'}).waitFor();
 await sidebar.screenshot({path:join(dir,'sidebar-daily.png')});
 await page.getByRole('button',{name:'独立实验工作台',exact:false}).click();
 assert.equal(await page.locator('.shell').getAttribute('data-workbench'),'daily');
 cases.push('Daily sidebar is light and remains daily when visiting the experiment management page');
 await page.evaluate(()=>window.__TAURI_INTERNALS__.metadata.currentWindow.label='experiment-exp-flow-20261106');
 await page.getByRole('button',{name:'工作总览',exact:false}).click();
 assert.equal(await page.locator('.shell').getAttribute('data-workbench'),'experiment');
 await sidebar.locator('.workspace').filter({hasText:'独立实验台'}).waitFor();
 await sidebar.getByText('exp-flow-20261106',{exact:true}).waitFor();
 assert.equal(await sidebar.evaluate(e=>getComputedStyle(e).backgroundColor),'rgb(17, 30, 43)');
 await sidebar.screenshot({path:join(dir,'sidebar-experiment.png')});
 for(const width of [1280,940,640]) {
  await page.setViewportSize({width,height:900});
  assert(await sidebar.evaluate(e=>e.scrollWidth<=e.clientWidth));
  const contrast=await sidebar.evaluate(e=>{
   const luminance=value=>{const rgb=value.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;});return .2126*rgb[0]+.7152*rgb[1]+.0722*rgb[2];};
   const bg=luminance(getComputedStyle(e).backgroundColor);
   return ['.nav-label','.sidebar-footer small','nav button','.workbench-exit-trigger'].map(selector=>{const fg=luminance(getComputedStyle(e.querySelector(selector)).color);return (Math.max(bg,fg)+.05)/(Math.min(bg,fg)+.05);});
  });
  assert(contrast.every(value=>value>=4.5));
 }
 cases.push('Experiment identity follows the bound window; dark sidebar text and footer meet contrast checks at three widths');
 await page.evaluate(()=>window.__TAURI_INTERNALS__.metadata.currentWindow.label='main');
 await page.getByRole('button',{name:'项目设置',exact:false}).click();
 assert.equal(await page.locator('.shell').getAttribute('data-workbench'),'daily');
 cases.push('Returning to a daily window restores its light palette independently of page and core data');
 assert.deepEqual(errors,[]);assert.equal(networkRequests,0);assert.deepEqual(await hashes(),before);await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,sourceBefore:before,sourceAfter:await hashes(),pageErrors:errors,networkRequests,modelCalls:0,scope:'Actual production App with mocked Tauri invoke, not installed native integration',calls:await page.evaluate(()=>window.calls)},null,2));console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
}catch(e){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,cases,errors},null,2));throw e;}finally{await browser?.close();}
