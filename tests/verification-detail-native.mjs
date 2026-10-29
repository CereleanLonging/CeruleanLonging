import {chromium} from 'playwright-core';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
const mode=process.argv[2]??'success';
const recover=process.argv.includes('--recover');
assert.ok(!recover || mode==='unknown');
let recovered=false;
assert.ok(['success','second-fail','timeout','unknown'].includes(mode));
const dir=await mkdtemp(resolve('reports/verification-detail-native-'));
const runtime=resolve('target/debug/cerulean-core.exe');
const seeded=spawnSync('cargo',['test','-p','cerulean-core','--test','strict_workflow','seed_native_verification_details','--locked','--','--ignored','--exact','--nocapture'],{
  windowsHide:true,encoding:'utf8',timeout:120000,env:{...process.env,CERULEAN_DETAIL_NATIVE_DIR:dir,CERULEAN_DETAIL_NATIVE_MODE:mode}});
await writeFile(join(dir,'fixture-build.log'),seeded.stdout+'\n'+seeded.stderr);
assert.equal(seeded.status,0,seeded.stdout+seeded.stderr);
const fixture=JSON.parse(await readFile(join(dir,'fixture.json'),'utf8')),data=fixture.data;
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
async function inspect(filename){
  await page.getByRole('button',{name:'查看阶段结果',exact:true}).click();
  const dialog=page.getByRole('dialog',{name:'验收阶段结果'});
  await dialog.getByRole('heading',{name:'语法检查',exact:true}).waitFor();
  assert.deepEqual(await dialog.locator('ol > li > h3').allTextContents(),['语法检查','单元测试']);
  const expected={success:['检查通过','检查通过'],'second-fail':['检查通过','检查失败'],timeout:['检查超时','未运行（前项超时）'],unknown:['已有观察，待整体验收确认','已有观察，待整体验收确认']}[recovered?'success':mode];
  for(let i=0;i<2;i++)assert.ok((await dialog.locator('ol > li').nth(i).innerText()).includes(expected[i]));
  if(mode==='unknown'&&!recovered)await dialog.getByText('验收待对账',{exact:true}).waitFor();
  await dialog.screenshot({path:join(dir,filename)});
  await dialog.locator('summary').first().click();
  await dialog.getByText('标准输出',{exact:true}).first().waitFor();
  await dialog.screenshot({path:join(dir,'logs-'+filename)});
  await dialog.getByRole('button',{name:'关闭详情',exact:true}).click();
}
try{
  await startCore();await launch();const before=await snapshot();
  await inspect('stages.png');const after=await snapshot();
  assert.equal(after.workflows[0].sequence,before.workflows[0].sequence);
  assert.equal(after.verificationDetail,null,'ordinary snapshot does not carry old detail');
  assert.equal(after.codexJobs.length,0);
  if(recover){
    await page.getByRole('button',{name:'查看阶段结果',exact:true}).click();
    const dialog=page.getByRole('dialog',{name:'验收阶段结果'});
    await dialog.getByRole('button',{name:'审阅补记结果',exact:true}).click();
    const confirmation=page.getByRole('region',{name:'补记验收结果确认'});
    await confirmation.waitFor();
    await confirmation.screenshot({path:join(dir,'recovery-confirm.png')});
    await confirmation.getByRole('button',{name:'返回阶段结果',exact:true}).click();
    assert.equal((await snapshot()).workflows[0].sequence,before.workflows[0].sequence);
    await dialog.getByRole('button',{name:'审阅补记结果',exact:true}).click();
    await confirmation.getByRole('button',{name:'确认补记原结果',exact:true}).click();
    await dialog.getByText('验收检查通过',{exact:true}).waitFor();
    const recorded=await snapshot();
    assert.equal(recorded.workflows[0].sequence,before.workflows[0].sequence+1);
    assert.equal(recorded.workflows[0].state.tasks.task.state,'submitted');
    assert.equal(recorded.workflows[0].state.tasks.task.verified,true);
    assert.equal(recorded.codexJobs.length,0);
    assert.equal(recorded.managementDecisions.length,0);
    recovered=true;
    await dialog.screenshot({path:join(dir,'recovery-recorded.png')});
    await dialog.getByRole('button',{name:'关闭详情',exact:true}).click();
  }
  await close();await stopCore();await startCore();await launch();
  await inspect('stages-reopened.png');const reopened=await snapshot();
  assert.equal(reopened.workflows[0].sequence,before.workflows[0].sequence+(recover?1:0));
  assert.equal(reopened.verificationJobs[0].state,recover?'succeeded':before.verificationJobs[0].state);
  assert.equal(reopened.managementDecisions.length,0);
  assert.deepEqual(errors,[]);await close();
  const result={at:new Date().toISOString(),status:'passed',mode,recover,realModelCalls:0,scope:'native stage details and optional explicit recovery, real validators, synthetic submission and report publication gap; full core restart',checks:{orderedResults:true,logsReferenced:true,unknownNotPassedBeforeRecovery:mode==='unknown',noQueryMutation:true,explicitRecoveryWithoutAccept:recover,fullCoreRestart:true,defaultSnapshotClearsDetail:true}};
  await writeFile(join(dir,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({dir,...result}));
}catch(error){await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;
}finally{await browser?.close().catch(()=>{});app?.kill();await stopCore();await writeFile(join(dir,'cleanup.json'),JSON.stringify({complete:true,ownedCoreStopped:!core,cleanups},null,2));}
