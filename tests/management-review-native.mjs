// Five actual Tauri review workflows; review preserves original evidence and workflow.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, readFile, writeFile, readdir } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';
import { assertNativeRuntime } from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/management-review-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const testExe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(testExe);
const core=resolve('target/debug/cerulean-core.exe'),desktop=resolve('target/debug/cerulean-desktop.exe');
const results=[],sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let n=0;n<180;n++){const v=await fn();if(v)return v;await sleep(100);}throw Error('Timed out: '+label);}
for(let stage=0;stage<=4;stage++){
 const dir=await mkdtemp(join(report,`stage-${stage}-`));
 const seeded=spawnSync(testExe,['--exact','host::omp_management_bridge::tests::review_tests::native_management_review_fixture','--ignored','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:30000,env:{...process.env,CERULEAN_MANAGEMENT_REVIEW_DIR:dir,CERULEAN_MANAGEMENT_REVIEW_STAGE:String(stage)}});
 assert.equal(seeded.status,0,seeded.stdout+seeded.stderr);
 const fixture=JSON.parse(await readFile(join(dir,'fixture.json'),'utf8')),data=fixture.data;
 const original=await readFile(join(data,fixture.relativePath));let app,browser,page;const errors=[];
 const eventBytes=async()=>{const folder=join(data,'workflows/project/events');const names=(await readdir(folder)).sort();return Promise.all(names.map(async name=>[name,(await readFile(join(folder,name))).toString('base64')]));};
 const beforeEvents=await eventBytes();
 const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
 async function launch(){
  const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
  app=spawn(desktop,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(dir,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
  page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
 }
 async function close(){
  const exited=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await Promise.race([exited,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);
  await browser.close();browser=null;app=null;page=null;
  const r=spawnSync(core,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(r.status,0,r.stderr);
 }
 try{
  await launch();const region=page.getByRole('region',{name:'执行数据诊断',exact:true});await region.waitFor();
  const before=await request({type:'snapshot'}),file=before.coreDiagnostic.managementFiles.find(f=>f.relativePath===fixture.relativePath);assert.ok(file.reviewDigest);assert.equal(file.reviewKind,'management');
  const row=region.getByRole('article').filter({has:page.getByRole('heading',{name:fixture.relativePath,exact:true})});
  await row.getByRole('button',{name:'复核半写管理记录…',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'确认复核半写管理记录',exact:true});await dialog.waitFor();
  assert.equal(await dialog.getByRole('button',{name:'保存管理复核',exact:true}).isDisabled(),true);
  await dialog.getByRole('button',{name:'返回检查',exact:true}).click();
  assert.equal((await request({type:'snapshot'})).coreDiagnostic.managementFiles.find(f=>f.relativePath===fixture.relativePath).reviewed,false);
  const stale=await page.evaluate(async command=>{try{await window.__TAURI_INTERNALS__.invoke('core_request',{command});return false;}catch{return true;}},{type:'reviewManagementRecord',relativePath:fixture.relativePath,evidenceDigest:'0'.repeat(64),reason:'stale'});assert.equal(stale,true);
  await row.getByRole('button',{name:'复核半写管理记录…',exact:true}).click();
  await dialog.locator('#management-record-review-reason').fill(`已核对阶段 ${stage} 的原文和前置链，保留残片并按原流程处置`);await dialog.getByRole('checkbox').check();
  await dialog.screenshot({path:join(report,`stage-${stage}-review.png`)});
  await dialog.getByRole('button',{name:'保存管理复核',exact:true}).evaluate(b=>{b.click();b.click();});await dialog.waitFor({state:'hidden'});
  assert.equal((await readdir(join(data,'management-record-reviews'))).filter(n=>n.endsWith('.json')).length,1);
  assert.deepEqual(await readFile(join(data,fixture.relativePath)),original);assert.ok((await request({type:'snapshot'})).coreDiagnostic);
  await region.getByRole('button',{name:'重新启动并校验',exact:true}).click();await region.waitFor({state:'hidden',timeout:45000});
  let healthy=await request({type:'snapshot'});assert.equal(healthy.coreDiagnostic,null);assert.equal(healthy.managementRecordReviews.length,1);
  assert.equal(healthy.workflows[0].state.tasks.task.state,stage===3?'assigned':'pending');assert.deepEqual(await eventBytes(),beforeEvents);
  if(stage===0)assert.equal(healthy.managementDecisions.length,0);
  else assert.equal(healthy.managementDecisions.find(d=>d.operationId===fixture.operation).state,stage===1?'unknown':stage===3?'applied':'ready');
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
  if(stage===1||stage===4){
   const history=page.getByRole('region',{name:'主副指挥请求记录',exact:true});
   await history.getByRole('button',{name:'放弃本地请求…',exact:true}).click();
   const second=page.getByRole('dialog',{name:'确认放弃本地指挥请求',exact:true});await second.waitFor();
   await second.locator('#management-review-input').fill('重新核对后明确放弃原本地请求');await second.getByRole('button',{name:'确认放弃本地请求',exact:true}).click();await second.waitFor({state:'hidden'});
  }else if(stage===2){
   const panel=page.getByRole('region',{name:'OMP 执行记录',exact:true});await panel.getByRole('button',{name:'关闭未分配启动…',exact:true}).click();
   const second=page.getByRole('dialog',{name:'确认关闭 OMP 启动',exact:true});await second.waitFor();await second.locator('#omp-control-reason').fill('重新核对后关闭未分配启动');await second.getByRole('checkbox').check();await second.getByRole('button',{name:'确认处置',exact:true}).click();await second.waitFor({state:'hidden'});
  }else if(stage===3){
   const panel=page.getByRole('region',{name:'OMP 执行记录',exact:true});await panel.getByRole('button',{name:'放弃未派发尝试…',exact:true}).click();
   const second=page.getByRole('dialog',{name:'确认放弃 OMP 未派发尝试',exact:true});await second.waitFor();await second.locator('#omp-control-reason').fill('回执核对完成，明确放弃未派发尝试');await second.getByRole('checkbox',{name:'我已核对目标和处置影响'}).check();await second.getByRole('button',{name:'确认处置',exact:true}).click();await second.waitFor({state:'hidden'});
  }
  healthy=await request({type:'snapshot'});if(stage===1||stage===4)assert.equal(healthy.managementDecisions.find(d=>d.operationId===fixture.operation).state,'discarded');
  if(stage===2)assert.equal(healthy.ompJobs[0].state,'abandoned');
  if(stage===3)assert.equal(healthy.ompJobs[0].state,'cancelled');
  const afterEvents=await eventBytes();
  if(stage===3){
   assert.equal(afterEvents.length,beforeEvents.length+1);assert.deepEqual(afterEvents.slice(0,-1),beforeEvents);
   const added=JSON.parse(Buffer.from(afterEvents.at(-1)[1],'base64').toString('utf8'));assert.equal(added.body.event.command.kind,'abandon-unsent');
  }else assert.deepEqual(afterEvents,beforeEvents);
  await page.screenshot({path:join(report,`stage-${stage}-healthy.png`),fullPage:true});
  await close();await launch();const reopened=await request({type:'snapshot'});assert.equal(reopened.coreDiagnostic,null);assert.equal(reopened.managementRecordReviews.length,1);
  assert.deepEqual(await readFile(join(data,fixture.relativePath)),original);assert.deepEqual(await eventBytes(),afterEvents);assert.equal(reopened.workflows[0].state.tasks.task.state,'pending');
  if(stage===1||stage===4)assert.equal(reopened.managementDecisions.find(d=>d.operationId===fixture.operation).state,'discarded');
  if(stage===2)assert.equal(reopened.ompJobs[0].state,'abandoned');
  if(stage===3)assert.equal(reopened.ompJobs[0].state,'cancelled');
  assert.deepEqual(errors,[]);await close();
  results.push({stage,status:'passed',data,originalBytesPreserved:true,workflowUnchangedByReview:true,userAbandonUnsentRecorded:stage===3,workflowEventsAddedBySeparateDisposition:afterEvents.length-beforeEvents.length,workflowStableAcrossSecondRestart:true,fullCoreRestarts:2,cancelDialogPreserved:true,staleDigestRejected:true,doubleClickGuard:true,separateUserDisposition:[1,2,3,4].includes(stage),realModelCalls:0});
 }catch(error){await page?.screenshot({path:join(report,`stage-${stage}-failure.png`),fullPage:true}).catch(()=>{});await writeFile(join(report,`stage-${stage}-failure.json`),JSON.stringify({error:String(error),errors,data}));throw error;}
 finally{await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exited=once(app,'exit');app.kill();await exited;}}
}
await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',at:new Date().toISOString(),realModelCalls:0,results},null,2));console.log(JSON.stringify({report,status:'passed',scenarios:results.length}));
