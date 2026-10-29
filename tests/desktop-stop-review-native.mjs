// Production Tauri/IPC review and retirement; original desktop transport is synthetic.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/desktop-stop-review-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
const seed=spawnSync(exe,['--exact','host::desktop_sessions::tests::stop_review_tests::native_desktop_stop_review_seed','--ignored','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:45000,env:{...process.env,CERULEAN_DESKTOP_STOP_REVIEW_DIR:report}});assert.equal(seed.status,0,seed.stdout+seed.stderr);
await writeFile(join(report,'seed.log'),seed.stdout+seed.stderr);
const fixtures=JSON.parse(await readFile(join(report,'fixtures.json'),'utf8'));
let app,browser,page,data,caseDir;const errors=[],results=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let i=0;i<220;i++){const value=await fn();if(value)return value;await sleep(100);}throw Error('Timed out: '+label);}
const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
const rejected=command=>page.evaluate(async command=>{try{await window.__TAURI_INTERNALS__.invoke('core_request',{command});return false;}catch{return true;}},command);
const tasks=()=>page.getByRole('region',{name:'桌面编程任务',exact:true});
async function open(){
 const socket=createServer().listen(0,'127.0.0.1');await once(socket,'listening');const port=socket.address().port;await new Promise(r=>socket.close(r));
 app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(caseDir,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
 await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
 page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
 await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
 await page.getByRole('button',{name:'需求与计划',exact:false}).click();
}
async function close(stopCore=true){const exit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);await browser.close();browser=null;if(!stopCore)return;const stop=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stop.status,0,stop.stderr);}
try{
 for(const f of fixtures){
  data=f.data;caseDir=join(report,f.stage);
  const dir=join(data,f.relativePath,'..');
  const originals=new Map(await Promise.all((await readdir(dir)).map(async name=>[name,await readFile(join(dir,name))])));
  async function preserved(){assert.deepEqual((await readdir(dir)).sort(),[...originals.keys()].sort());for(const [name,bytes] of originals)assert.deepEqual(await readFile(join(dir,name)),bytes);}
  const eventDir=join(data,'workflows/project/events');
  const beforeNames=(await readdir(eventDir)).sort();
  await open();
  const diagnostic=page.getByRole('region',{name:'执行数据诊断',exact:true});await diagnostic.waitFor();
  const before=await request({type:'snapshot'}),file=before.coreDiagnostic.desktopFiles.find(v=>v.relativePath===f.relativePath);assert.equal(file.reviewKind,'desktop-stop');
  assert.equal(await rejected({type:'reviewDesktopStopRecord',relativePath:f.relativePath,evidenceDigest:'0'.repeat(64),reason:'stale'}),true);
  await diagnostic.getByRole('button',{name:'复核未写完整的桌面停止记录…',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'桌面停止残片复核',exact:true}),confirm=dialog.getByRole('button',{name:'保存桌面停止复核',exact:true});
  assert.equal(await confirm.isDisabled(),true);await dialog.getByLabel('复核说明',{exact:true}).fill('已核对原文件，仅恢复本地管理，保留未知结果');assert.equal(await confirm.isDisabled(),true);await dialog.getByRole('checkbox').check();
  await dialog.screenshot({path:join(caseDir,'review-dialog.png')});await confirm.click();await dialog.waitFor({state:'hidden'});
  await until(async()=>{const s=await request({type:'snapshot'});return s.coreDiagnostic.desktopFiles.find(v=>v.relativePath===f.relativePath)?.reviewed;},'review saved');
  await preserved();assert.deepEqual((await readdir(eventDir)).sort(),beforeNames);
  await diagnostic.getByRole('button',{name:'重新启动并校验',exact:true}).click();await diagnostic.waitFor({state:'hidden'});await tasks().waitFor();
  let snapshot=await request({type:'snapshot'});assert.equal(snapshot.coreDiagnostic,null);assert.equal(snapshot.workflows[0].state.tasks.task.state,'unknown');assert.equal(snapshot.desktopAccess.tasks[0].reviewedStopRecord,f.stage);
  assert.equal(await tasks().getByRole('button',{name:'回读原任务结果',exact:true}).isDisabled(),true);assert.equal(await tasks().getByRole('button',{name:'停止原桌面任务',exact:true}).count(),0);
  for(const command of [{type:'readDesktopTask',projectId:'project',attemptId:f.attemptId},{type:'cancelDesktopTask',projectId:'project',attemptId:f.attemptId,reviewDigest:f.reviewDigest,reason:'forbidden retry'}])assert.equal(await rejected(command),true);
  await tasks().screenshot({path:join(caseDir,'unknown-restored.png')});await preserved();await close(false);
  await open();snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'unknown');assert.ok(snapshot.desktopAccess.tasks[0].retirementDigest);
  await tasks().getByRole('button',{name:'结束本地跟踪',exact:true}).click();
  const retire=page.getByRole('dialog',{name:'结束未知桌面任务的本地跟踪',exact:true});await retire.getByRole('textbox').fill('已自行处理外部应用，仅结束本地跟踪，原结果仍未知');await retire.getByRole('checkbox').check();await retire.getByRole('button',{name:'确认结束本地跟踪',exact:true}).click();await retire.waitFor({state:'hidden'});
  await tasks().getByRole('heading',{name:'本地跟踪已结束，结果未确认',exact:true}).waitFor();await close();await open();
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'retired');assert.equal(snapshot.workflows[0].state.tasks.task.verified,false);assert.equal(snapshot.desktopAccess.tasks[0].submitted,false);assert.equal(snapshot.desktopAccess.tasks[0].cancellation?.confirmed??false,false);assert.equal(snapshot.desktopAccess.tasks[0].reviewedStopRecord,f.stage);
  await tasks().screenshot({path:join(caseDir,'retired-reopened.png')});await preserved();
  const events=await Promise.all((await readdir(eventDir)).map(async n=>JSON.parse(await readFile(join(eventDir,n),'utf8'))));
  assert.equal(events.filter(e=>e.body.event.command.kind==='dispatch-intent').length,1);assert.equal(events.filter(e=>e.body.event.command.kind==='retire-unknown-desktop').length,1);assert.equal(events.filter(e=>['submit','verify','accept'].includes(e.body.event.command.kind)||e.body.event.command.kind==='reconcile'&&e.body.event.command.outcome==='cancelled').length,0);
  results.push({stage:f.stage,originalBytesPreserved:true,fullCoreRestarts:2,unknownWindowReopen:true,finalState:'retired',verified:false,dispatches:1,retirements:1});await close();
 }
 assert.deepEqual(errors,[]);
 await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'production Tauri and core; original dispatch/stop transport synthetic; original partial writes injected',cases:results,realModelCalls:0,externalMessages:0,externalAppStops:0,repairReplays:0,screenshots:9,pageErrors:errors},null,2));console.log(JSON.stringify({status:'passed',report}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally{await browser?.close().catch(()=>{});if(app&&app.exitCode===null){const exit=once(app,'exit');app.kill();await exit;}}
