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
const report=await mkdtemp(resolve('reports/desktop-managed-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
const server=spawn(exe,['--exact','host::desktop_sessions::tests::run_tests::native_desktop_managed_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_NATIVE_DIR:report}});
let serverLog='';server.stdout.on('data',b=>serverLog+=b);server.stderr.on('data',b=>serverLog+=b);
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
try{
  const fixture=await until(async()=>{assert.equal(server.exitCode,null,serverLog);try{return JSON.parse(await readFile(join(report,'fixture.json'),'utf8'));}catch{return false;}},'fixture');
  data=fixture.data;await sleep(300);await open();
  let snapshot=await request({type:'snapshot'});assert.equal(snapshot.managementDecisions.length,0);assert.equal(snapshot.projectRuns.length,0);assert.equal(snapshot.desktopAccess.tasks.length,0);
  await page.getByRole('button',{name:'请求主指挥生成方案',exact:true}).click();
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  const plan=page.getByRole('dialog',{name:'审阅项目执行方案',exact:true});await plan.waitFor();await plan.screenshot({path:join(report,'plan-review.png')});
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.managementDecisions.length,1);assert.equal(snapshot.projectRuns.length,0);assert.equal(snapshot.desktopAccess.tasks.length,0);
  await plan.getByRole('button',{name:'确认执行授权',exact:true}).click();await plan.waitFor({state:'hidden'});
  await page.getByRole('button',{name:'启动已批准项目',exact:true}).click();
  const runDialog=page.getByRole('dialog',{name:'启动持续项目执行',exact:true});await runDialog.waitFor();await runDialog.screenshot({path:join(report,'run-review.png')});
  await runDialog.getByRole('button',{name:'确认启动调度',exact:true}).click();await runDialog.waitFor({state:'hidden'});
  const tasks=page.getByRole('region',{name:'桌面编程任务',exact:true}),runs=page.getByRole('region',{name:'项目持续执行',exact:true});
  await tasks.getByRole('button',{name:'准备任务消息',exact:true}).waitFor({timeout:25000});
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.managementDecisions.length,2);assert.equal(snapshot.workflows[0].state.tasks.task.state,'assigned');assert.equal(snapshot.desktopAccess.tasks.length,0);
  await tasks.getByRole('button',{name:'准备任务消息',exact:true}).click();await tasks.getByRole('button',{name:'审阅编程任务',exact:true}).click();
  const taskDialog=page.getByRole('dialog',{name:'确认发送编程任务',exact:true});await taskDialog.waitFor();
  const prepared=(await request({type:'snapshot'})).desktopAccess.taskReview;
  assert.equal(await taskDialog.getByRole('button',{name:'确认发送任务一次',exact:true}).isDisabled(),true);
  await taskDialog.getByRole('button',{name:'返回检查',exact:true}).click();
  await runs.getByRole('button',{name:'暂停后续调度',exact:true}).click();await runs.getByText('已暂停后续调度',{exact:true}).waitFor();
  await assert.rejects(request({type:'sendDesktopTask',reviewDigest:prepared.id}));
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.desktopAccess.tasks.length,0);assert.equal(snapshot.managementDecisions.length,2);
  assert.equal(await tasks.getByRole('button',{name:'准备任务消息',exact:true}).isDisabled(),true);await runs.screenshot({path:join(report,'paused.png')});
  await runs.getByRole('button',{name:'恢复项目调度',exact:true}).click();await runDialog.getByRole('button',{name:'确认启动调度',exact:true}).click();await runDialog.waitFor({state:'hidden'});
  await tasks.getByRole('button',{name:'准备任务消息',exact:true}).click();await tasks.getByRole('button',{name:'审阅编程任务',exact:true}).click();await taskDialog.getByRole('checkbox').check();
  const original=(await request({type:'snapshot'})).desktopAccess.taskReview;await taskDialog.screenshot({path:join(report,'task-review.png')});
  await taskDialog.getByRole('button',{name:'确认发送任务一次',exact:true}).click();await taskDialog.waitFor({state:'hidden'});
  await tasks.getByRole('button',{name:'准备任务消息',exact:true}).waitFor({timeout:60000});
  snapshot=await request({type:'snapshot'});
  assert.equal(snapshot.managementDecisions.length,4);assert.equal(snapshot.workflows[0].state.tasks.task.state,'accepted');assert.equal(snapshot.workflows[0].state.tasks.second.state,'assigned');assert.equal(snapshot.desktopAccess.tasks.length,1);
  await tasks.getByRole('button',{name:'准备任务消息',exact:true}).click();await tasks.getByRole('button',{name:'审阅编程任务',exact:true}).click();await taskDialog.waitFor();
  const second=(await request({type:'snapshot'})).desktopAccess.taskReview;
  assert.equal(second.continuation.previousAttemptId,original.attempt.id);assert.equal(second.continuation.previousReviewDigest,original.id);
  assert.equal(second.baseline.at(-1),snapshot.desktopAccess.tasks[0].proof.roundSha256);
  assert.equal(await taskDialog.getByRole('button',{name:'确认发送任务一次',exact:true}).isDisabled(),true);
  await taskDialog.getByRole('checkbox').check();await taskDialog.screenshot({path:join(report,'second-task-review.png')});
  await taskDialog.getByRole('button',{name:'确认发送任务一次',exact:true}).click();await taskDialog.waitFor({state:'hidden'});
  await runs.getByText('项目已收口',{exact:true}).waitFor({timeout:60000});
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.managementDecisions.length,5);assert.equal(snapshot.projectRuns[0].state,'completed');
  for(const task of ['task','second']) { assert.equal(snapshot.workflows[0].state.tasks[task].state,'accepted');assert.equal(snapshot.workflows[0].state.tasks[task].verified,true); }
  assert.equal(snapshot.verificationJobs.length,2);assert.ok(snapshot.verificationJobs.every(j=>j.state==='succeeded'));assert.equal(snapshot.desktopAccess.tasks.length,2);assert.ok(snapshot.desktopAccess.tasks.every(t=>t.submitted&&t.proof));
  assert.deepEqual(snapshot.desktopAccess.tasks.map(t=>t.review.id).sort(),[original.id,second.id].sort());
  assert.equal(snapshot.apiJobs.length,0);assert.equal(snapshot.codexJobs.length,0);assert.equal(snapshot.ompJobs.length,0);
  const eventDir=join(data,'workflows/project/events');const events=await Promise.all((await readdir(eventDir)).sort().map(async name=>JSON.parse(await readFile(join(eventDir,name),'utf8'))));
  const seq=(task,kind)=>events.find(e=>e.body.event.command.task===task&&e.body.event.command.kind===kind).body.event.sequence;
  for(const task of ['task','second']){assert.ok(seq(task,'assign')<seq(task,'dispatch-intent'));assert.ok(seq(task,'submit')<seq(task,'verify'));assert.ok(seq(task,'verify')<seq(task,'accept'));}
  assert.ok(seq('task','accept')<seq('second','assign'));
  await tasks.getByRole('heading',{name:'已独立验收并收口',exact:true}).nth(1).waitFor();await tasks.screenshot({path:join(report,'accepted.png')});
  await close();await stopCore();await until(()=>server.exitCode!==null,'test core stopped');assert.equal(server.exitCode,0,serverLog);
  await open();const restored=await request({type:'snapshot'});assert.deepEqual(restored.desktopAccess.tasks,snapshot.desktopAccess.tasks);assert.deepEqual(restored.projectRuns,snapshot.projectRuns);assert.equal(restored.workflows[0].state.tasks.task.state,'accepted');assert.equal(restored.managementDecisions.length,5);
  await page.getByRole('region',{name:'项目持续执行',exact:true}).getByText('项目已收口',{exact:true}).waitFor();await page.getByRole('region',{name:'桌面编程任务',exact:true}).screenshot({path:join(report,'restored.png')});assert.deepEqual(errors,[]);
  await close();await stopCore();await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'actual Tauri/WebView/IPC chief plan, user approval and run authorization, deputy assignment, two separately confirmed desktop sends with persisted history continuity, actual Node verification, chief acceptance and production-core reopen; model/UI transports synthetic',chiefPlan:true,userApproval:true,deputyAssignment:true,waitsForUserSend:true,pauseRejectsStaleSend:true,independentNodeVerification:true,chiefAcceptance:true,managementRequests:5,taskSends:2,continuousHistory:true,secondTaskRequiresSeparateReview:true,gracefulCoreRestart:true,externalMessages:0,realModelCalls:0,pageErrors:errors},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally{await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server])if(child&&child.exitCode===null&&child.signalCode===null&&!child.killed){const exit=once(child,'exit');child.kill();await exit;}}
