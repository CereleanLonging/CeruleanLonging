// Exercise the product's explicit revision/approval flow. Chief/worker fixtures
// are synthetic; this never requests a model or starts a worker.
import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
const dir=await mkdtemp(resolve('reports/project-revision-native-'));
const pending=process.argv.includes('--pending-dependent');
const submitted=process.argv.includes('--submitted');
const data=join(dir,'data'),runtime=resolve('target/debug/cerulean-core.exe');
function fixture(name){
  const result=spawnSync('cargo',['test','-p','cerulean-core','--lib',`host::project_revision::tests::${name}`,'--locked','--','--ignored','--exact','--nocapture'],{
    windowsHide:true,encoding:'utf8',timeout:120000,env:{...process.env,CERULEAN_REVISION_NATIVE_DIR:dir,...(pending?{CERULEAN_REVISION_PENDING:'1'}:{}),...(submitted?{CERULEAN_REVISION_SUBMITTED:'1'}:{})}});
  assert.equal(result.status,0,result.stdout+result.stderr);
}
if(submitted){
  const seed=spawnSync('cargo',['test','-p','cerulean-core','--lib','host::project_dag_tests::two_task_production_pipeline_with_multichecks_and_restarts','--locked','--','--ignored','--exact','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:120000,env:{...process.env,CERULEAN_DAG_DIR:dir,CERULEAN_DAG_MODE:'fail-a-unit',CERULEAN_DAG_EXE:resolve('target/debug/examples/codex_dag_fixture.exe')}});
  await writeFile(join(dir,'seed.log'),seed.stdout+'\n'+seed.stderr);
  assert.equal(seed.status,0,seed.stdout+seed.stderr);
}else fixture('seed_native_terminal_revision');
const reviseButton=submitted?'结束本版提交并修订':'修订下一版方案';
const eventsPath=join(data,'workflows/project/events');
const originalEvents=await Promise.all((await readdir(eventsPath)).map(async name=>({name,bytes:await readFile(join(eventsPath,name),'utf8')})));
let core,app,browser,page;const errors=[],cleanups=[];
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function startCore(){
  core=spawn(runtime,['--serve','--data-dir',data],{windowsHide:true,stdio:'ignore'});
  for(let i=0;i<80;i++){
    assert.equal(core.exitCode,null);
    const query=spawnSync(runtime,['--request','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:2000,input:JSON.stringify({id:'ready',protocolVersion:1,command:{type:'snapshot'}})});
    if(query.status===0)return;
    await delay(100);
  }
  throw Error('owned core not ready');
}
async function stopCore(){
  if(!core)return;
  const owned=core;
  if(owned.exitCode===null&&owned.signalCode===null){
    const exited=once(owned,'exit');
    const result=spawnSync(runtime,['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:10000});
    if(result.status!==0)owned.kill();
    await Promise.race([exited,delay(8000).then(()=>{throw Error('owned core cleanup timeout');})]);
    cleanups.push({pid:owned.pid,gracefulStatus:result.status,forcedOwnedProcess:result.status!==0});
  }
  core=null;
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
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
}
async function close(){
  if(!app)return;
  const exiting=once(app,'exit');
  await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await Promise.race([exiting,delay(8000).then(()=>{throw Error('window close timeout');})]);
  await browser.close();browser=null;app=null;
}
const snapshot=()=>page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('core_request',{command:{type:'snapshot'}}));
try{
  await startCore();await launch();const before=await snapshot();
  assert.equal(await page.locator('#planning-requirement').isDisabled(),true);
  await page.getByRole('button',{name:reviseButton,exact:true}).click();
  let dialog=page.getByRole('dialog',{name:'修订下一版方案',exact:true});
  if(pending)await dialog.getByText('确认后，旧方案中尚未开始的任务也将停止后续派发。新方案可以保留、调整或移除这些任务，需要重新审阅批准后才能执行。',{exact:true}).waitFor();
  assert.equal(await dialog.getByRole('button',{name:'确认开放修订',exact:true}).isDisabled(),true);
  await dialog.getByRole('button',{name:'返回',exact:true}).click();
  assert.equal((await snapshot()).workflows[0].sequence,before.workflows[0].sequence);
  await page.getByRole('button',{name:reviseButton,exact:true}).click();
  dialog=page.getByRole('dialog',{name:'修订下一版方案',exact:true});
  await dialog.locator('#revision-reason').fill('任务停止后调整需求，保留旧轮次证据');
  if(submitted){
    await dialog.getByRole('region',{name:'结束本版提交确认'}).waitFor();
    assert.equal(await dialog.getByRole('button',{name:'确认开放修订',exact:true}).isDisabled(),true);
    await dialog.getByRole('checkbox',{name:'我确认结束本版未收口的提交'}).check();
  }
  await dialog.screenshot({path:join(dir,'revision-review.png')});
  await dialog.getByRole('button',{name:'确认开放修订',exact:true}).click();
  await page.getByText('已转入修订',{exact:true}).waitFor();
  const requested=await snapshot();
  assert.equal(requested.workflows[0].sequence,before.workflows[0].sequence+1);
  assert.ok(requested.workflows[0].state.revisionRequested);
  assert.equal(requested.projectRuns[0].state,'superseded');
  assert.equal(requested.projectRevisions.length,0);
  if(submitted){assert.equal(requested.workflows[0].state.tasks.a.state,'submitted');assert.equal(requested.workflows[0].state.tasks.a.verified,false);assert.equal(requested.workflows[0].state.tasks.b.state,'pending');assert.equal(requested.verificationJobs[0].state,'failed');}
  if(pending){assert.equal(requested.workflows[0].state.tasks.dependent.state,'pending');assert.ok(requested.workflows[0].state.tasks.dependent.attempt==null);}
  assert.equal(await page.getByRole('button',{name:'恢复项目调度',exact:true}).count(),0);
  await close();await stopCore();await startCore();await launch();
  assert.ok((await snapshot()).workflows[0].state.revisionRequested);
  await page.locator('#planning-requirement').fill('下一版需求：保留原文件并补充行为验证');
  await page.locator('#planning-management-limit').fill('4');
  await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  await page.getByText('配置版本 2',{exact:true}).waitFor();
  await page.getByRole('region',{name:'主指挥项目规划'}).screenshot({path:join(dir,'revised-config.png')});
  await close();await stopCore();fixture('propose_native_terminal_revision');await startCore();await launch();
  let next=await snapshot();assert.equal(next.workflows[0].approval,null);
  assert.equal(next.workflows[0].revision.workflow.version,2);
  assert.equal(next.workflows[0].state.tasks[submitted?'a':'task'].fence,1);
  if(submitted)assert.equal(next.workflows[0].state.tasks.b,undefined);
  if(pending)assert.equal(next.workflows[0].state.tasks.dependent,undefined);
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  dialog=page.getByRole('dialog',{name:'审阅项目执行方案',exact:true});
  await dialog.screenshot({path:join(dir,'new-approval.png')});
  await dialog.getByRole('button',{name:'返回检查',exact:true}).click();
  assert.equal((await snapshot()).workflows[0].approval,null);
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  await page.getByRole('dialog',{name:'审阅项目执行方案',exact:true}).getByRole('button',{name:'确认执行授权',exact:true}).click();
  await page.getByText('执行方案已批准',{exact:true}).waitFor();
  const approved=await snapshot();assert.ok(approved.workflows[0].approval);
  assert.notEqual(approved.workflows[0].revisionDigest,before.workflows[0].revisionDigest);
  await close();await stopCore();await startCore();await launch();next=await snapshot();
  assert.deepEqual(next.workflows[0].approval,approved.workflows[0].approval);
  assert.equal(next.projectRuns.length,1);assert.equal(next.projectRuns[0].state,'superseded');
  assert.equal(next.codexJobs.length,submitted?1:0);assert.equal(next.verificationJobs.length,submitted?1:0);
  assert.equal(next.managementDecisions.length,submitted?3:2);
  if(submitted){
    await page.getByRole('button',{name:'查看阶段结果',exact:true}).click();
    const history=page.getByRole('dialog',{name:'验收阶段结果'});
    await history.getByText('验收未通过',{exact:true}).waitFor();
    assert.ok((await history.locator('ol > li').nth(1).innerText()).includes('检查失败'));
    await history.screenshot({path:join(dir,'old-failure-after-revision.png')});
    await history.getByRole('button',{name:'关闭详情',exact:true}).click();
  }
  for(const event of originalEvents)assert.equal(await readFile(join(eventsPath,event.name),'utf8'),event.bytes);
  assert.deepEqual(errors,[]);await close();
  const result={at:new Date().toISOString(),status:'passed',pendingDependent:pending,submittedRetirement:submitted,realModelCalls:0,scope:'native revision and reapproval; synthetic chief/worker; submitted fixture has real failed validators; no new dispatch during revision',checks:{returnDoesNotRequest:true,reasonRequired:true,explicitSubmissionAcknowledgement:submitted,oldRunClosed:true,restartPendingRevision:true,configurationUnlocked:true,newApprovalRequired:true,fullCoreRestart:true,oldJournalBytesUnchanged:true,oldFailureReadable:submitted,noAutomaticDispatch:true}};
  await writeFile(join(dir,submitted?'native-result.json':'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({dir,...result}));
}catch(error){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;
}finally{await browser?.close().catch(()=>{});app?.kill();await stopCore();await writeFile(join(dir,'cleanup.json'),JSON.stringify({complete:true,ownedCoreStopped:!core,cleanups},null,2));}
