import {mkdtemp,readFile,writeFile} from 'node:fs/promises';import {join} from 'node:path';import {createHash} from 'node:crypto';import assert from 'node:assert/strict';import {build} from 'vite';import {chromium} from 'playwright-core';
if(process.env.CERULEAN_NATIVE_OWNED_JOB!=='1')throw Error('Use owned runner');
const dir=await mkdtemp('reports/workspace-app-ui-151-'),cases=[],errors=[];let browser,page,networkRequests=0;
const files=['App.tsx','WorkspaceBrowser.tsx','commandSubmission.ts','contracts.ts'].map(n=>'apps/desktop/src/'+n);const hashes=async()=>Object.fromEntries(await Promise.all(files.map(async n=>[n,createHash('sha256').update(await readFile(n)).digest('hex')])));const before=await hashes();
const entry=join(dir,'entry.tsx');await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {App} from '../../apps/desktop/src/App';createRoot(document.getElementById('root')!).render(<App/>);`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'WorkspaceApp',formats:['iife'],fileName:()=> 'ui.js'}}});
try{
 browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});page=await browser.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));await page.route('**/*',r=>{if(r.request().url()==='http://localhost/workspace-ui-fixture')return r.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><div id="root"></div></html>'});networkRequests++;return r.abort();});await page.goto('http://localhost/workspace-ui-fixture');assert.equal(await page.evaluate(()=>window.isSecureContext&&typeof crypto.randomUUID==='function'),true);
 await page.evaluate(()=>{
  window.calls=[];window.overviewMessage=null;window.plan=null;window.failSubmit=false;window.fullMode='error';window.deferred=[];
  window.stubSnapshot={dataDirectory:'isolated',events:[],validators:[],managementHistory:{projects:[]},verificationJobs:[],apiJobs:[{state:'executing'}]};
  window.__TAURI_INTERNALS__={invoke:async(name,args)=>{
   window.calls.push({name,args});
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
   if(q.type==='overview')return {type:'overview',value:{product:'local',protocolVersion:1,dataDirectory:'isolated',mode:'ready',message:window.overviewMessage,legacyPlanCount:window.plan?1:0,pendingLegacyPlanCount:window.plan?1:0,projectCount:0}};
   if(q.type==='plans'||q.type==='projects')return {type:q.type,value:{revision:'v1',offset:0,total:q.type==='plans'&&window.plan?1:0,nextOffset:null,entries:q.type==='plans'&&window.plan?[window.plan]:[]}};
   throw Error('unexpected query');
  }};
 });
 await page.addScriptTag({content:await readFile(join(dir,'bundle/ui.js'),'utf8')});
 await page.getByText('本地核心已连接',{exact:true}).waitFor();await page.getByText('尚未读取操作记录。请打开“交接记录”查看。',{exact:true}).waitFor();assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request'||c.name==='core_submit').length),0);assert.equal(await page.getByRole('alert').count(),0);cases.push('actual App startup uses only bounded workspace reads; unrequested events are not presented as empty');
 await page.evaluate(()=>window.overviewMessage='1 个项目的日报或查询索引更新失败；已提交任务不会自动重发。');
 await page.getByRole('button',{name:'刷新工作区记录',exact:true}).click();
 await page.getByRole('alert').filter({hasText:'日报或查询索引更新失败'}).waitFor();
 assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit').length),0);
 await page.evaluate(()=>window.overviewMessage=null);
 await page.getByRole('button',{name:'刷新工作区记录',exact:true}).click();
 await page.getByRole('alert').filter({hasText:'日报或查询索引更新失败'}).waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit').length),0);
 cases.push('ready mode displays derived-file degradation and clears after repair without command submission');
 await page.getByRole('button',{name:'＋ 新建计划',exact:true}).click();await page.getByRole('alert').filter({hasText:'responseTooLarge'}).waitFor();await page.getByRole('status').filter({hasText:'不能据此判断没有任务或事件'}).waitFor();await page.getByLabel('计划名称',{exact:true}).fill('App original');await page.getByLabel('具体任务，每行一项').fill('one local task');await page.getByRole('button',{name:'保存计划',exact:true}).click();
 await page.getByRole('heading',{name:'App original',exact:true}).waitFor();await page.getByRole('alert').filter({hasText:'操作确认已收到'}).waitFor();assert.equal(await page.getByLabel('计划名称',{exact:true}).inputValue(),'');assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit'&&c.args.command.type==='createPlan').length),1);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length),2);cases.push('legacy page overflow is explicit; one create ACK survives separate refresh overflow');
 await page.getByRole('button',{name:'重新读取记录',exact:true}).click();await page.getByRole('alert').filter({hasText:'responseTooLarge'}).waitFor();assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit'&&c.args.command.type==='createPlan').length),1);assert.equal(await page.getByRole('heading',{name:'App original',exact:true}).count(),1);cases.push('App retry banner only reads; original accepted mutation and independent list are retained');
 await page.evaluate(()=>window.failSubmit=true);await page.getByLabel('计划名称',{exact:true}).fill('Uncertain App input');await page.getByLabel('具体任务，每行一项').fill('preserve me');await page.getByRole('button',{name:'保存计划',exact:true}).click();await page.getByText('原操作待核对，输入已保留。',{exact:true}).waitFor();assert.equal(await page.getByLabel('计划名称',{exact:true}).inputValue(),'Uncertain App input');await page.getByRole('button',{name:'读取原记录核对',exact:true}).click();await page.getByRole('status').filter({hasText:'名称相同本身不能证明'}).waitFor();assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_submit'&&c.args.command.type==='createPlan').length),2);cases.push('App submit failure freezes original input; narrow readback cannot resend or falsely identify create');
 await page.evaluate(()=>window.fullMode='deferred');await page.getByRole('button',{name:'项目设置',exact:false}).click();await page.waitForFunction(()=>window.deferred.length===1);await page.getByRole('button',{name:'维护与升级',exact:false}).click();await page.getByText('controlled-ui-only',{exact:true}).waitFor();const maintenanceReads=await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length);await page.evaluate(()=>window.deferred[0].resolve({...window.stubSnapshot,events:[{sequence:99,at:'2026-10-14',summary:'STALE PAGE MUST NOT APPEAR'}]}));await page.getByRole('button',{name:'工作总览',exact:false}).click();await page.getByText('尚未读取操作记录。请打开“交接记录”查看。',{exact:true}).waitFor();assert.equal(await page.getByText('STALE PAGE MUST NOT APPEAR',{exact:true}).count(),0);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length),maintenanceReads);cases.push('leaving a pending full view drops its late response; maintenance and overview do not initiate full snapshots');
 await page.evaluate(()=>window.fullMode='active');await page.getByRole('button',{name:'项目设置',exact:false}).click();await page.waitForFunction(n=>window.calls.filter(c=>c.name==='core_request').length>=n+2,maintenanceReads);await page.getByRole('button',{name:'工作总览',exact:false}).click();const afterReturn=await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length);await page.waitForTimeout(1300);assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.name==='core_request').length),afterReturn);cases.push('legacy active polling operates on requested pages and stops when returning to bounded overview');
 assert.deepEqual(errors,[]);assert.equal(networkRequests,0);assert.deepEqual(await hashes(),before);await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',cases,sourceBefore:before,sourceAfter:await hashes(),pageErrors:errors,networkRequests,modelCalls:0,scope:'Actual production App with mocked Tauri invoke, not installed native integration',calls:await page.evaluate(()=>window.calls)},null,2));console.log(JSON.stringify({status:'passed',cases:cases.length,artifact:dir}));
}catch(e){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,cases,errors},null,2));throw e;}finally{await browser?.close();}
