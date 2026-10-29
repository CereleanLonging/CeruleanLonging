// Isolated paused authorization fixture; real native UI/core, no provider calls.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';
const dir=await mkdtemp(resolve('reports/project-runs-native-'));
const data=join(dir,'data');
const seeded=spawnSync('cargo',['test','-p','cerulean-core','seed_native_paused_run_fixture','--locked','--','--ignored','--nocapture'],{
  windowsHide:true,encoding:'utf8',timeout:120000,env:{...process.env,CERULEAN_RUN_NATIVE_DIR:dir},
});
assert.equal(seeded.status,0,seeded.stderr);
let app,browser,page;
const errors=[];
async function launch(){
  const listener=createServer();listener.listen(0,'127.0.0.1');await once(listener,'listening');
  const port=listener.address().port;await new Promise(r=>listener.close(r));
  app=spawn(resolve('target/debug/cerulean-desktop.exe'),[],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_TEST_DATA_DIR:data,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  app.stderr.on('data',b=>{if(b.toString().includes('panicked'))errors.push(b.toString());});
  for(let i=0;i<80;i++){
    assert.equal(app.exitCode,null,'native app remains running');
    try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1000});break;}catch{await new Promise(r=>setTimeout(r,250));}
  }
  assert.ok(browser);
  for(let i=0;i<50;i++){
    page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));
    if(page)break;await new Promise(r=>setTimeout(r,200));
  }
  assert.ok(page);page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
}
async function close(){
  const exiting=once(app,'exit');
  await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await Promise.race([exiting,new Promise((_,reject)=>setTimeout(()=>reject(Error('native close timed out')),8000).unref())]);
  await browser.close();browser=null;app=null;
}
async function snapshot(){return page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('core_request',{command:{type:'snapshot'}}));}
try{
  await launch();
  const section=page.getByRole('region',{name:'项目持续执行'});
  await section.getByText('已暂停后续调度',{exact:true}).waitFor();
  assert.equal(await section.getByRole('button',{name:'恢复项目调度'}).isDisabled(),true);
  const initial=await snapshot();
  assert.equal(initial.projectRuns[0].state,'paused');assert.equal(initial.managementDecisions.length,1);
  await page.getByRole('button',{name:'接入管理',exact:false}).click();
  await page.getByLabel('OpenAI API Key',{exact:true}).fill('sk-native-run-fixture-never-real');
  await page.getByRole('button',{name:'加密保存 Key',exact:true}).click();
  await page.getByText('已加密保存，待检查',{exact:true}).waitFor();
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
  await section.getByRole('button',{name:'恢复项目调度'}).click();
  const dialog=page.getByRole('dialog',{name:'启动持续项目执行'});
  await dialog.waitFor();await dialog.getByText(/请求次数不是金额上限/).waitFor();
  await page.screenshot({path:join(dir,'run-review.png'),fullPage:true});
  await dialog.getByRole('button',{name:'返回',exact:true}).click();
  const after=await snapshot();
  assert.equal(after.projectRuns[0].state,'paused');assert.equal(after.managementDecisions.length,1);
  assert.equal(after.codexJobs.length,0);
  await page.getByRole('button',{name:'接入管理',exact:false}).click();
  await page.getByRole('button',{name:'删除本机 Key',exact:true}).click();
  await page.getByText('尚未配置',{exact:true}).waitFor();
  await close();await launch();
  await page.getByRole('region',{name:'项目持续执行'}).getByText('已暂停后续调度',{exact:true}).waitFor();
  const reopened=await snapshot();
  assert.equal(reopened.projectRuns[0].state,'paused');assert.equal(reopened.managementDecisions.length,1);
  await page.getByRole('region',{name:'项目持续执行'}).screenshot({path:join(dir,'paused-reopened.png')});
  assert.deepEqual(errors,[]);await close();
  const result={at:new Date().toISOString(),status:'passed',scope:'native UI and core with offline synthetic approved/paused fixture',checks:{pausedAuthorizationDisplayed:true,missingKeyDisablesResume:true,explicitStartReview:true,reviewCancelDoesNotStart:true,pauseSurvivesReopen:true,noNewManagementIntent:true,noWorkerDispatch:true},realModelCalls:0};
  await writeFile(join(dir,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({dir,...result},null,2));
}catch(error){
  await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;
}finally{
  await browser?.close().catch(()=>{});app?.kill();
  const stopped=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});
  if(stopped.status!==0)console.error('Isolated test cleanup:',stopped.stderr);
}
