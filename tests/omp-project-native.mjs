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
const dir = await mkdtemp(resolve('reports/omp-project-native-')); 
const data = join(dir, 'data');
assert.ok(process.env.CERULEAN_OMP_TEST_PARENT,'explicit neutral runtime parent required');
const runtime = await mkdtemp(join(process.env.CERULEAN_OMP_TEST_PARENT,'native-runtime-'));
await writeFile(join(dir,'runtime-location.json'),JSON.stringify({runtime}));
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
let app, browser, page; let coreStopped=false; const errors = [];
async function launch() {
  coreStopped=false;
  const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
  const port = listener.address().port; await new Promise(r => listener.close(r));
  app = spawn(resolve('target/debug/cerulean-desktop.exe'), [], {windowsHide:true, stdio:'ignore', env:{...process.env,
    CERULEAN_TEST_DATA_DIR:data, CERULEAN_TEST_WEBVIEW_ROOT:join(dir,'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
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
  assert.equal(result.status,0,result.stderr);coreStopped=true;
}
const snapshot=()=>page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('core_request',{command:{type:'snapshot'}}));
try {
  await launch();
  const form=page.getByRole('region',{name:'主指挥项目规划'});
  assert.equal(await form.locator('#planning-adapter').inputValue(),'codex');
  const original=await snapshot();const originalWorker=original.projectSetups[0].setup.worker.executable.path;
  await form.locator('#planning-adapter').selectOption('omp');
  assert.equal(await form.locator('#planning-executable').inputValue(),'');
  assert.equal(await form.locator('#planning-worker').inputValue(),'');
  assert.equal(await form.getByRole('checkbox',{name:'记录连续命令失败'}).isDisabled(),true);
  await form.locator('#planning-executable').fill(originalWorker);
  await form.locator('#planning-worker').fill('fixture-deepseek-model');
  await form.locator('#planning-omp-runtime').fill(original.workflows[0].project.root);
  await form.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  await page.getByRole('alert').filter({hasText:/OMP/}).waitFor();
  assert.equal((await snapshot()).projectSetups[0].setup.version,1);
  await form.locator('#planning-omp-runtime').fill(runtime);
  await form.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  await form.getByText('配置版本 2',{exact:true}).waitFor();
  const configured=await snapshot();const setup=configured.projectSetups[0];
  assert.equal(setup.setup.workerAdapter,'omp');assert.equal(setup.setup.worker.provider,'deepseek');assert.equal(setup.setup.worker.model,'fixture-deepseek-model');
  assert.equal(setup.setup.policy.supervision,undefined);
  assert.ok(setup.setup.worker.ompRuntimeParent.endsWith(runtime.slice(3)));
  assert.equal(await form.locator('#planning-omp-runtime').inputValue(),setup.setup.worker.ompRuntimeParent);
  assert.equal(configured.ompJobs.length,0);assert.equal(configured.managementDecisions.length,0);assert.equal(configured.workflows[0].approval,null);
  await form.screenshot({path:join(dir,'omp-selected-saved.png')});
  await close();stopCore();await launch();
  const reopened=await snapshot();assert.deepEqual(reopened.projectSetups[0],setup);
  assert.equal(await page.locator('#planning-adapter').inputValue(),'omp');
  assert.equal(await page.locator('#planning-worker').inputValue(),'fixture-deepseek-model');
  assert.equal(await page.locator('#planning-omp-runtime').inputValue(),setup.setup.worker.ompRuntimeParent);
  await page.getByRole('region',{name:'主指挥项目规划'}).screenshot({path:join(dir,'omp-reopened.png')});
  assert.equal(await readFile(legacyPath,'utf8'),legacyBytes);assert.deepEqual(errors,[]);
  await close();stopCore();
  const result={at:new Date().toISOString(),status:'passed',scope:'native executor selection and immutable configuration persistence',realModelCalls:0,fullCoreRestarts:1,legacyBytesUnchanged:true,workerDispatches:0,supervisionDisabled:true,isolatedRuntimePersisted:true,runtime};
  await writeFile(join(dir,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({dir,...result}));
} catch(error) {
  await page?.screenshot({path:join(dir,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),errors}));throw error;
} finally {
  await browser?.close().catch(()=>{});app?.kill();
  const stopped=coreStopped?{status:0,stderr:'',alreadyStopped:true}:spawnSync(resolve('target/debug/cerulean-core.exe'),['--stop','--data-dir',data],{windowsHide:true,encoding:'utf8',timeout:20000});
  await writeFile(join(dir,'cleanup.json'),JSON.stringify({exitCode:stopped.status,stderr:stopped.stderr}));
}
