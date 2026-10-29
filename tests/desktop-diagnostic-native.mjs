// Actual Tauri diagnostics: metadata only, two whole-core restarts, no repair.
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
const report=await mkdtemp(resolve('reports/desktop-diagnostic-native-'));
const data=join(report,'data'),directory=join(data,'desktop-task-dispatch-v1','a'.repeat(64));
await mkdir(directory,{recursive:true});
const original=Buffer.from('{"private-marker-not-for-ui":"private","unfinished":');
const names=['intent','proof','seal-intent','completion','stop-request','stop-intent','stop-proof'].map(n=>`${n}.pending`).sort();
const relative=name=>`desktop-task-dispatch-v1/${'a'.repeat(64)}/${name}`;
for(const name of names)await writeFile(join(directory,name),original);
const core=resolve('target/debug/cerulean-core.exe'),desktop=resolve('target/debug/cerulean-desktop.exe');
let app,browser,page;const errors=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let i=0;i<180;i++){const r=await fn();if(r)return r;await sleep(100);}throw Error('Timed out: '+label);}
async function request(command){return page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);}
async function rejected(command){return page.evaluate(async command=>{try{await window.__TAURI_INTERNALS__.invoke('core_request',{command});return false;}catch{return true;}},command);}
async function unchanged(){assert.deepEqual((await readdir(directory)).sort(),names);for(const name of names)assert.deepEqual(await readFile(join(directory,name)),original);}
try{
 const server=createServer().listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));
 app=spawn(desktop,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
 await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
 page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
 await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
 const region=page.getByRole('region',{name:'执行数据诊断',exact:true});await region.waitFor();
 const desktopRecords=region.getByRole('region',{name:'桌面任务记录诊断',exact:true});await desktopRecords.waitFor();
 let before=await request({type:'snapshot'});assert.equal(before.coreDiagnostic.desktopFiles.length,7);assert.equal(before.coreDiagnostic.startupFiles.length,0);
 assert.equal(JSON.stringify(before).includes('private-marker-not-for-ui'),false);
 for(const name of names)await desktopRecords.getByRole('heading',{name:relative(name),exact:true}).waitFor();
 for(const stage of ['原任务派发意图','原任务完成回复','文件封存意图','原交付封存','用户停止请求','单次停止动作意图','取消终态证明'])await desktopRecords.getByText(stage,{exact:false}).waitFor();
 assert.equal(await desktopRecords.getByRole('button').count(),0);
 assert.equal(before.coreDiagnostic.desktopFiles.filter(f=>f.reviewDigest||f.reviewKind).length,0);
 for(const command of [{type:'createPlan',title:'blocked',tasks:['blocked']},{type:'checkOpenAi'},{type:'cancelDesktopTask',projectId:'project',attemptId:'attempt',reviewDigest:'a'.repeat(64),reason:'blocked'},{type:'reviewStartupIntent',relativePath:relative(names[0]),evidenceDigest:'0'.repeat(64),reason:'wrong recovery route'}])assert.equal(await rejected(command),true);
 for(const name of ['工作总览','需求与计划','接入管理','交接记录','项目设置']){await page.getByRole('button',{name,exact:false}).click();await desktopRecords.waitFor();assert.equal(await page.getByRole('button',{name:'＋ 新建计划',exact:true}).count(),0);}
 await region.getByRole('button',{name:'刷新诊断',exact:true}).click();
 await until(async()=>!(await region.getByRole('button',{name:'重新启动并校验',exact:true}).isDisabled()),'refresh');
 await unchanged();
 await desktopRecords.getByText('查看桌面文件摘要',{exact:true}).first().click();
 await region.screenshot({path:join(report,'diagnostic.png')});
 for(let index=0;index<2;index++){
  await region.getByRole('button',{name:'重新启动并校验',exact:true}).click();
  await until(async()=>{try{const s=await request({type:'snapshot'});return s.coreDiagnostic&&s.coreDiagnostic.detectedAt!==before.coreDiagnostic.detectedAt?s:null;}catch{return false;}},'new diagnostic host');
  await until(async()=>!(await region.getByRole('button',{name:'重新启动并校验',exact:true}).isDisabled()),'recheck completed');
  before=await request({type:'snapshot'});assert.equal(before.coreDiagnostic.desktopFiles.length,7);assert.equal(before.workflows.length,0);assert.equal(before.events.length,0);await unchanged();
 }
 assert.equal(await page.locator('body').innerText().then(t=>t.includes('private-marker-not-for-ui')),false);
 await region.screenshot({path:join(report,'reopened.png')});assert.deepEqual(errors,[]);
 const exited=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exited,new Promise((_,reject)=>setTimeout(()=>reject(Error('owned window exit timeout')),8000).unref())]);
 await browser.close();browser=null;
 const stop=spawnSync(core,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stop.status,0,stop.stderr);
 await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',at:new Date().toISOString(),data,realModelCalls:0,desktopFiles:7,fullCoreRestarts:2,originalBytesPreserved:true,rawContentsHidden:true,mutationsRejected:true,normalControlsHidden:true,stageLabelsRendered:true,reviewNotPerformed:true,reviewableFiles:0,partialRecoveryImplemented:false,gracefulCoreStop:true},null,2));
 console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;}
finally{await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exited=once(app,'exit');app.kill();await exited;}}
