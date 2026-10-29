// Native read-only observation inspection, with synthetic protocol events and actual workers.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
await assertNativeRuntime();
const supervision=process.argv.includes('--supervision');
const clean=process.argv.includes('--clean');
const dir=await mkdtemp(resolve(supervision?'reports/supervision-incidents-native-':'reports/worker-observation-native-'));
const data=join(dir,'data'),runtime=resolve('target/debug/cerulean-core.exe');
const seed=spawnSync('cargo',['test','-p','cerulean-core','--lib','host::project_dag_tests::two_task_production_pipeline_with_multichecks_and_restarts','--locked','--','--ignored','--exact','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:120000,env:{...process.env,CERULEAN_DAG_DIR:dir,CERULEAN_DAG_MODE:'success',CERULEAN_DAG_OBSERVATIONS:'1',...(supervision?{CERULEAN_DAG_SUPERVISION:'1'}:{}),...(clean?{CERULEAN_DAG_OBSERVATIONS_CLEAN:'1'}:{}),CERULEAN_DAG_EXE:resolve('target/debug/examples/codex_dag_fixture.exe')}});
await writeFile(join(dir,'seed.log'),seed.stdout+'\n'+seed.stderr);assert.equal(seed.status,0,seed.stdout+seed.stderr);
const evidence=JSON.parse(await readFile(join(dir,'observations.json'),'utf8'));
let core,app,browser,page;const errors=[],cleanups=[];
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function startCore(){
  core=spawn(runtime,['--serve','--data-dir',data],{windowsHide:true,stdio:'ignore'});
  for(let i=0;i<80;i++){
    assert.equal(core.exitCode,null);
    const query=spawnSync(runtime,['--request','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:2000,input:JSON.stringify({id:'ready',protocolVersion:1,command:{type:'snapshot'}})});
    if(query.status===0)return;await delay(100);
  }throw Error('owned core not ready');
}
async function stopCore(){
  if(!core)return;const owned=core;
  if(owned.exitCode===null&&owned.signalCode===null){
    const exited=once(owned,'exit');
    const result=spawnSync(runtime,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:10000});
    if(result.status!==0)owned.kill();
    await Promise.race([exited,delay(8000).then(()=>{throw Error('owned core cleanup timeout');})]);
    cleanups.push({pid:owned.pid,gracefulStatus:result.status,forcedOwnedProcess:result.status!==0});
  }core=null;
}
async function launch(){
  const listener=createServer().listen(0,'127.0.0.1');await once(listener,'listening');
  const port=listener.address().port;await new Promise(r=>listener.close(r));
  app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:'ignore',env:{...process.env,CERULEAN_TEST_DATA_DIR:data,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  for(let i=0;i<80;i++){assert.equal(app.exitCode,null);try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:800});break;}catch{await delay(100);}}
  assert.ok(browser);
  for(let i=0;i<50;i++){page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));if(page)break;await delay(100);}
  assert.ok(page);page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
  await page.getByRole('button',{name:'工作总览',exact:false}).click();
}
async function close(){
  if(!app)return;const exiting=once(app,'exit');
  await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await Promise.race([exiting,delay(8000).then(()=>{throw Error('window close timeout');})]);
  await browser.close();browser=null;app=null;
}
const request=command=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command);
const snapshot=()=>request({type:'snapshot'});
async function inspect(suffix){
  assert.equal(evidence.length,2);
  for(let i=0;i<evidence.length;i++){
    const expected=evidence[i];
    await page.getByRole('button',{name:`查看项目 ${expected.projectId} 的作业`,exact:true}).click();
    await page.getByRole('button',{name:`查看尝试 ${expected.attemptId} 的执行观察`,exact:true}).click();
    const dialog=page.getByRole('region',{name:'执行观察',exact:true});
    await dialog.getByText('仅覆盖命令执行和文件变更的完成事件，不代表全部执行活动。没有记录不能据此判定卡循环。',{exact:true}).waitFor();
    if(clean)assert.equal(await dialog.getByText('观察证据不完整，不可据此自动纠偏。',{exact:true}).count(),0);
    else await dialog.getByText('观察证据不完整，不可据此自动纠偏。',{exact:true}).waitFor();
    await dialog.getByRole('button',{name:'读取工具观察首页',exact:true}).click();
    await dialog.getByRole('navigation',{name:'工具观察分页',exact:true}).waitFor();
    assert.equal(await dialog.getByRole('heading',{name:'命令执行 · 失败',exact:true}).count(),2);
    assert.equal(await dialog.getByRole('heading',{name:'文件变更 · 已完成',exact:true}).count(),1);
    if(supervision){const incidents=dialog.getByRole('region',{name:'监督异常'});await incidents.getByRole('button',{name:'读取监督异常首页',exact:true}).click();await incidents.getByRole('navigation',{name:'监督异常分页',exact:true}).waitFor();assert.equal(await incidents.getByText('连续命令失败',{exact:true}).count(),1);await incidents.getByText('事件 1–2',{exact:true}).waitFor();}
    assert.ok(!(await dialog.innerText()).includes('private-observation-sentinel'));
    await dialog.getByRole('button',{name:'重新读取观察',exact:true}).click();
    await dialog.getByText('仅覆盖命令执行和文件变更的完成事件，不代表全部执行活动。没有记录不能据此判定卡循环。',{exact:true}).waitFor();
    if(clean)assert.equal(await dialog.getByText('观察证据不完整，不可据此自动纠偏。',{exact:true}).count(),0);
    else await dialog.getByText('观察证据不完整，不可据此自动纠偏。',{exact:true}).waitFor();
    await dialog.screenshot({path:join(dir,`${suffix}-${i}.png`)});
    await dialog.getByRole('button',{name:'关闭观察',exact:true}).click();
    await page.getByRole('button',{name:'关闭项目作业',exact:true}).click();
  }
  for(const expected of evidence){
    const read=await request({type:'readWorkerObservation',projectId:expected.projectId,attemptId:expected.attemptId});
    assert.equal(read.workerObservation.evidenceDigest,expected.evidenceDigest);assert.deepEqual(read.workerObservation.incidents??[],expected.incidents??[]);
  }
}
try{
  await startCore();await launch();const before=await snapshot();await inspect('observations');
  const after=await snapshot();assert.equal(after.workerObservation,null);
  assert.deepEqual(after.workflows,before.workflows);assert.deepEqual(after.codexJobs,before.codexJobs);
  assert.deepEqual(after.managementDecisions,before.managementDecisions);
  await close();await stopCore();await startCore();await launch();await inspect('reopened');
  const reopened=await snapshot();assert.deepEqual(reopened.workflows,before.workflows);
  assert.deepEqual(reopened.codexJobs,before.codexJobs);assert.deepEqual(errors,[]);
  await close();await stopCore();
  const result={at:new Date().toISOString(),status:'passed',realModelCalls:0,scope:'synthetic protocol metadata; actual worker and validator processes, native Tauri UI, complete core restart',checks:{supervision,clean,stableIncidentAfterRestart:supervision,twoTaskDetails:true,gapWarningMatchesEvidence:true,noPrivatePayload:true,readOnly:true,fullRestartSameEvidence:true},cleanups};
  await writeFile(join(dir,'native-result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({dir,...result}));
}catch(error){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;
}finally{await browser?.close().catch(()=>{});app?.kill();await stopCore();await writeFile(join(dir,'cleanup.json'),JSON.stringify({complete:true,ownedCoreStopped:!core,cleanups},null,2));}
