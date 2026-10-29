// Real Tauri/IPC/background/persistence; desktop and OpenAI replies are fixtures.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');await assertNativeRuntime();
const scenario=process.env.CERULEAN_DESKTOP_CANCELLATION_SCENARIO;assert.ok(['cancel','recover','complete'].includes(scenario));
const report=await mkdtemp(resolve(`reports/desktop-cancellation-${scenario}-`));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
let serverLog='';
function launch(reopen=false){const c=spawn(exe,['--exact','host::desktop_sessions::tests::run_tests::native_desktop_cancellation_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_NATIVE_DIR:report,CERULEAN_DESKTOP_NATIVE_REOPEN:reopen?'1':'0'}});c.stdout.on('data',b=>serverLog+=b);c.stderr.on('data',b=>serverLog+=b);return c;}
let server=launch();const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let i=0;i<900;i++){const value=await fn();if(value)return value;await sleep(100);}throw Error('Timed out: '+label);}
let app,browser,page,data;const errors=[];const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
async function open(){
  const socket=createServer().listen(0,'127.0.0.1');await once(socket,'listening');const port=socket.address().port;await new Promise(r=>socket.close(r));
  app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
  page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});const s=await request({type:'snapshot'});assert.equal(toNamespacedPath(resolve(s.dataDirectory)),toNamespacedPath(resolve(data)));
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
}
async function close(){const exit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);await browser.close();browser=null;}
async function stopCore(){const s=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(s.status,0,s.stderr);}
async function ready(){return until(async()=>{assert.equal(server.exitCode,null,serverLog);try{const f=JSON.parse(await readFile(join(report,'fixture.json'),'utf8'));return f.pid===server.pid?f:false;}catch{return false;}},'fixture');}
const tasks=()=>page.getByRole('region',{name:'桌面编程任务',exact:true});
const runs=()=>page.getByRole('region',{name:'项目持续执行',exact:true});
const runDialog=()=>page.getByRole('dialog',{name:'启动持续项目执行',exact:true});
try{
  data=(await ready()).data;
  if(scenario!=='cancel')await writeFile(join(report,'project',scenario==='recover'?'unknown-stop':'complete-on-stop'),'fixture');
  await sleep(300);await open();
  await page.getByRole('button',{name:'请求主指挥生成方案',exact:true}).click();await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  const plan=page.getByRole('dialog',{name:'审阅项目执行方案',exact:true});await plan.getByRole('button',{name:'确认执行授权',exact:true}).click();await plan.waitFor({state:'hidden'});
  await page.getByRole('button',{name:'启动已批准项目',exact:true}).click();await runDialog().getByRole('button',{name:'确认启动调度',exact:true}).click();await runDialog().waitFor({state:'hidden'});
  await tasks().getByRole('button',{name:'准备任务消息',exact:true}).click();await tasks().getByRole('button',{name:'审阅编程任务',exact:true}).click();
  const send=page.getByRole('dialog',{name:'确认发送编程任务',exact:true});await send.getByRole('checkbox').check();const original=(await request({type:'snapshot'})).desktopAccess.taskReview;
  await send.getByRole('button',{name:'确认发送任务一次',exact:true}).click();await send.waitFor({state:'hidden'});
  await tasks().getByRole('heading',{name:'正在执行桌面任务',exact:true}).waitFor();await tasks().screenshot({path:join(report,'running.png')});
  await tasks().getByRole('button',{name:'停止原桌面任务',exact:true}).click();
  const stop=page.getByRole('dialog',{name:'停止原桌面任务',exact:true});const confirm=stop.getByRole('button',{name:'确认停止原任务一次',exact:true});assert.equal(await confirm.isDisabled(),true);
  await stop.getByLabel('停止原因',{exact:true}).fill('隔离验收：停止这一条原任务，保留修改');assert.equal(await confirm.isDisabled(),true);await stop.getByRole('checkbox').check();
  await stop.screenshot({path:join(report,'stop-review.png')});await confirm.click();await stop.waitFor({state:'hidden'});
  let snapshot=await until(async()=>{const s=await request({type:'snapshot'});return !s.desktopAccess.tasks[0].active&&s.desktopAccess.tasks[0].cancellation&&s;},'stop writer released');
  assert.equal(snapshot.projectRuns[0].state,'paused');assert.equal(snapshot.managementDecisions.length,2);assert.equal(snapshot.verificationJobs.length,0);
  await assert.rejects(request({type:'cancelDesktopTask',projectId:'project',attemptId:original.attempt.id,reviewDigest:original.id,reason:'duplicate'}));
  await assert.rejects(request({type:'sendDesktopTask',reviewDigest:original.id}));
  const bytes=await readFile(join(report,'project/sum.mjs'));
  if(scenario==='recover'){
    assert.equal(snapshot.workflows[0].state.tasks.task.state,'unknown');assert.equal(snapshot.desktopAccess.tasks[0].cancellation.confirmed,false);
    await tasks().getByRole('heading',{name:'任务结果待核对',exact:true}).waitFor();
    await tasks().screenshot({path:join(report,'unknown.png')});
    const dispatch=join(data,'desktop-task-dispatch-v1',(await readdir(join(data,'desktop-task-dispatch-v1')))[0]);const saved=await readFile(join(dispatch,'stop-intent.json'));
    await close();const exit=once(server,'exit');server.kill();await exit;server=launch(true);await ready();await sleep(300);await open();
    snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'unknown');assert.equal(snapshot.projectRuns[0].state,'paused');assert.equal(snapshot.desktopAccess.tasks[0].cancellation.controlReserved,true);
    await writeFile(join(report,'project/read-confirm'),'ready');await tasks().getByRole('button',{name:'回读原任务结果',exact:true}).click();
    await tasks().getByRole('heading',{name:'已确认取消，修改保留',exact:true}).waitFor();assert.deepEqual(await readFile(join(dispatch,'stop-intent.json')),saved);
  }else if(scenario==='complete'){
    assert.equal(snapshot.workflows[0].state.tasks.task.state,'submitted');assert.equal(snapshot.workflows[0].state.tasks.task.verified,false);assert.equal(snapshot.desktopAccess.tasks[0].cancellation.confirmed,false);
    await tasks().getByRole('heading',{name:'交付已提交，等待验收',exact:true}).waitFor();
    await tasks().screenshot({path:join(report,'completed-race.png')});
    await runs().getByRole('button',{name:'恢复项目调度',exact:true}).click();await runDialog().getByRole('button',{name:'确认启动调度',exact:true}).click();await runDialog().waitFor({state:'hidden'});
    await runs().getByText('项目已收口',{exact:true}).waitFor({timeout:60000});
  }else await tasks().getByRole('heading',{name:'已确认取消，修改保留',exact:true}).waitFor();
  snapshot=await request({type:'snapshot'});const expected=scenario==='complete'?'accepted':'cancelled';assert.equal(snapshot.workflows[0].state.tasks.task.state,expected);assert.equal(snapshot.workflows[0].state.tasks.task.verified,scenario==='complete');
  assert.equal(snapshot.desktopAccess.tasks[0].cancellation.confirmed,scenario!=='complete');assert.equal(snapshot.verificationJobs.length,scenario==='complete'?1:0);assert.equal(snapshot.managementDecisions.length,scenario==='complete'?3:2);
  assert.deepEqual(await readFile(join(report,'project/sum.mjs')),bytes);assert.equal((await readdir(join(report,'project'))).filter(n=>n.startsWith('sent-once-')).length,1);assert.ok((await readdir(join(report,'project'))).includes('stop-once'));
  const events=await Promise.all((await readdir(join(data,'workflows/project/events'))).sort().map(async name=>JSON.parse(await readFile(join(data,'workflows/project/events',name),'utf8'))));assert.equal(events.filter(e=>e.body.event.command.kind==='dispatch-intent').length,1);assert.equal(events.filter(e=>e.body.event.command.kind==='cancel-intent').length,1);
  assert.equal(events.filter(e=>e.body.event.command.kind==='reconcile'&&e.body.event.command.outcome==='cancelled').length,scenario==='complete'?0:1);
  const terminalHeading=scenario==='complete'?'已独立验收并收口':'已确认取消，修改保留';
  await tasks().getByRole('heading',{name:terminalHeading,exact:true}).waitFor();
  await tasks().screenshot({path:join(report,'terminal.png')});await close();await stopCore();await until(()=>server.exitCode!==null,'normal core exit');assert.equal(server.exitCode,0,serverLog);
  await open();const restored=await request({type:'snapshot'});assert.deepEqual(restored.desktopAccess.tasks,snapshot.desktopAccess.tasks);assert.deepEqual(restored.projectRuns,snapshot.projectRuns);assert.equal(restored.workflows[0].state.tasks.task.state,expected);
  await tasks().getByRole('heading',{name:terminalHeading,exact:true}).waitFor();
  await tasks().screenshot({path:join(report,'terminal-restored.png')});assert.deepEqual(errors,[]);await close();await stopCore();
  await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scenario,scope:'actual Tauri/IPC/background persistent cancellation; synthetic desktop and OpenAI replies; production core reopen',unknownOwnedCoreTermination:scenario==='recover',dispatches:1,stopInvocations:1,independentNodeVerification:scenario==='complete',managementRequests:scenario==='complete'?3:2,externalMessages:0,realModelCalls:0,externalUserAppStops:0,pageErrors:errors},null,2));console.log(JSON.stringify({report,status:'passed',scenario}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally{await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server])if(child&&child.exitCode===null&&child.signalCode===null&&!child.killed){const exit=once(child,'exit');child.kill();await exit;}}
