import assert from 'node:assert/strict';
import {mkdtemp, writeFile} from 'node:fs/promises';
import {join, resolve} from 'node:path';
import {build} from 'vite';
import {chromium} from 'playwright-core';
import {openConnectionMethod} from './connection-ui-helpers.mjs';

assert(process.env.CERULEAN_CACHE_SESSION, 'Use managed cache runner');
const run = await mkdtemp(resolve('reports/background-access-ui-'));
const bundle = join(process.env.CERULEAN_CACHE_SESSION, 'background-access-ui');
const data = {
  dataDirectory:'synthetic-background-access',
  workflows:[{project:{id:'project',root:'D:/synthetic/project'},state:{tasks:{}},approval:null}],
  projectSetups:[],workerAccountSetups:[],validators:[],managementDecisions:[],
  managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},
  codexConnection:{models:[],executable:'D:/synthetic/codex.exe'},
  providerConnections:[{provider:'deepseek',state:'unchecked',keyConfigured:true,models:[],target:{kind:'deepSeek'}}],
  providerCredentials:[{provider:'deepseek',keyConfigured:true}],
  projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[],
};
const entry = join(run, 'entry.tsx');
await writeFile(entry, `import React from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';
import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';
import{IntegrationHub}from'../../apps/desktop/src/IntegrationHub';
import{WorkerProfiles}from'../../apps/desktop/src/WorkerProfiles';
import{accessDraftKey}from'../../apps/desktop/src/accessDraft';
const data=${JSON.stringify(data)};const root=createRoot(document.getElementById('root'));
window.calls=[];window.native=[];window.generation=0;window.hub=false;window.cliMissing=false;
window.profileView=false;window.extraProfiles=[];
window.__TAURI_INTERNALS__={invoke:async(name,args)=>{
  window.native.push({name,args});
  if(name==='model_reasoning_options')return ['none','low','high','max'];
  if(name==='test_local_connection')return {message:'synthetic connection checked',checkedAt:new Date().toISOString()};
  if(name==='cli_model_catalog')return window.cliMissing?[]:[{id:'cli-model',model:'cli-model',displayName:'CLI model',supportedReasoningEfforts:['low','high'],isDefault:true}];
  if(name==='objective_checker_status')return {ready:true,version:'fixture',path:'D:/fixture/check.exe',sha256:'a'.repeat(64)};
  throw Error('Unexpected native call '+name);
}};
const send=async command=>{window.calls.push(command);return true;};
window.render=()=>flushSync(()=>root.render(window.profileView
  ?<WorkerProfiles data={data} projectId='project' value={window.extraProfiles} worktree={true} commonResources={[]} methodFilter='desktop' showConnections send={send} onChange={profiles=>{window.extraProfiles=profiles;window.render();}}/>
  :window.hub?<IntegrationHub data={data} busy={false} send={send} configure={()=>{}}/>:<ProjectPlanning key={window.generation} data={data} mode='connections' busy={false} send={send}/>));
window.stored=()=>JSON.parse(localStorage.getItem(accessDraftKey(data.dataDirectory,'project')))?.selection;
window.readyKey=()=>{data.providerConnections[0]={...data.providerConnections[0],state:'listed',models:['deepseek-flash']};window.render();};
window.render();`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},
  build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:'BackgroundAccess',formats:['iife'],fileName:()=> 'ui.js'}}});
const browser = await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
const page = await browser.newPage({viewport:{width:1280,height:1000}});
const cases=[], errors=[];
try {
  page.setDefaultTimeout(10000);
  page.on('pageerror', error=>errors.push(error.message));
  await page.route('**/*', route=>route.request().url()==='https://cerulean.test/'
    ?route.fulfill({contentType:'text/html',body:'<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>'}):route.abort());
  await page.goto('https://cerulean.test/');
  await page.addStyleTag({path:'apps/desktop/src/style.css'});
  await page.addScriptTag({path:join(bundle,'ui.js')});
  const region = name=>page.getByRole('region',{name,exact:true});
  const worker=region('执行端'), deputy=region('副指挥'), chief=region('主指挥');
  assert.equal(await page.locator('#planning-adapter').inputValue(),'deepseek');
  await openConnectionMethod(page,'主指挥','desktop');
  await page.locator('#planning-chief-session').fill('bound-chief-session');
  assert.equal(await chief.getByRole('button',{name:/思考强度/}).count(),0);
  assert.equal(await chief.getByText('OMP',{exact:true}).count(),0);
  cases.push('New projects default to API workers; client-bound chief sessions use their own settings');

  await openConnectionMethod(page,'副指挥','desktop');
  await page.locator('#planning-deputy-executable').fill('D:/synthetic/omp.exe');
  assert.equal(await page.locator('#planning-deputy').inputValue(),'deepseek-flash');
  await deputy.getByRole('button',{name:'思考强度 · 最高',exact:true}).click();
  const options=page.getByRole('dialog',{name:'模型选项',exact:true});
  await options.getByRole('radio',{name:'低',exact:true}).check();
  await options.getByRole('button',{name:'确认思考强度',exact:true}).click();
  await page.waitForFunction(()=>window.stored()?.managementSources.deputyReasoningEffort==='low');
  assert.equal((await page.evaluate(()=>window.stored())).managementSources.deputy.kind,'omp');
  cases.push('Deputy OMP is desktop-only and saves an adjustable explicit effort');

  await openConnectionMethod(page,'执行端','desktop');
  await worker.getByRole('button',{name:'将桌面端接入设为默认执行端',exact:true}).click();
  await page.locator('#planning-adapter').selectOption('omp');
  await page.locator('#planning-executable').fill('D:/synthetic/worker-omp.exe');
  await page.locator('#planning-worker').fill('deepseek-flash');
  await worker.getByRole('button',{name:'测试连通',exact:true}).click();
  const confirmation=page.getByRole('dialog',{name:'确认本次 OMP 接入',exact:true});
  assert.equal((await page.evaluate(()=>window.native.filter(call=>call.name==='test_local_connection'))).length,0);
  await confirmation.getByRole('button',{name:'确认并检查',exact:true}).click();
  await worker.getByRole('status').filter({hasText:'synthetic connection checked'}).waitFor();
  const request=(await page.evaluate(()=>window.native.filter(call=>call.name==='test_local_connection').at(-1))).args.request;
  assert.equal(request.reasoningEffort,'max');
  assert.equal(request.executable,'D:/synthetic/worker-omp.exe');
  assert.equal((await page.evaluate(()=>window.stored())).managementSources.deputyReasoningEffort,'low');
  assert.equal(await worker.locator('details[data-method="api"]').count(),0);
  cases.push('Worker OMP requires explicit preflight confirmation and cannot overwrite deputy selection');

  await page.evaluate(()=>{window.generation++;window.render();});
  await openConnectionMethod(page,'副指挥','desktop');
  await openConnectionMethod(page,'执行端','desktop');
  assert.equal(await page.locator('#planning-adapter').inputValue(),'omp');
  assert(await deputy.getByRole('button',{name:'思考强度 · 低',exact:true}).isVisible());
  assert(await worker.getByRole('button',{name:'思考强度 · 最高',exact:true}).isVisible());
  await openConnectionMethod(page,'副指挥','api');
  await page.waitForFunction(()=>document.querySelector('#planning-deputy-source')?.value==='openAi');
  await page.waitForFunction(()=>window.stored()?.managementSources.deputy.kind==='openAi');
  assert.equal((await page.evaluate(()=>window.stored())).workerAdapter,'omp');
  cases.push('Reload preserves both role selections and switching deputy routes leaves worker OMP intact');

  await openConnectionMethod(page,'执行端','cli');
  await worker.getByRole('button',{name:'将CLI 接入设为默认执行端',exact:true}).click();
  await page.locator('#planning-executable').fill('D:/synthetic/codex.exe');
  await worker.getByRole('button',{name:'选择 CLI 模型',exact:true}).click();
  let cli=page.getByRole('dialog',{name:'选择 CLI 模型',exact:true});
  await cli.getByRole('radio',{name:'cli-model',exact:true}).check();
  await cli.getByRole('button',{name:'下一步 · 思考强度',exact:true}).click();
  cli=page.getByRole('dialog',{name:'CLI 思考强度',exact:true});
  assert(await cli.getByRole('radio',{name:'高',exact:true}).isChecked());
  assert.equal(await cli.getByRole('radio',{name:'最高',exact:true}).count(),0);
  await cli.getByRole('radio',{name:'低',exact:true}).check();
  await cli.getByRole('button',{name:'确认模型与思考强度',exact:true}).click();
  await page.waitForFunction(()=>window.stored()?.workerReasoningEffort==='low');
  assert.equal((await page.evaluate(()=>window.stored())).workerModel,'cli-model');
  cases.push('CLI offers the actual catalog efforts, defaults to its highest supported level and saves the chosen level');

  await page.evaluate(()=>{window.cliMissing=true;window.readyKey();});
  await worker.getByRole('button',{name:'选择 CLI 模型',exact:true}).click();
  const fallback=page.getByRole('dialog',{name:'补充所需模型的 Key',exact:true});
  await fallback.getByLabel('Key 所属服务',{exact:true}).selectOption('deepseek');
  await fallback.getByRole('button',{name:'选择或修改模型',exact:true}).click();
  const api=page.getByRole('dialog',{name:'选择具体模型',exact:true});
  await api.getByRole('radio',{name:'deepseek-flash',exact:true}).check();
  await api.getByRole('button',{name:'下一步 · 思考强度',exact:true}).click();
  await api.getByRole('radio',{name:'高',exact:true}).check();
  await api.getByRole('button',{name:'确认模型与思考强度',exact:true}).click();
  await page.waitForFunction(()=>window.stored()?.workerAdapter==='deepseek');
  await page.locator('#planning-worker').waitFor({state:'visible'});
  const direct=await page.evaluate(()=>window.stored());
  assert.equal(direct.apiWorker.reasoningEffort,'high');
  assert.equal(direct.codexExecutable,'');
  assert.equal(direct.workerReasoningEffort,undefined);
  cases.push('Unavailable CLI models open an explicit provider-Key handoff and reveal the confirmed direct connection');

  await worker.getByRole('button',{name:'添加Key 管理执行配置',exact:true}).click();
  assert.equal(await page.locator('#profile-0-adapter option[value="omp"]').count(),0);
  await page.evaluate(()=>{window.profileView=true;window.render();});
  const addDesktop=page.getByRole('button',{name:'添加桌面端接入执行配置',exact:true});
  assert(await addDesktop.isEnabled());
  await addDesktop.click();
  assert.equal(await page.locator('#profile-0-adapter').inputValue(),'omp');
  assert(await page.locator('#profile-0-adapter option[value="qoder"]').isDisabled());
  assert.equal((await page.evaluate(()=>window.extraProfiles))[0].workerReasoningEffort,'max');
  cases.push('Additional OMP belongs to Desktop access and remains available with worktree execution');

  await page.evaluate(()=>{window.profileView=false;window.hub=true;window.render();});
  await openConnectionMethod(page,'副指挥','desktop');
  await openConnectionMethod(page,'执行端','desktop');
  await page.locator('#integration-deputy-omp-model').fill('deputy-specific');
  assert.equal(await page.locator('#integration-worker-omp-model').inputValue(),'deepseek-flash');
  assert.equal(await worker.locator('summary').filter({hasText:/^API 连接$/}).count(),0);
  await page.screenshot({path:join(run,'desktop-background-options.png'),fullPage:true});
  assert.deepEqual(await page.evaluate(()=>window.calls),[]);
  assert.deepEqual(errors,[]);
  cases.push('Unbound hub keeps role-specific OMP preferences and performs no implicit model generation');
  await writeFile(join(run,'result.json'),JSON.stringify({passed:true,cases,errors},null,2));
  console.log(JSON.stringify({passed:true,cases:cases.length,run}));
} catch(error) {
  await page.screenshot({path:join(run,'failure.png'),fullPage:true}).catch(()=>{});
  await writeFile(join(run,'failure.json'),JSON.stringify({error:String(error),cases,errors},null,2));
  throw error;
} finally {
  await browser.close();
}
