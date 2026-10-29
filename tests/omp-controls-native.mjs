// Isolated Tauri and named-pipe controls. Offline worker and synthetic journals only.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, readFile, writeFile, readdir, rename } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';
import { assertNativeRuntime } from './native-runtime.mjs';
await assertNativeRuntime();
const parent=resolve(process.env.CERULEAN_OMP_TEST_PARENT);
const reportDir=await mkdtemp(resolve('reports/omp-controls-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const testExe=build.stdout.split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(testExe);
const runtime=resolve('target/debug/cerulean-core.exe'),desktop=resolve('target/debug/cerulean-desktop.exe');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));const results=[];
async function until(fn,label,limit=160){for(let i=0;i<limit;i++){const value=await fn();if(value)return value;await sleep(100);}throw Error('Timed out: '+label);}
for(const mode of ['cancel','startup','prepared','unsent','recover']){
 const dir=await mkdtemp(join(parent,'native-controls-')),data=join(dir,'data');let core,app,browser,page,fixture;const errors=[];let coreOutput='';let startupBytes,managementPending;
 const env={...process.env,CERULEAN_OMP_CONTROL_DIR:dir,CERULEAN_OMP_CONTROL_MODE:mode};
 const args=['--exact','host::omp_controls_native::native_omp_controls_fixture','--ignored','--nocapture'];
 function request(command){return page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);}
 const snapshot=()=>request({type:'snapshot'});
 async function stop(child){if(!child||child.exitCode!==null)return;const exited=once(child,'exit');child.kill();await Promise.race([exited,new Promise((_,reject)=>setTimeout(()=>reject(Error('owned process exit timeout')),8000).unref())]);}
 async function startCore(){core=spawn(runtime,['--serve','--data-dir',data],{windowsHide:true,stdio:['ignore','pipe','pipe']});core.stdout.resume();core.stderr.on('data',b=>{coreOutput+=b;});await ready();}
 async function ready(){await until(()=>{assert.equal(core.exitCode,null,coreOutput);const r=spawnSync(runtime,['--request','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:2000,input:JSON.stringify({id:'ready',protocolVersion:1,command:{type:'snapshot'}})});return r.status===0;},'service');}
 async function launch(){const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));app=spawn(desktop,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(dir,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
 await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
 page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});await page.getByRole('button',{name:'需求与计划',exact:false}).click();await panel().waitFor();}
 const panel=()=>page.getByRole('region',{name:'OMP 执行记录',exact:true});
 async function close(){if(app&&app.exitCode===null){const exited=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exited,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);}await browser?.close();app=null;browser=null;page=null;await stop(core);core=null;}
 const eventCount=async()=> (await readdir(join(data,'workflows/project/events'))).length;
 try{
  if(mode==='cancel'){core=spawn(testExe,args,{windowsHide:true,stdio:['ignore','pipe','pipe'],env});core.stdout.on('data',b=>{coreOutput+=b;});core.stderr.on('data',b=>{coreOutput+=b;});fixture=await until(async()=>{assert.equal(core.exitCode,null,coreOutput);try{return JSON.parse(await readFile(join(dir,'fixture.json'),'utf8'));}catch{return null;}},'running fixture');assert.equal(fixture.ownerPid,core.pid);await ready();}
  else{const r=spawnSync(testExe,args,{windowsHide:true,encoding:'utf8',timeout:30000,env});coreOutput=r.stdout+r.stderr;assert.equal(r.status,0,coreOutput);fixture=JSON.parse(await readFile(join(dir,'fixture.json'),'utf8'));
   if(process.env.CERULEAN_MANAGEMENT_CONTROL_PENDING==='1'&&['startup','prepared','unsent'].includes(mode)){
    assert.match(fixture.operation,/^[a-zA-Z0-9_-]+$/);
    const sequence={startup:1,prepared:2,unsent:3}[mode];
    const target=join(data,'management-decisions',`${fixture.operation}.${sequence}.json`),pending=target.replace(/\.json$/,'.pending');
    const bytes=await readFile(target);await assert.rejects(readFile(pending),{code:'ENOENT'});await rename(target,pending);
    managementPending={target,pending,bytes,sequence};await assert.rejects(readFile(target),{code:'ENOENT'});
   }
   if(mode==='startup'&&process.env.CERULEAN_OMP_CONTROL_STARTUP_PENDING==='1'){
    startupBytes=await readFile(join(data,'omp-startups/project/attempt.0.pending'));
    await assert.rejects(readFile(join(data,'omp-startups/project/attempt.0.json')),{code:'ENOENT'});
   }
   await startCore();
   if(managementPending){assert.deepEqual(await readFile(managementPending.target),managementPending.bytes);await assert.rejects(readFile(managementPending.pending),{code:'ENOENT'});}
   if(startupBytes){assert.deepEqual(await readFile(join(data,'omp-startups/project/attempt.0.json')),startupBytes);await assert.rejects(readFile(join(data,'omp-startups/project/attempt.0.pending')),{code:'ENOENT'});}
  }
  await launch();const before=await snapshot();const beforeEvents=await eventCount();
  if(mode==='prepared'){
   await panel().getByRole('button',{name:'查看分配决定',exact:true}).click();const detail=page.getByRole('article',{name:'请求完整记录'});await detail.waitFor();assert.equal(await detail.getAttribute('data-management-detail'),fixture.operation);
   await detail.getByRole('button',{name:'关闭未提交应用…',exact:true}).click();const dialog=page.getByRole('dialog',{name:'确认关闭未提交应用',exact:true});await dialog.waitFor();await dialog.getByRole('button',{name:'返回',exact:true}).click();assert.equal(await eventCount(),beforeEvents);
   await detail.getByRole('button',{name:'关闭未提交应用…',exact:true}).click();await dialog.locator('#management-review-input').fill('关闭尚未提交的 OMP 分配准备');await dialog.screenshot({path:join(reportDir,mode+'-review.png')});await dialog.getByRole('button',{name:'确认关闭未提交应用',exact:true}).click();await dialog.waitFor({state:'hidden'});
  }else if(mode==='recover'){
   await panel().getByRole('button',{name:'核对原始执行证据',exact:true}).click();
  }else{
   const action=mode==='cancel'?'停止当前 OMP 任务…':mode==='startup'?'关闭未分配启动…':'放弃未派发尝试…';const title=mode==='cancel'?'确认停止 OMP 任务':mode==='startup'?'确认关闭 OMP 启动':'确认放弃 OMP 未派发尝试';
   await panel().getByRole('button',{name:action,exact:true}).click();let dialog=page.getByRole('dialog',{name:title,exact:true});assert.equal(await dialog.getByRole('button',{name:'确认处置',exact:true}).isDisabled(),true);await dialog.getByRole('button',{name:'返回检查',exact:true}).click();assert.equal(await eventCount(),beforeEvents);
   await panel().getByRole('button',{name:action,exact:true}).click();dialog=page.getByRole('dialog',{name:title,exact:true});await dialog.locator('#omp-control-reason').fill('核对目标后处置本次 OMP 尝试');await dialog.getByRole('checkbox',{name:'我已核对目标和处置影响'}).check();await dialog.screenshot({path:join(reportDir,mode+'-review.png')});
   if(mode==='startup'||mode==='unsent'){
    const invalid=mode==='startup'?{type:'abandonOmpStartup',projectId:fixture.projectId,attemptId:fixture.attemptId,evidenceDigest:'0'.repeat(64),reason:'stale review must fail'}:{type:'abandonUnsentAttempt',projectId:fixture.projectId,taskId:fixture.taskId,attemptId:fixture.attemptId,evidenceDigest:'0'.repeat(64),reason:'stale review must fail'};
    const rejected=await page.evaluate(async command=>{try{await window.__TAURI_INTERNALS__.invoke('core_request',{command});return false;}catch{return true;}},invalid);assert.equal(rejected,true);assert.equal(await eventCount(),beforeEvents);
   }
   await dialog.getByRole('button',{name:'确认处置',exact:true}).evaluate(button=>{button.click();button.click();});await dialog.waitFor({state:'hidden'});
  }
  const expected=['startup','prepared'].includes(mode)?'abandoned':'cancelled';
  const after=await until(async()=>{const s=await snapshot();return s.ompJobs[0].state===expected?s:null;},'terminal state');
  if(mode==='cancel'){assert.equal(after.projectRuns[0].state,'paused');assert.equal(after.workflows[0].state.tasks.b.state,'pending');assert.equal(await readFile(join(dir,'project/result.txt'),'utf8'),'fixture output');}
  if(['startup','prepared','unsent'].includes(mode))assert.equal(after.workflows[0].state.tasks[fixture.taskId].state,'pending');
  if(['startup','prepared'].includes(mode))assert.equal(await eventCount(),beforeEvents);
  if(mode==='recover')assert.equal(after.workflows[0].state.tasks[fixture.taskId].state,'cancelled');
  await panel().getByText(expected==='abandoned'?'本次尝试已关闭':'本次尝试已结束',{exact:true}).waitFor({timeout:15000});
  await panel().screenshot({path:join(reportDir,mode+'-finished.png')});const count=await eventCount();const managementCount=after.managementHistory.projects[0].total;
  await close();await startCore();await launch();const reopened=await snapshot();assert.equal(reopened.ompJobs[0].state,expected);assert.equal(await eventCount(),count);assert.equal(reopened.managementHistory.projects[0].total,managementCount);assert.deepEqual(errors,[]);await close();
  if(managementPending){assert.deepEqual(await readFile(managementPending.target),managementPending.bytes);await assert.rejects(readFile(managementPending.pending),{code:'ENOENT'});}
  await writeFile(join(reportDir,mode+'-core.log'),coreOutput);results.push({mode,dir,status:'passed',beforeEvents,afterEvents:count,fullCoreRestarts:1,terminal:expected,terminalRendered:true,duplicateClickGuard:mode!=='prepared'&&mode!=='recover',staleDigestRejected:mode==='startup'||mode==='unsent',startupPendingRecovered:!!startupBytes,managementPendingRecovered:!!managementPending,managementPendingSequence:managementPending?.sequence,realModelCalls:0});
 }catch(error){await page?.screenshot({path:join(reportDir,mode+'-failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(reportDir,mode+'-failure.json'),JSON.stringify({error:String(error),dir,coreOutput,errors}));throw error;}
 finally{await browser?.close().catch(()=>{});await stop(app);await stop(core);}
}
await writeFile(join(reportDir,'result.json'),JSON.stringify({at:new Date().toISOString(),status:'passed',realModelCalls:0,results},null,2));console.log(JSON.stringify({reportDir,status:'passed',scenarios:results.length}));
