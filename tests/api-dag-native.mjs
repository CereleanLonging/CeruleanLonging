// Actual desktop/IPC/scheduler/file tools/validator. Only model transports are synthetic.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join,toNamespacedPath} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
await assertNativeRuntime();
const report=await mkdtemp(resolve('reports/api-dag-native-'));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});
assert.equal(build.status,0,build.stderr);
const testExe=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;
assert.ok(testExe);
const desktop=resolve('target/debug/cerulean-desktop.exe'),core=resolve('target/debug/cerulean-core.exe');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label){for(let n=0;n<450;n++){const value=await fn();if(value)return value;await sleep(100);}throw Error('Timed out: '+label);}
const server=spawn(testExe,['--exact','host::api_run_tests::native_api_dag_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_API_DAG_NATIVE_DIR:report}});
let serverLog='';server.stdout.on('data',b=>serverLog+=b);server.stderr.on('data',b=>serverLog+=b);
let app,browser,page;const errors=[];
const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
try {
  const fixture=await until(async()=>{assert.equal(server.exitCode,null,serverLog);try{return JSON.parse(await readFile(join(report,'fixture.json'),'utf8'));}catch{return false;}},'test service setup');
  const data=fixture.data;await sleep(300);
  const portServer=createServer().listen(0,'127.0.0.1');await once(portServer,'listening');const port=portServer.address().port;await new Promise(r=>portServer.close(r));
  app=spawn(desktop,[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(report,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  await until(async()=>{assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});return true;}catch{return false;}},'WebView');
  page=await until(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost')),'page');page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
  let snapshot=await request({type:'snapshot'});
  assert.equal(toNamespacedPath(resolve(snapshot.dataDirectory)),toNamespacedPath(resolve(data)),'isolated fixture only');
  assert.equal(snapshot.projectRuns.length,0);assert.equal(snapshot.apiJobs.length,0);
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
  await page.getByRole('button',{name:'请求主指挥生成方案',exact:true}).click();
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  const review=page.getByRole('dialog',{name:'审阅项目执行方案',exact:true});await review.waitFor();
  await review.screenshot({path:join(report,'plan-review.png')});
  snapshot=await request({type:'snapshot'});assert.equal(snapshot.apiJobs.length,0);assert.equal(snapshot.projectRuns.length,0);
  assert.equal(snapshot.workflows[0].revision.workflow.tasks.length,2);
  await review.getByRole('button',{name:'确认执行授权',exact:true}).click();await review.waitFor({state:'hidden'});
  await page.getByRole('button',{name:'启动已批准项目',exact:true}).click();
  const start=page.getByRole('dialog',{name:'启动持续项目执行',exact:true});await start.waitFor();
  await start.screenshot({path:join(report,'start-review.png')});
  await start.getByRole('button',{name:'确认启动调度',exact:true}).click();await start.waitFor({state:'hidden'});
  // Observe UI polling itself, without driving the core through test-side snapshot polling.
  await page.getByRole('region',{name:'项目持续执行',exact:true}).getByText('项目已收口',{exact:true}).waitFor({timeout:60000});
  snapshot=await request({type:'snapshot'});
  assert.equal(snapshot.projectRuns[0].state,'completed');
  assert.ok(Object.values(snapshot.workflows[0].state.tasks).every(t=>t.state==='accepted'));
  assert.equal(snapshot.verificationJobs.length,2);assert.ok(snapshot.verificationJobs.every(j=>j.state==='succeeded'));
  assert.equal(snapshot.apiJobs.length,2);
  assert.equal(await readFile(join(report,'project/result.txt'),'utf8'),'alpha beta');
  const eventDir=join(data,'workflows/project/events');
  const events=await Promise.all((await readdir(eventDir)).sort().map(async name=>JSON.parse(await readFile(join(eventDir,name),'utf8'))));
  const seq=(task,kind)=>events.find(e=>e.body.event.command.task===task&&e.body.event.command.kind===kind).body.event.sequence;
  for(const task of ['a','b']){assert.ok(seq(task,'submit')<seq(task,'verify'));assert.ok(seq(task,'verify')<seq(task,'accept'));}
  assert.ok(seq('a','accept')<seq('b','assign'));
  await page.getByRole('region',{name:'项目持续执行',exact:true}).screenshot({path:join(report,'completed.png')});
  assert.deepEqual(errors,[]);
  const exit=once(app,'exit');await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>reject(Error('owned window exit timeout')),8000).unref())]);await browser.close();browser=null;
  const stopped=spawnSync(core,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:25000});assert.equal(stopped.status,0,stopped.stderr);
  await until(()=>server.exitCode!==null,'fixture service exit');assert.equal(server.exitCode,0,serverLog);
  await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'actual Tauri/WebView, resident IPC, continuous two-task dependent API run; model transports synthetic',tasks:2,independentValidators:2,chiefPlan:true,userApproval:true,deputyAssignments:true,chiefAcceptance:true,dependencyOrder:true,uiPolling:true,externalProviderRequests:0,realModelCalls:0,gracefulServiceStop:true,pageErrors:errors},null,2));
  console.log(JSON.stringify({report,status:'passed'}));
}catch(error){await page?.screenshot({path:join(report,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(report,'failure.json'),JSON.stringify({error:String(error),errors,serverLog},null,2));throw error;}
finally{await writeFile(join(report,'service.log'),serverLog);await browser?.close().catch(()=>{});for(const child of [app,server])if(child&&child.exitCode===null){const exit=once(child,'exit');child.kill();await exit;}}
