// Native product UI/IPC/persistence with synthetic external-window observations.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');await assertNativeRuntime();
const scenario=process.env.CERULEAN_DESKTOP_RECOVERY_SCENARIO;assert.ok(['recover','retire'].includes(scenario));
const report=await mkdtemp(resolve(`reports/desktop-recovery-${scenario}-`));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
let serverLog='';
function launch(reopen=false){const child=spawn(exe,['--exact','host::desktop_sessions::tests::run_tests::native_desktop_recovery_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_NATIVE_DIR:report,CERULEAN_DESKTOP_NATIVE_REOPEN:reopen?'1':'0'}});child.stdout.on('data',b=>serverLog+=b);child.stderr.on('data',b=>serverLog+=b);return child;}
let server=launch();
const sleep=ms=>new Promise(r=>setTimeout(r,ms));async function until(fn,label){for(let i=0;i<900;i++){const v=await fn();if(v)return v;await sleep(100);}throw Error('Timed out: '+label);}
let app,browser,page,data;const errors=[];const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
async function open(){
  const socket=createServer().listen(0,'127.0.0.1');await once(socket,'listening');const port=socket.address().port;await new Promise(r=>socket.close(r));
  app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
  page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});const snap=await request({type:'snapshot'});assert.equal(toNamespacedPath(resolve(snap.dataDirectory)),toNamespacedPath(resolve(data)),'isolated fixture only');
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
}
async function close(){const exit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>reject(Error('window exit timeout')),8000).unref())]);await browser.close();browser=null;}

async function stopCore(){const stop=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stop.status,0,stop.stderr);}
async function fixtureReady(){return until(async()=>{assert.equal(server.exitCode,null,serverLog);try{const f=JSON.parse(await readFile(join(report,'fixture.json'),'utf8'));return f.pid===server.pid?f:false;}catch{return false;}},'fixture');}
try{
  const fixture=await fixtureReady();data=fixture.data;await sleep(300);await open();
  await page.getByRole('button',{name:'请求主指挥生成方案',exact:true}).click();await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  const plan=page.getByRole('dialog',{name:'审阅项目执行方案',exact:true});await plan.getByRole('button',{name:'确认执行授权',exact:true}).click();await plan.waitFor({state:'hidden'});
  await page.getByRole('button',{name:'启动已批准项目',exact:true}).click();
  const runDialog=()=>page.getByRole('dialog',{name:'启动持续项目执行',exact:true});await runDialog().getByRole('button',{name:'确认启动调度',exact:true}).click();await runDialog().waitFor({state:'hidden'});
  const tasks=()=>page.getByRole('region',{name:'桌面编程任务',exact:true}),runs=()=>page.getByRole('region',{name:'项目持续执行',exact:true});
  await tasks().getByRole('button',{name:'准备任务消息',exact:true}).click();await tasks().getByRole('button',{name:'审阅编程任务',exact:true}).click();
  const send=page.getByRole('dialog',{name:'确认发送编程任务',exact:true});await send.getByRole('checkbox').check();
  const original=(await request({type:'snapshot'})).desktopAccess.taskReview;
  await send.getByRole('button',{name:'确认发送任务一次',exact:true}).click();await send.waitFor({state:'hidden'});
  await tasks().getByRole('heading',{name:'任务结果待核对',exact:true}).waitFor({timeout:45000});
  await until(async()=>{const s=await request({type:'snapshot'});return s.projectRuns[0].state==='attention'&&s;},'unknown attention');
  let snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'unknown');assert.equal(snapshot.verificationJobs.length,0);assert.equal(snapshot.managementDecisions.length,2);assert.equal(snapshot.desktopAccess.tasks[0].proof,null);assert.ok(snapshot.desktopAccess.tasks[0].retirementDigest);
  await assert.rejects(request({type:'sendDesktopTask',reviewDigest:original.id}));await tasks().screenshot({path:join(report,'unknown.png')});
  const bytes=await readFile(join(report,'project/sum.mjs'));
  const before=snapshot.desktopAccess.tasks[0].review;
  // Terminate only this fixture-owned test core after its writer has finished.
  await close();const exit=once(server,'exit');server.kill();await exit;
  server=launch(true);await fixtureReady();await sleep(300);await open();
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'unknown');assert.equal(snapshot.managementDecisions.length,2);assert.equal(snapshot.verificationJobs.length,0);assert.deepEqual(snapshot.desktopAccess.tasks[0].review,before);assert.equal(snapshot.desktopAccess.tasks[0].proof,null);
  await tasks().screenshot({path:join(report,'unknown-restored.png')});
  const retirement=snapshot.desktopAccess.tasks[0].retirementDigest;assert.ok(retirement);
  await tasks().getByRole('button',{name:'结束本地跟踪',exact:true}).click();
  const retireDialog=()=>page.getByRole('dialog',{name:'结束未知桌面任务的本地跟踪',exact:true});await retireDialog().waitFor();
  assert.equal(await retireDialog().getByRole('button',{name:'确认结束本地跟踪',exact:true}).isDisabled(),true);
  await retireDialog().screenshot({path:join(report,'retirement-review.png')});
  if(scenario==='recover'){
    await retireDialog().getByRole('button',{name:'返回核对',exact:true}).click();
    await tasks().getByRole('button',{name:'回读原任务结果',exact:true}).click();
    await tasks().getByRole('heading',{name:'交付已提交，等待验收',exact:true}).waitFor({timeout:30000});
    snapshot=await request({type:'snapshot'});assert.equal(snapshot.projectRuns[0].state,'attention');assert.equal(snapshot.verificationJobs.length,0);assert.equal(snapshot.managementDecisions.length,2);
    await assert.rejects(request({type:'retireUnknownDesktop',projectId:'project',attemptId:original.attempt.id,evidenceDigest:retirement,reason:'stale review after readback'}));
    await tasks().screenshot({path:join(report,'readback-submitted.png')});
    await runs().getByRole('button',{name:'恢复项目调度',exact:true}).click();await runDialog().getByRole('button',{name:'确认启动调度',exact:true}).click();await runDialog().waitFor({state:'hidden'});
    await runs().getByText('项目已收口',{exact:true}).waitFor({timeout:60000});
    snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'accepted');assert.equal(snapshot.workflows[0].state.tasks.task.verified,true);assert.equal(snapshot.managementDecisions.length,3);assert.equal(snapshot.verificationJobs.length,1);assert.equal(snapshot.verificationJobs[0].state,'succeeded');assert.equal(snapshot.desktopAccess.tasks[0].retired,false);
  }else{
    await retireDialog().getByLabel('处置原因',{exact:true}).fill('隔离验证：外部运行已自行处理，保留未知结果和已有修改');
    assert.equal(await retireDialog().getByRole('button',{name:'确认结束本地跟踪',exact:true}).isDisabled(),true);
    await retireDialog().getByRole('checkbox').check();await retireDialog().getByRole('button',{name:'确认结束本地跟踪',exact:true}).click();await retireDialog().waitFor({state:'hidden'});
    await tasks().getByRole('heading',{name:'本地跟踪已结束，结果未确认',exact:true}).waitFor();
    snapshot=await request({type:'snapshot'});assert.equal(snapshot.workflows[0].state.tasks.task.state,'retired');assert.equal(snapshot.workflows[0].state.tasks.task.verified,false);assert.equal(snapshot.managementDecisions.length,2);assert.equal(snapshot.verificationJobs.length,0);assert.equal(snapshot.desktopAccess.tasks[0].submitted,false);assert.equal(snapshot.desktopAccess.tasks[0].proof,null);assert.equal(snapshot.desktopAccess.tasks[0].retired,true);
    assert.notEqual(snapshot.projectRuns[0].state,'completed');
    await assert.rejects(request({type:'readDesktopTask',projectId:'project',attemptId:original.attempt.id}));
    await assert.rejects(request({type:'retireUnknownDesktop',projectId:'project',attemptId:original.attempt.id,evidenceDigest:retirement,reason:'duplicate'}));
    await assert.rejects(request({type:'sendDesktopTask',reviewDigest:original.id}));
  }
  assert.deepEqual(await readFile(join(report,'project/sum.mjs')),bytes);assert.equal((await readdir(join(report,'project'))).filter(n=>n.startsWith('sent-once-')).length,1);
  const events=await Promise.all((await readdir(join(data,'workflows/project/events'))).sort().map(async name=>JSON.parse(await readFile(join(data,'workflows/project/events',name),'utf8'))));
  assert.equal(events.filter(e=>e.body.event.command.kind==='dispatch-intent').length,1);
  if(scenario==='retire'){const event=events.find(e=>e.body.event.command.kind==='retire-unknown-desktop');assert.equal(event.body.event.principal,'user');assert.equal(event.body.event.command.evidence_digest,retirement);assert.equal(events.filter(e=>['submit','verify','accept','cancel-intent'].includes(e.body.event.command.kind)).length,0);}
  await tasks().screenshot({path:join(report,'terminal.png')});
  await close();await stopCore();await until(()=>server.exitCode!==null,'fixture core normal exit');assert.equal(server.exitCode,0,serverLog);
  await open();const restored=await request({type:'snapshot'});assert.deepEqual(restored.desktopAccess.tasks,snapshot.desktopAccess.tasks);assert.deepEqual(restored.projectRuns,snapshot.projectRuns);assert.equal(restored.workflows[0].state.tasks.task.state,scenario==='recover'?'accepted':'retired');assert.equal(restored.managementDecisions.length,scenario==='recover'?3:2);
  await tasks().screenshot({path:join(report,'terminal-restored.png')});assert.deepEqual(errors,[]);
  await close();await stopCore();await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scenario,scope:'actual Tauri/WebView/IPC unknown dispatch, owned test-core termination/reopen, read-only recovery with explicit run resume OR user-only local retirement, production-core terminal reopen; external transports synthetic',unknownRestart:true,taskSends:1,readOnlyRecovery:scenario==='recover',independentNodeVerification:scenario==='recover',localRetirement:scenario==='retire',remoteCancellationConfirmed:false,managementRequests:scenario==='recover'?3:2,externalMessages:0,realModelCalls:0,pageErrors:errors},null,2));console.log(JSON.stringify({report,status:'passed',scenario}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally{await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server])if(child&&child.exitCode===null&&child.signalCode===null&&!child.killed){const exit=once(child,'exit');child.kill();await exit;}}
