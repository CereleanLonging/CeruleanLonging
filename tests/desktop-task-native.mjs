// Native product UI/IPC/persistence with synthetic external-window observations.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/desktop-task-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});assert.equal(build.status,0,build.stderr);
const exe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(exe);
const server=spawn(exe,['--exact','host::desktop_sessions::tests::native_desktop_task_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_DESKTOP_NATIVE_DIR:report}});
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

try{
  const fixture=await until(async()=>{assert.equal(server.exitCode,null,serverLog);try{return JSON.parse(await readFile(join(report,'fixture.json'),'utf8'));}catch{return false;}},'fixture');
  data=fixture.data;await sleep(300);await open();
  let region=page.getByRole('region',{name:'桌面编程任务',exact:true});
  await region.getByRole('button',{name:'准备任务消息',exact:true}).click();
  await region.getByRole('button',{name:'审阅编程任务',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'确认发送编程任务',exact:true});await dialog.waitFor();
  const confirm=dialog.getByRole('button',{name:'确认发送任务一次',exact:true});assert.equal(await confirm.isDisabled(),true);
  const before=await request({type:'snapshot'});const reviewed=before.desktopAccess.taskReview;
  assert.equal(before.desktopAccess.tasks.length,0);assert.ok(reviewed.prompt.includes('sum.mjs'));
  assert.ok((await dialog.innerText()).includes(reviewed.attempt.binding.workspace));
  await dialog.getByRole('button',{name:'返回检查',exact:true}).click();assert.equal((await request({type:'snapshot'})).desktopAccess.tasks.length,0);
  await region.getByRole('button',{name:'审阅编程任务',exact:true}).click();await dialog.getByRole('checkbox').check();
  await dialog.screenshot({path:join(report,'task-review.png')});await confirm.click();await dialog.waitFor({state:'hidden'});
  await until(async()=>{const s=await request({type:'snapshot'});return s.desktopAccess.tasks.some(t=>t.active);},'background owns workflow');
  const running=await request({type:'snapshot'});assert.equal(running.workflows.length,1);assert.equal(running.workflows[0].state.tasks.task.state,'dispatching');
  assert.equal(running.desktopAccess.tasks.length,1);assert.ok(running.desktopAccess.tasks[0].active);
  await assert.rejects(request({type:'sendDesktopTask',reviewDigest:reviewed.id}));
  await assert.rejects(request({type:'readDesktopTask',projectId:'project',attemptId:'desktop-attempt'}));
  await page.getByRole('button',{name:'接入管理',exact:false}).click();await page.getByRole('heading',{name:'Qoder / WorkBuddy 已有会话',exact:true}).waitFor();
  assert.equal(await page.getByRole('button',{name:'发现已打开窗口',exact:true}).isDisabled(),true);
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();await region.screenshot({path:join(report,'task-background.png')});
  const windowExit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await windowExit;await browser.close();browser=null;await open();region=page.getByRole('region',{name:'桌面编程任务',exact:true});
  assert.ok((await request({type:'snapshot'})).desktopAccess.tasks[0].active,'window closure retains resident task');
  await writeFile(join(report,'project/release-send'),'release');
  await until(async()=>{const s=await request({type:'snapshot'});if(!s.desktopAccess.checking&&s.desktopAccess.error)throw Error(s.desktopAccess.error);return s.desktopAccess.tasks[0]?.submitted;},'sealed submission');
  const sent=await request({type:'snapshot'});assert.equal(sent.workflows[0].state.tasks.task.state,'submitted');assert.equal(sent.workflows[0].state.tasks.task.verified,false);
  assert.ok(sent.desktopAccess.tasks[0].proof);assert.equal(sent.desktopAccess.taskReview,null);assert.equal(sent.projectRuns.length,0);
  await region.getByRole('heading',{name:'交付已提交，等待验收',exact:true}).waitFor();await region.screenshot({path:join(report,'task-submitted.png')});
  await close();const fixtureExit=once(server,'exit');server.kill();await fixtureExit; // Owned test process only; Submitted correctly forbids graceful service stop.
  await open();const restored=await request({type:'snapshot'});assert.deepEqual(restored.desktopAccess.tasks,sent.desktopAccess.tasks);assert.equal(restored.desktopAccess.checking,false);
  assert.equal(restored.workflows[0].state.tasks.task.state,'submitted');await page.getByRole('heading',{name:'交付已提交，等待验收',exact:true}).waitFor();
  await page.getByRole('region',{name:'桌面编程任务',exact:true}).screenshot({path:join(report,'task-restored.png')});assert.deepEqual(errors,[]);
  await close();await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'actual Tauri/WebView/IPC with test-core synthetic management and desktop observations; durable dispatch, real local artifact sealing, production-core reopen',confirmationRequired:true,returnWithoutSend:true,duplicateSendRejected:true,backgroundSnapshotResponsive:true,windowReopenWhileActive:true,coreRestartMode:'terminate owned test core, reopen production core',restoredWithoutReplay:true,independentVerificationPerformed:false,externalMessages:0,modelCalls:0,pageErrors:errors},null,2));console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog,snapshot:await request({type:'snapshot'}).catch(()=>null)},null,2));throw error;}
finally{await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server])if(child&&child.exitCode===null&&child.signalCode===null&&!child.killed){const exit=once(child,'exit');child.kill();await exit;}}
