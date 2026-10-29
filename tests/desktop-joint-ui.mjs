// Full Tauri/IPC/UIA path. The WPF application is owned independently of the core.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');await assertNativeRuntime();
const scenario=process.env.CERULEAN_DESKTOP_JOINT_UI_SCENARIO;
assert.ok(['success','unconfirmed'].includes(scenario));
const report=await mkdtemp(resolve('reports/desktop-joint-ui-'+scenario+'-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
const windowExe=join(report,'Qoder.exe');
const compile=spawnSync(join(framework,'csc.exe'),['/nologo','/target:winexe','/platform:x64','/r:System.Web.Extensions.dll','/r:System.Xaml.dll',...['PresentationFramework','PresentationCore','WindowsBase','UIAutomationProvider','UIAutomationTypes'].map(r=>'/r:'+join(framework,'WPF',r+'.dll')),'/out:'+windowExe,resolve('tests/fixtures/DesktopJointWindow.cs')],{windowsHide:true,encoding:'utf8',timeout:30000});
assert.equal(compile.status,0,compile.stdout+compile.stderr);
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});
if(build.status!==0)await writeFile(join(report,'build-failure.log'),build.stdout+'\n'+build.stderr);
assert.equal(build.status,0,build.stdout+build.stderr);
const testExe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(testExe);
const ownedWindow=spawn(windowExe,[report,scenario,String(process.pid)],{windowsHide:true,stdio:'ignore'});
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label,timeout=30000){const start=Date.now();while(Date.now()-start<timeout){const v=await fn();if(v)return v;await sleep(100);}throw Error('Timed out: '+label);}
async function file(name){return JSON.parse(await readFile(join(report,name),'utf8'));}
async function hashes(dir){const result={};for(const e of await readdir(dir,{withFileTypes:true})){const p=join(dir,e.name);if(e.isDirectory())Object.assign(result,await hashes(p));else result[p]=createHash('sha256').update(await readFile(p)).digest('hex');}return result;}
async function terminate(child){if(child&&child.exitCode===null&&child.signalCode===null&&!child.killed){const exit=once(child,'exit');child.kill();await exit;}}
let server,app,browser,page,data,serverLog='';const errors=[];
const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
function collect(child){child.stdout?.on('data',b=>serverLog+=b);child.stderr?.on('data',b=>serverLog+=b);return child;}
function launchProduction(){return collect(spawn(resolve('target/debug/cerulean-core.exe'),['--serve','--data-dir',data],{windowsHide:true,stdio:['ignore','pipe','pipe']}));}
async function open(){
 const socket=createServer().listen(0,'127.0.0.1');await once(socket,'listening');const port=socket.address().port;await new Promise(r=>socket.close(r));
 app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
 await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
 page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
 await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});const s=await request({type:'snapshot'});assert.equal(toNamespacedPath(resolve(s.dataDirectory)),toNamespacedPath(resolve(data)));
 await page.getByRole('button',{name:'需求与计划',exact:false}).click();
}
async function close(){const exit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);await browser.close();browser=null;}
async function stopCore(){const stop=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stop.status,0,stop.stderr);await until(()=>server.exitCode!==null,'owned core exit');assert.equal(server.exitCode,0,serverLog);}
const tasks=()=>page.getByRole('region',{name:'桌面编程任务',exact:true});
const runDialog=()=>page.getByRole('dialog',{name:'启动持续项目执行',exact:true});
async function counters(){assert.equal(await file('send-count.json'),1);assert.equal(await file('stop-count.json'),1);assert.equal(ownedWindow.exitCode,null);assert.equal(ownedWindow.signalCode,null);}
try {
 const identity=await until(async()=>{assert.equal(ownedWindow.exitCode,null);try{return await file('window.json');}catch{return false;}},'owned WPF');assert.equal(identity.pid,ownedWindow.pid);
 server=collect(spawn(testExe,['--exact','host::desktop_sessions::tests::run_tests::joint_native::ui::native_desktop_joint_ui_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_JOINT_UI_DIR:report,CERULEAN_OWNED_WINDOW_PID:String(ownedWindow.pid)}}));
 const fixture=await until(async()=>{assert.equal(server.exitCode,null,serverLog);try{return await file('fixture.json');}catch{return false;}},'IPC fixture');assert.equal(fixture.desktopFixtureUsed,false);data=fixture.data;
 await writeFile(join(report,'project/retained-user-file.txt'),'pre-existing owned fixture changes\n');const retainedFile=await readFile(join(report,'project/retained-user-file.txt'));
 await sleep(300);await open();
 await page.getByRole('button',{name:'请求主指挥生成方案',exact:true}).click();await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
 const plan=page.getByRole('dialog',{name:'审阅项目执行方案',exact:true});await plan.getByRole('button',{name:'确认执行授权',exact:true}).click();await plan.waitFor({state:'hidden'});
 await page.getByRole('button',{name:'启动已批准项目',exact:true}).click();await runDialog().getByRole('button',{name:'确认启动调度',exact:true}).click();await runDialog().waitFor({state:'hidden'});
 await tasks().getByRole('button',{name:'准备任务消息',exact:true}).click();await tasks().getByRole('button',{name:'审阅编程任务',exact:true}).click();
 const send=page.getByRole('dialog',{name:'确认发送编程任务',exact:true});assert.equal(await send.getByRole('button',{name:'确认发送任务一次',exact:true}).isDisabled(),true);await send.getByRole('checkbox').check();
 const original=(await request({type:'snapshot'})).desktopAccess.taskReview;assert.equal(original.observation.window.pid,ownedWindow.pid);assert.equal(original.maxWaitSeconds,50);await writeFile(join(report,'original-review.json'),JSON.stringify(original,null,2));
 await send.screenshot({path:join(report,'send-review.png')});await send.getByRole('button',{name:'确认发送任务一次',exact:true}).click();await send.waitFor({state:'hidden'});
 await until(async()=>{try{return await file('started.json');}catch{return false;}},'actual WPF send',12000);assert.equal(await file('send-count.json'),1);
 let started=Date.now();let snapshot=await request({type:'snapshot'});const snapshotLatencyMs=Date.now()-started;assert.ok(snapshotLatencyMs<2000);assert.equal(snapshot.desktopAccess.tasks[0].active,true);
 const dispatchDir=join(data,'desktop-task-dispatch-v1');const before=await hashes(dispatchDir);await tasks().screenshot({path:join(report,'running.png')});
 await tasks().getByRole('button',{name:'停止原桌面任务',exact:true}).click();const stop=page.getByRole('dialog',{name:'停止原桌面任务',exact:true});const confirm=stop.getByRole('button',{name:'确认停止原任务一次',exact:true});assert.equal(await confirm.isDisabled(),true);
 await stop.getByLabel('停止原因',{exact:true}).fill('联合 UIA 验证：只停止原任务并保留修改');assert.equal(await confirm.isDisabled(),true);await stop.getByRole('checkbox').check();await stop.screenshot({path:join(report,'stop-review.png')});
 started=Date.now();await confirm.click();await stop.waitFor({state:'hidden'});const stopRequestLatencyMs=Date.now()-started;assert.ok(stopRequestLatencyMs<3000);
 snapshot=await until(async()=>{const s=await request({type:'snapshot'});return !s.desktopAccess.tasks[0].active&&s.desktopAccess.tasks[0].cancellation&&s;},'actual UIA stop completed',25000);const stopCompletedMs=Date.now()-started;assert.ok(stopCompletedMs<25000);
 const expected=scenario==='success'?'cancelled':'unknown';assert.equal(snapshot.workflows[0].state.tasks.task.state,expected,JSON.stringify(snapshot.desktopAccess));assert.equal(snapshot.workflows[0].state.tasks.task.verified,false);assert.equal(snapshot.desktopAccess.tasks[0].cancellation.confirmed,scenario==='success');assert.equal(snapshot.projectRuns[0].state,'paused');assert.equal(snapshot.managementDecisions.length,2);assert.equal(snapshot.verificationJobs.length,0);await counters();
 const after=await hashes(dispatchDir);for(const [path,hash] of Object.entries(before))assert.equal(after[path],hash);
 const terminalHeading=scenario==='success'?'已确认取消，修改保留':'任务结果待核对';
 const stoppedHashes=await hashes(dispatchDir);await writeFile(join(report,'stopped-snapshot.json'),JSON.stringify(snapshot,null,2));await tasks().getByRole('heading',{name:terminalHeading,exact:true}).waitFor();await tasks().screenshot({path:join(report,'stopped.png')});
 await assert.rejects(request({type:'cancelDesktopTask',projectId:'project',attemptId:original.attempt.id,reviewDigest:original.id,reason:'duplicate'}));await assert.rejects(request({type:'sendDesktopTask',reviewDigest:original.id}));
 // Kill only the known test/core child after its background operation is terminal.
 // The external WPF process belongs to this driver and survives every core restart.
 for(let reopen=1;reopen<=2;reopen++) {
  await close();if(expected==='unknown')await terminate(server);else await stopCore();
  server=launchProduction();await sleep(300);await open();assert.equal(server.exitCode,null,serverLog);
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,expected);assert.equal(snapshot.desktopAccess.tasks[0].active,false);assert.equal(snapshot.projectRuns[0].state,'paused');assert.equal(snapshot.managementDecisions.length,2);assert.equal(snapshot.verificationJobs.length,0);
  await assert.rejects(request({type:'sendDesktopTask',reviewDigest:original.id}));await assert.rejects(request({type:'cancelDesktopTask',projectId:'project',attemptId:original.attempt.id,reviewDigest:original.id,reason:'no replay'}));
  if(expected==='unknown') {await tasks().getByRole('button',{name:'回读原任务结果',exact:true}).click();await until(async()=>{const s=await request({type:'snapshot'});return !s.desktopAccess.tasks[0].active&&s;},'read-only original outcome');snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'unknown');}
  assert.deepEqual(await hashes(dispatchDir),stoppedHashes);await counters();await tasks().getByRole('heading',{name:terminalHeading,exact:true}).waitFor();await tasks().screenshot({path:join(report,'restored-'+reopen+'.png')});
 }
 if(expected==='unknown') {
  await tasks().getByRole('button',{name:'结束本地跟踪',exact:true}).click();const retire=page.getByRole('dialog',{name:'结束未知桌面任务的本地跟踪',exact:true});await retire.waitFor();
  const button=retire.getByRole('button',{name:'确认结束本地跟踪',exact:true});assert.equal(await button.isDisabled(),true);await retire.getByLabel('处置原因',{exact:true}).fill('受控测试：结束本地跟踪，外部结果保持未确认');assert.equal(await button.isDisabled(),true);await retire.getByRole('checkbox').check();await button.click();await retire.waitFor({state:'hidden'});
  await tasks().getByRole('heading',{name:'本地跟踪已结束，结果未确认',exact:true}).waitFor();snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'retired');assert.equal(snapshot.workflows[0].state.tasks.task.verified,false);assert.equal(snapshot.desktopAccess.tasks[0].cancellation.confirmed,false);assert.deepEqual(await hashes(dispatchDir),stoppedHashes);await tasks().screenshot({path:join(report,'retired.png')});
 }
 const events=await Promise.all((await readdir(join(data,'workflows/project/events'))).sort().map(async n=>JSON.parse(await readFile(join(data,'workflows/project/events',n),'utf8'))));
 assert.equal(events.filter(e=>e.body.event.command.kind==='dispatch-intent').length,1);assert.equal(events.filter(e=>e.body.event.command.kind==='cancel-intent').length,1);assert.equal(events.filter(e=>['submit','verify','accept'].includes(e.body.event.command.kind)).length,0);
 assert.deepEqual(await readFile(join(report,'project/retained-user-file.txt')),retainedFile);await counters();assert.deepEqual(errors,[]);await close();await stopCore();
 const result={status:'passed',scenario,scope:'actual Tauri UI/IPC and production UIA send/stop against independently owned WPF; synthetic management only; two production core reopens',desktopFixtureUsed:false,realUia:true,originalState:expected,finalState:scenario==='success'?'cancelled':'retired',verified:false,remoteCancellationConfirmed:scenario==='success',dispatches:1,stopInvocations:1,productionCoreReopens:2,ownedCoreTerminations:scenario==='unconfirmed'?2:0,readOnlyRecovery:scenario==='unconfirmed',windowSurvived:true,originalBytesPreserved:true,managementRequests:2,snapshotLatencyMs,stopRequestLatencyMs,stopCompletedMs,originalObserverWaitSeconds:50,externalUserMessages:0,realModelCalls:0,externalUserAppStops:0,pageErrors:errors};
 await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,...result}));
} catch(error) {await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally {await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server,ownedWindow])await terminate(child);}
