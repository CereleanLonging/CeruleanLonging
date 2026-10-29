// Actual Tauri, service fallback and two whole-core rechecks. No model calls.
// User review retains the injected evidence and permanently reserves its ID.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, mkdir, readFile, writeFile, readdir } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';
import { assertNativeRuntime } from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','requires owned process wrapper');
await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/data-diagnostic-native-')),data=join(report,'data');
const damaged=join(data,'omp-startups/project/attempt.0.pending'),original=Buffer.from('{"schemaVersion":');
await mkdir(join(data,'omp-startups/project'),{recursive:true});await writeFile(damaged,original);
const core=resolve('target/debug/cerulean-core.exe'),desktop=resolve('target/debug/cerulean-desktop.exe');
let app,browser,page;const errors=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let i=0;i<180;i++){const result=await fn();if(result)return result;await sleep(100);}throw Error('Timed out: '+label);}
async function request(command){return page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);}
async function invokeRejected(name,args){return page.evaluate(async({name,args})=>{try{await window.__TAURI_INTERNALS__.invoke(name,args);return false;}catch{return true;}},{name,args});}
try {
 const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
 app=spawn(desktop,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
 await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
 page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
 await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
 const region=page.getByRole('region',{name:'执行数据诊断',exact:true});await region.waitFor();
 const before=await request({type:'snapshot'});assert.equal(before.coreDiagnostic.startupFiles[0].state,'truncated-json');assert.equal(before.workflows.length,0);
 for(const command of [{type:'createPlan',title:'blocked',tasks:['blocked']},{type:'checkOpenAi'}])assert.equal(await invokeRejected('core_request',{command}),true);
 assert.equal(await invokeRejected('restart_reviewed_stops'),true);
 for(const name of ['工作总览','需求与计划','接入管理','交接记录','项目设置']){
  await page.getByRole('button',{name,exact:false}).click();await region.waitFor();
  assert.equal(await page.getByRole('button',{name:'＋ 新建计划',exact:true}).count(),0);
 }
 await region.getByRole('button',{name:'刷新诊断',exact:true}).click();
 await until(async()=>!(await region.getByRole('button',{name:'重新启动并校验',exact:true}).isDisabled()),'refresh completed');
 await region.screenshot({path:join(report,'diagnostic.png')});
 await region.getByRole('button',{name:'重新启动并校验',exact:true}).click();
 await until(async()=>{try{return (await request({type:'snapshot'})).coreDiagnostic?.detectedAt!==before.coreDiagnostic.detectedAt;}catch{return false;}},'new diagnostic host');
 await until(async()=>!(await region.getByRole('button',{name:'重新启动并校验',exact:true}).isDisabled()),'recheck completed');
 assert.deepEqual(await readFile(damaged),original);assert.ok((await request({type:'snapshot'})).coreDiagnostic);
 const file=(await request({type:'snapshot'})).coreDiagnostic.startupFiles[0];assert.ok(file.reviewDigest);
 await region.getByRole('button',{name:'复核半写启动意图…',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'确认复核半写启动意图',exact:true});await dialog.waitFor();
 assert.equal(await dialog.getByRole('button',{name:'保存启动复核',exact:true}).isDisabled(),true);
 await dialog.getByRole('button',{name:'返回检查',exact:true}).click();assert.equal((await request({type:'snapshot'})).coreDiagnostic.startupFiles[0].reviewed,false);
 assert.equal(await invokeRejected('core_request',{command:{type:'reviewStartupIntent',relativePath:file.relativePath,evidenceDigest:'0'.repeat(64),reason:'stale'}}),true);
 await region.getByRole('button',{name:'复核半写启动意图…',exact:true}).click();await dialog.locator('#startup-intent-review-reason').fill('已核对半写意图，保留原文并禁止复用旧尝试编号');await dialog.getByRole('checkbox').check();
 await dialog.screenshot({path:join(report,'review.png')});
 await dialog.getByRole('button',{name:'保存启动复核',exact:true}).evaluate(b=>{b.click();b.click();});await dialog.waitFor({state:'hidden'});
 assert.equal((await request({type:'snapshot'})).coreDiagnostic.startupFiles[0].reviewed,true);assert.deepEqual(await readFile(damaged),original);
 assert.equal((await readdir(join(data,'startup-intent-reviews'))).filter(n=>n.endsWith('.json')).length,1);
 await region.screenshot({path:join(report,'reviewed.png')});
 await region.getByRole('button',{name:'重新启动并校验',exact:true}).click();await region.waitFor({state:'hidden',timeout:45000});
 const healthy=await request({type:'snapshot'});assert.equal(healthy.coreDiagnostic,null);assert.equal(healthy.events.length,0);assert.equal(healthy.workflows.length,0);
 assert.equal(healthy.startupReservations.length,1);assert.equal(healthy.startupReservations[0].attemptId,'attempt');
 assert.deepEqual(await readFile(damaged),original);assert.equal(await invokeRejected('restart_data_diagnostics'),true);
 assert.equal(await invokeRejected('core_request',{command:{type:'reviewStartupIntent',relativePath:file.relativePath,evidenceDigest:file.reviewDigest,reason:'not diagnostic'}}),true);
 await page.getByRole('button',{name:'工作总览',exact:false}).click();await page.getByRole('button',{name:'＋ 新建计划',exact:true}).waitFor();
 await page.screenshot({path:join(report,'healthy.png'),fullPage:true});assert.deepEqual(errors,[]);
 const appExit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([appExit,new Promise((_,reject)=>setTimeout(()=>reject(Error('owned window did not exit')),8000).unref())]);await browser.close();browser=null;
 const stop=spawnSync(core,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stop.status,0,stop.stderr);
 await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',at:new Date().toISOString(),data,realModelCalls:0,diagnosticMutationRejected:true,wrongRestartRejected:true,normalControlsHidden:true,fullCoreRestarts:2,originalBytesPreserved:true,restoration:'product review of unpublished truncated intent; permanent ID reservation',cancelDialogPreserved:true,staleDigestRejected:true,doubleClickGuard:true,healthyAfterUserReview:true,gracefulCoreStop:true},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(e){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(e),errors}));throw e;}
finally{await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exit=once(app,'exit');app.kill();await exit;}}
