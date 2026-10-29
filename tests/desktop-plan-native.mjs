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
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const dir=await mkdtemp(resolve('reports/desktop-plan-native-'));
const data = join(dir, 'data');
function fixture(name) {
  const result = spawnSync('cargo', ['test', '-p', 'cerulean-core', '--lib', name, '--locked', '--', '--ignored', '--nocapture'], {
    windowsHide: true, encoding: 'utf8', timeout: 120000,
    env: {...process.env, CERULEAN_DESKTOP_PLAN_NATIVE_DIR: dir},
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
}
fixture('native_desktop_plan_configuration_fixture');
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
  const initial=await snapshot();
  for(const adapter of ['workbuddy','qoder']) {
    await form.locator('#planning-adapter').selectOption(adapter);
    assert.equal(await form.locator('#planning-executable').count(),0);
    const binding=initial.desktopAccess.bindings.find(b=>b.observation.window.app===adapter);
    await form.locator('#planning-desktop-binding').selectOption(binding.id);
    const region=form.getByRole('region',{name:'桌面编程会话配置'});
    const a=region.getByRole('checkbox').nth(0),b=region.getByRole('checkbox').nth(1);
    assert.equal(await a.isChecked(),false);assert.equal(await b.isChecked(),false);
    assert.equal(await form.locator('form').evaluate(el=>el.checkValidity()),false);
    await a.check();assert.equal(await form.locator('form').evaluate(el=>el.checkValidity()),false);
    await b.check();assert.equal(await form.locator('form').evaluate(el=>el.checkValidity()),true);
    if(adapter==='workbuddy')await region.getByText('WorkBuddy内容指纹与用户关联；没有供应商会话ID',{exact:true}).waitFor();
    await region.screenshot({path:join(dir,adapter+'-config.png')});
    await form.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
    await form.getByText(`配置版本 ${adapter==='workbuddy'?2:3}`,{exact:true}).waitFor();
    const saved=await snapshot();assert.equal(saved.projectSetups[0].setup.workerAdapter,adapter);
    assert.deepEqual(saved.projectSetups[0].setup.desktopWorker.binding,binding);
    assert.equal(saved.managementDecisions.length,0);assert.equal(saved.projectRuns.length,0);
  }
  await close();stopCore();fixture('native_desktop_plan_proposal_fixture');await launch();
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  let dialog=page.getByRole('dialog',{name:'审阅项目执行方案'});
  const scope=dialog.getByRole('region',{name:'桌面执行授权范围'});
  await scope.getByText('fixture-model',{exact:true}).waitFor();
  await scope.screenshot({path:join(dir,'approval-scope.png')});
  await dialog.getByRole('button',{name:'返回检查',exact:true}).click();assert.equal((await snapshot()).workflows[0].approval,null);
  await page.getByRole('button',{name:'审阅执行方案',exact:true}).click();
  dialog=page.getByRole('dialog',{name:'审阅项目执行方案'});await dialog.getByRole('button',{name:'确认执行授权',exact:true}).click();
  await page.getByText('执行方案已批准',{exact:true}).waitFor();
  const approved=await snapshot();assert.ok(approved.workflows[0].approval);
  assert.deepEqual(approved.workflows[0].revision.execution[0].desktopWorker,approved.projectSetups[0].setup.desktopWorker);
  const command={type:'startProjectRun',projectId:'project',revisionDigest:approved.workflows[0].revisionDigest,setupDigest:approved.projectSetups[0].digest};
  await assert.rejects(()=>page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('core_request',{command}),command),/尚未接入/);
  await close();stopCore();await launch();
  const reopened=await snapshot();assert.deepEqual(reopened.workflows[0].approval,approved.workflows[0].approval);
  assert.deepEqual(reopened.projectSetups,approved.projectSetups);
  assert.equal(reopened.projectRuns.length,0);assert.equal(reopened.apiJobs.length,0);assert.equal(reopened.codexJobs.length,0);assert.equal(reopened.verificationJobs.length,0);
  assert.equal(reopened.managementDecisions.length,1);assert.equal(await readFile(legacyPath,'utf8'),legacyBytes);assert.deepEqual(errors,[]);
  await page.getByRole('region',{name:'桌面编程会话配置'}).screenshot({path:join(dir,'reopened.png')});
  await close();
  await writeFile(join(dir,'result.json'),JSON.stringify({status:'passed',scope:'actual Tauri UI + IPC + production core configuration, approval and reopening; synthetic desktop observations and chief proposal',externalMessages:0,realModelCalls:0,checks:{bothAppsSaved:true,explicitAcknowledgements:true,adapterSwitchClearsAcknowledgements:true,approvalScopeVisible:true,returnDoesNotApprove:true,exactSelectionInApprovedPlan:true,dispatchRejectedBeforeRequest:true,reopenPreservesApproval:true,legacyConfigurationUnchanged:true},pageErrors:errors},null,2));console.log(JSON.stringify({dir,status:'passed'}));
} catch(error) {
  await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors},null,2));throw error;
} finally {
  await browser?.close().catch(()=>{});app?.kill();
  const stopped=spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});
  await writeFile(join(dir,'cleanup.json'),JSON.stringify({exitCode:stopped.status,stderr:stopped.stderr}));
}
