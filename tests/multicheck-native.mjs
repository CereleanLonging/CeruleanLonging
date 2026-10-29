// Product configuration/approval with synthetic chief output; no model calls.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';
import {assertNativeRuntime} from './native-runtime.mjs';
await assertNativeRuntime();
const delivery=process.argv.includes('--delivery');
const supervision=delivery||process.argv.includes('--supervision');
const dir = await mkdtemp(resolve(supervision?'reports/supervision-policy-native-':'reports/multicheck-native-'));
const data = join(dir, 'data');
function fixture(name) {
  const result = spawnSync('cargo', ['test', '-p', 'cerulean-core', '--lib', name, '--locked', '--', '--ignored', '--nocapture'], {
    windowsHide: true, encoding: 'utf8', timeout: 120000,
    env: {...process.env, CERULEAN_MULTICHECK_NATIVE_DIR: dir},
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
}
fixture('native_multicheck_configuration_fixture');
const legacyPath = join(data, 'project-setups/project.00000001.json');
const legacyBytes = await readFile(legacyPath, 'utf8');
let app, browser, page; const errors = [];
async function launch() {
  const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
  const port = listener.address().port; await new Promise(r => listener.close(r));
  app = spawn(resolve('target/debug/cerulean-desktop.exe'), [], {windowsHide:true, stdio:'ignore', env:{...process.env,
    CERULEAN_TEST_DATA_DIR:data, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  for(let i=0;i<80;i++) {
    assert.equal(app.exitCode,null);
    try {browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1000});break;} catch {await new Promise(r=>setTimeout(r,150));}
  }
  assert.ok(browser);
  for(let i=0;i<50;i++) {
    page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().includes('tauri.localhost'));
    if(page)break;await new Promise(r=>setTimeout(r,150));
  }
  assert.ok(page);page.on('pageerror',e=>errors.push(e.message));
  await page.getByText('本地核心已连接',{exact:true}).waitFor({timeout:20000});
  await page.getByRole('button',{name:'需求与计划',exact:false}).click();
}
async function close() {
  const exiting=once(app,'exit');
  await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'main'})).catch(()=>{});
  await Promise.race([exiting,new Promise((_,reject)=>setTimeout(()=>reject(Error('window close timed out')),8000).unref())]);
  await browser.close();browser=null;app=null;
}
function stopCore() {
  const result=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});
  assert.equal(result.status,0,result.stderr);
}
const snapshot=()=>page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('core_request',{command:{type:'snapshot'}}));
try {
  await launch();
  const form=page.getByRole('region',{name:'主指挥项目规划'});
  await form.locator('#stage-name-0').fill('语法检查');
  await form.locator('#stage-timeout-0').fill('40');
  await form.getByRole('button',{name:'添加验收检查',exact:true}).click();
  await form.locator('#stage-name-1').fill('单元测试');
  await form.locator('#stage-timeout-1').fill('100');
  const save=form.getByRole('button',{name:'保存需求与执行配置',exact:true});
  assert.equal(await save.isDisabled(),true,'sum exceeds approved total');
  await form.locator('#stage-timeout-1').fill('60');
  await form.locator('#stage-args-1').fill('--list');
  await form.getByRole('button',{name:'上移',exact:true}).nth(1).click();
  assert.equal(await form.locator('#stage-name-0').inputValue(),'单元测试');
  await form.getByRole('button',{name:'添加验收检查',exact:true}).click();
  await form.getByRole('button',{name:'删除检查',exact:true}).nth(2).click();
  if(supervision){
    assert.equal(await form.getByRole('checkbox',{name:'记录连续命令失败'}).isChecked(),false);
    await form.getByRole('checkbox',{name:'记录连续命令失败'}).check();
    assert.equal(await form.locator('#supervision-advice-limit').inputValue(),'0');
    await form.locator('#supervision-threshold').fill('3');
    await form.locator('#supervision-advice-limit').fill('2');
    assert.equal(await form.getByRole('checkbox',{name:'允许向当前执行轮次发送建议'}).isChecked(),false);
    if(delivery)await form.getByRole('checkbox',{name:'允许向当前执行轮次发送建议'}).check();
    await form.getByRole('region',{name:'异常监督配置'}).screenshot({path:join(dir,'supervision-config.png')});
  }
  await save.click(); await form.getByText('配置版本 2',{exact:true}).waitFor();
  let snap=await snapshot();
  const stages=snap.projectSetups[0].setup.acceptance.stages;
  if(supervision)assert.deepEqual(snap.projectSetups[0].setup.policy.supervision,{schemaVersion:1,repeatedFailureThreshold:3,maxAdvicePerAttempt:2,...(delivery?{deliveryEnabled:true}:{})});
  assert.deepEqual(stages.map(s=>s.check.name),['单元测试','语法检查']);
  assert.deepEqual(stages.map(s=>s.check.timeoutSeconds),[60,40]);
  assert.equal(snap.workflows[0].revision,null);assert.equal(snap.codexJobs.length,0);assert.equal(snap.verificationJobs.length,0);
  assert.equal(snap.managementDecisions.length,0);
  await form.screenshot({path:join(dir,'ordered-config.png')});
  await close();stopCore();fixture('native_multicheck_proposal_fixture');await launch();
  assert.equal(await page.locator('#stage-name-0').inputValue(),'单元测试');
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  let dialog=page.getByRole('dialog',{name:'审阅项目执行方案'});
  assert.deepEqual(await dialog.locator('ol > li > strong').allTextContents(),['单元测试','语法检查']);
  if(supervision){await dialog.getByText('同一命令连续失败 3 次记录异常；每次尝试建议最多 2 条，仍受项目累计请求上限约束。'+(delivery?'允许自动请求、保存并向原执行轮次发送建议；未知回执不重发。':'达到阈值时可自动请求并保存建议，当前不会向执行器发送。'),{exact:true}).waitFor();await dialog.locator('dl').first().screenshot({path:join(dir,'supervision-approval.png')});}
  await dialog.getByText('按顺序完成全部验收检查：',{exact:true}).waitFor();
  await dialog.locator('ol').scrollIntoViewIfNeeded();
  await dialog.locator('ol').screenshot({path:join(dir,'approval-checks.png')});
  await dialog.getByRole('button',{name:'返回检查',exact:true}).click();
  snap=await snapshot();assert.equal(snap.workflows[0].approval,null);
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  dialog=page.getByRole('dialog',{name:'审阅项目执行方案'});
  await dialog.getByRole('button',{name:'确认执行授权',exact:true}).click();
  await page.getByText('执行方案已批准',{exact:true}).waitFor();
  const approved=await snapshot();assert.ok(approved.workflows[0].approval);
  assert.deepEqual(approved.workflows[0].revision.execution[0].checks,stages.map(s=>s.check));
  await close();stopCore();await launch();
  const reopened=await snapshot();
  assert.deepEqual(reopened.workflows[0].approval,approved.workflows[0].approval);
  if(supervision){assert.deepEqual(reopened.workflows[0].revision.policy.supervision,approved.workflows[0].revision.policy.supervision);assert.equal(await page.locator('#supervision-threshold').inputValue(),'3');assert.equal(await page.locator('#supervision-threshold').isDisabled(),true);}
  assert.equal(reopened.codexJobs.length,0);assert.equal(reopened.verificationJobs.length,0);
  assert.equal(reopened.projectRuns.length,0);assert.equal(reopened.managementDecisions.length,1);
  assert.equal(await readFile(legacyPath,'utf8'),legacyBytes);
  assert.deepEqual(errors,[]);await close();
  const result={at:new Date().toISOString(),status:'passed',scope:'native product multistage configuration and approval; synthetic chief proposal; no execution',realModelCalls:0,
    checks:{delivery,supervision,supervisionApprovalAndRestart:supervision,addRemoveReorder:true,totalTimeLimit:true,persistedTwoChecks:true,approvalDisplaysOrderAndNames:true,returnDoesNotApprove:true,fullCoreRestart:true,legacyBytesUnchanged:true,noWorkerOrValidatorDispatch:true}};
  await writeFile(join(dir,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({dir,...result}));
} catch(error) {
  await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;
} finally {
  await browser?.close().catch(()=>{});app?.kill();
  const stopped=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});
  await writeFile(join(dir,'cleanup.json'),JSON.stringify({exitCode:stopped.status,stderr:stopped.stderr}));
}
