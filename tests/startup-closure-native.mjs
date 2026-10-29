// Real Tauri/IPC and whole-core restart; synthetic interrupted startup closure, no model.
// Run in the owned Windows Job wrapper so an assertion also cleans a restarted resident.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, readFile, writeFile, readdir } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';
import { assertNativeRuntime } from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','requires owned process wrapper');
await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/startup-closure-native-'));
const dir=await mkdtemp(join(process.env.CERULEAN_OMP_TEST_PARENT,'native-closure-review-')),data=join(dir,'data');
await writeFile(join(report,'location.json'),JSON.stringify({dir,data}));
const fixture=spawnSync('cargo',['test','-p','cerulean-core','--lib','host::omp_controls_native::native_omp_controls_fixture','--locked','--','--exact','--ignored','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:120000,env:{...process.env,CERULEAN_OMP_CONTROL_DIR:dir,CERULEAN_OMP_CONTROL_MODE:'startup',CERULEAN_OMP_CONTROL_CLOSURE_PENDING:'1'}});
assert.equal(fixture.status,0,fixture.stdout+fixture.stderr);
const core=resolve('target/debug/cerulean-core.exe'),desktop=resolve('target/debug/cerulean-desktop.exe');
let app,browser,page;const errors=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let i=0;i<180;i++){const result=await fn();if(result)return result;await sleep(100);}throw Error('Timed out: '+label);}
async function request(command){return page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);}
const eventCount=async()=>(await readdir(join(data,'workflows/project/events'))).length;
try {
 const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
 app=spawn(desktop,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(dir,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
 await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
 page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
 await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
 const region=page.getByRole('region',{name:'执行数据诊断',exact:true});await region.waitFor();
 const before=await request({type:'snapshot'}),issue=before.coreDiagnostic.startupFiles[0],events=await eventCount();
 assert.equal(issue.reviewKind,'closure');assert.ok(issue.reviewDigest);
 const originalPath=join(data,issue.relativePath),original=await readFile(originalPath);
 const intentPath=join(data,'omp-startups/project/attempt.0.json'),intent=await readFile(intentPath);
 await region.getByRole('button',{name:'复核半写关闭记录…',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'确认复核半写关闭记录',exact:true});await dialog.waitFor();
 assert.equal(await dialog.getByRole('button',{name:'保存关闭复核',exact:true}).isDisabled(),true);
 await dialog.getByRole('button',{name:'返回检查',exact:true}).click();assert.equal((await request({type:'snapshot'})).coreDiagnostic.startupFiles[0].reviewed,false);
 assert.equal(await page.evaluate(async issue=>{try{await window.__TAURI_INTERNALS__.invoke('core_request',{command:{type:'reviewStartupClosure',relativePath:issue.relativePath,evidenceDigest:'0'.repeat(64),reason:'stale'}});return false;}catch{return true;}},issue),true);
 await region.getByRole('button',{name:'复核半写关闭记录…',exact:true}).click();
 await dialog.locator('#startup-intent-review-reason').fill('保留原关闭残片，恢复有效启动后重新审核关闭');await dialog.getByRole('checkbox').check();await dialog.screenshot({path:join(report,'review.png')});
 await dialog.getByRole('button',{name:'保存关闭复核',exact:true}).evaluate(b=>{b.click();b.click();});await dialog.waitFor({state:'hidden'});
 assert.equal((await request({type:'snapshot'})).coreDiagnostic.startupFiles[0].reviewed,true);assert.equal(await eventCount(),events);
 await region.getByRole('button',{name:'重新启动并校验',exact:true}).click();await region.waitFor({state:'hidden',timeout:45000});
 const reopened=await request({type:'snapshot'});assert.equal(reopened.startupClosureReviews.length,1);assert.equal(reopened.ompJobs[0].state,'unknown');assert.equal(await eventCount(),events);
 await page.getByRole('button',{name:'需求与计划',exact:false}).click();const panel=page.getByRole('region',{name:'OMP 执行记录',exact:true});await panel.waitFor();await panel.screenshot({path:join(report,'reopened-unknown.png')});
 await panel.getByRole('button',{name:'关闭未分配启动…',exact:true}).click();const close=page.getByRole('dialog',{name:'确认关闭 OMP 启动',exact:true});await close.waitFor();
 await close.locator('#omp-control-reason').fill('完成新的独立关闭审阅');await close.getByRole('checkbox').check();await close.getByRole('button',{name:'确认处置',exact:true}).click();await close.waitFor({state:'hidden'});
 assert.equal((await request({type:'snapshot'})).ompJobs[0].state,'abandoned');assert.equal(await eventCount(),events);
 assert.deepEqual(await readFile(originalPath),original);assert.deepEqual(await readFile(intentPath),intent);
 assert.equal((await readdir(join(data,'startup-closure-reviews'))).filter(n=>n.endsWith('.json')).length,1);
 await panel.screenshot({path:join(report,'closed.png')});
 assert.deepEqual(errors,[]);
 const appExit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([appExit,new Promise((_,reject)=>setTimeout(()=>reject(Error('owned window did not exit')),8000).unref())]);await browser.close();browser=null;
 const stop=spawnSync(core,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stop.status,0,stop.stderr);
 await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',at:new Date().toISOString(),dir,realModelCalls:0,cancelledReviewPreserved:true,staleDigestRejected:true,duplicateClickGuard:true,fullCoreRestarts:1,originalIntentAndPartialClosurePreserved:true,workflowEventsUnchanged:true,reviewDidNotCloseStartup:true,secondExplicitCloseRequired:true,finalStartupState:'abandoned',gracefulCoreStop:true},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(e){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(e),errors}));throw e;}
finally{await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exit=once(app,'exit');app.kill();await exit;}}
