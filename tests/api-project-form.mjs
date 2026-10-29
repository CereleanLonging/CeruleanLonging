// Interactive production React components with a recording command sink.
// No Tauri IPC, provider request, credential access or native execution.
import {readFile,writeFile,mkdtemp} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {build} from 'vite';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','run in private owned Job');
const dir=await mkdtemp(resolve('reports/api-project-form-'));
const entry=join(dir,'entry.tsx');
const snapshot={workflows:[{project:{id:'project',root:'D:\\fixture-project'},state:{tasks:{}},approval:null}],projectSetups:[],validators:[{id:'tests',name:'独立验收工具'}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:'D:\\fixture-codex.exe'},providerConnections:[],projectRuns:[],codexJobs:[]};
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {ProjectPlanning} from '../../apps/desktop/src/ProjectPlanning';import {ProjectRuns} from '../../apps/desktop/src/ProjectRuns';const root=createRoot(document.getElementById('root')!);window.commands=[];const send=async(command)=>{window.commands.push(command);return true;};window.renderFixture=(data,mode='planning')=>root.render(mode==='planning'?<ProjectPlanning data={data} busy={false} send={send}/>:<ProjectRuns data={data} busy={false} send={send}/>);window.renderFixture(${JSON.stringify(snapshot)});`);
await build({configFile:false,logLevel:'error',define:{'process.env.NODE_ENV':JSON.stringify('production')},oxc:{jsx:{runtime:'automatic'}},build:{outDir:join(dir,'bundle'),emptyOutDir:false,lib:{entry,name:'ApiProjectForm',formats:['iife'],fileName:()=> 'form.js'}}});
const script=await readFile(join(dir,'bundle/form.js'),'utf8');
const css=await readFile('apps/desktop/src/style.css','utf8');
let browser;
let blockedNetwork=0;
const cases=[];const errors=[];
try {
  browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  const page=await browser.newPage({viewport:{width:1250,height:940},deviceScaleFactor:1});
  page.on('pageerror',e=>errors.push(e.message));
  await page.route('**/*',route=>{blockedNetwork++;return route.abort();});
  await page.setContent(`<html lang="zh-CN"><meta charset="utf-8"><style>${css}</style><div id="root"></div></html>`);
  await page.addScriptTag({content:script});
  await page.locator('#planning-requirement').fill('实现求和函数并通过独立测试');
  await page.locator('#planning-chief').fill('fixture-chief');
  await page.locator('#planning-deputy').fill('fixture-deputy');
  await page.locator('#planning-artifacts').fill('sum.mjs');
  const save=page.getByRole('button',{name:'保存需求与执行配置'});
  for(const provider of ['openai','anthropic','deepseek','qwen','kimi','grok']) {
    await page.locator('#planning-adapter').selectOption(provider);
    await page.locator('#planning-worker').fill('vendor/fixture-model');
    assert.equal(await page.locator('#planning-executable').count(),0);
    assert.equal(await page.getByLabel('记录连续命令失败').isDisabled(),true);
    const regions=provider==='qwen'?['qwenBeijing','qwenSingapore','qwenHongKong']:[null];
    for(const region of regions) {
      if(region)await page.locator('#planning-api-region').selectOption(region);
      if(region==='qwenBeijing') {
        const before=await page.evaluate(()=>window.commands.length);
        await save.click();
        assert.equal(await page.evaluate(()=>window.commands.length),before,'empty Beijing workspace must block HTML submit');
        await page.locator('#planning-api-workspace').fill('approved-space');
      }
      await page.locator('#planning-api-files').selectOption('read');
      await page.locator('#planning-api-maxRounds').fill('4');
      await page.locator('#planning-api-maxToolCalls').fill('12');
      await page.locator('#planning-api-maxOutputTokens').fill('2048');
      await page.locator('#planning-api-maxTotalTokens').fill('20000');
      await page.locator('#planning-api-requestTimeoutSeconds').fill('45');
      const count=await page.evaluate(()=>window.commands.length);
      await save.click();
      await page.waitForFunction(n=>window.commands.length===n+1,count);
      const command=await page.evaluate(()=>window.commands.at(-1));
      assert.equal(command.type,'configureProject');
      assert.equal(command.draft.workerAdapter,provider);
      assert.equal(command.draft.codexExecutable,'');
      assert.equal(command.draft.apiWorker.model,command.draft.workerModel);
      assert.deepEqual(command.draft.apiWorker.fileTools,['read-files']);
      assert.equal(command.draft.apiWorker.maxRounds,4);
      assert.equal(command.draft.apiWorker.maxToolCalls,12);
      assert.equal(command.draft.apiWorker.maxOutputTokens,2048);
      assert.equal(command.draft.apiWorker.maxTotalTokens,20000);
      assert.equal(command.draft.apiWorker.requestTimeoutSeconds,45);
      if(region)assert.equal(command.draft.apiWorker.target.endpoint.kind,region);
      cases.push({provider,region,command});
      if(region==='qwenBeijing')await page.screenshot({path:join(dir,'qwen-config.png'),fullPage:true});
    }
  }
  await page.locator('#planning-adapter').selectOption('codex');
  assert.equal(await page.locator('#planning-api-files').count(),0);
  await page.locator('#planning-executable').fill('D:\\fixture-codex.exe');
  await page.locator('#planning-worker').fill('fixture-worker');
  await save.click();
  const legacy=await page.evaluate(()=>window.commands.at(-1));
  assert.equal(legacy.draft.apiWorker,undefined);
  assert.equal(legacy.draft.workerAdapter,'codex');
  // Reload a saved API configuration through the actual ProjectPlanning effect.
  const qwen=cases.find(c=>c.region==='qwenBeijing').command.draft;
  const setup={...qwen,acceptance:undefined,version:1,check:{id:'project-check',args:[],timeoutSeconds:60},policy:{maxRequests:3,maxSeconds:180},managementMaxRequests:10};
  const reloaded={...snapshot,projectSetups:[{digest:'fixture-setup',setup}]};
  await page.evaluate(data=>window.renderFixture(data),reloaded);
  await page.waitForFunction(()=>document.querySelector('#planning-api-workspace')?.value==='approved-space');
  assert.equal(await page.locator('#planning-api-files').inputValue(),'read');
  assert.equal(await page.locator('#planning-worker').inputValue(),'vendor/fixture-model');
  const approved={...reloaded,openaiConnection:{models:[],keyConfigured:true},workflows:[{...snapshot.workflows[0],approval:{revisionDigest:'fixture-revision'},revisionDigest:'fixture-revision',revision:{execution:[{apiWorker:qwen.apiWorker}]}}]};
  await page.evaluate(data=>window.renderFixture(data,'runs'),approved);
  assert.equal(await page.getByRole('button',{name:'启动已批准项目'}).isDisabled(),true);
  assert.ok((await page.locator('body').innerText()).includes('qwen 编程模型凭证'));
  await page.screenshot({path:join(dir,'run-gate.png'),fullPage:true});
  const configured={...approved,providerCredentials:[{provider:'qwen',keyConfigured:true}]};
  await page.evaluate(data=>window.renderFixture(data,'runs'),configured);
  await page.getByRole('button',{name:'启动已批准项目'}).click();
  const dialog=page.getByRole('dialog');
  const confirm=dialog.getByRole('button',{name:'确认启动调度',exact:true});
  await page.evaluate(data=>window.renderFixture(data,'runs'),approved);
  await page.waitForFunction(()=>document.querySelector('[role="dialog"] [role="alert"]')?.textContent.includes('qwen 编程模型凭证'));
  assert.equal(await confirm.isDisabled(),true,'lost worker credential invalidates open review');
  await page.evaluate(data=>window.renderFixture(data,'runs'),configured);
  await confirm.click();
  assert.deepEqual(await page.evaluate(()=>window.commands.at(-1)),{type:'startProjectRun',projectId:'project',revisionDigest:'fixture-revision',setupDigest:'fixture-setup'});
  for(const state of ['unknown','cancel-requested']) {
    await page.evaluate(data=>window.renderFixture(data,'runs'),{...configured,apiJobs:[{projectId:'project',state}]});
    await page.waitForFunction(()=>document.querySelector('[role="status"]')?.textContent.includes('API 执行或停止结果'));
    assert.equal(await page.getByRole('button',{name:'启动已批准项目'}).isDisabled(),true);
  }
  await page.evaluate(data=>window.renderFixture(data,'runs'),{...configured,apiRecovery:{message:'fixture disk error',canRestart:false}});
  await page.waitForFunction(()=>document.querySelector('[role="status"]')?.textContent.includes('API 记录需要恢复'));
  assert.equal(await page.getByRole('button',{name:'启动已批准项目'}).isDisabled(),true);
  assert.deepEqual(errors,[]);assert.equal(blockedNetwork,0);
} catch(error) {
  await writeFile(join(dir,'failure.json'),JSON.stringify({error:String(error),pageErrors:errors,completedRoutes:cases.length},null,2));
  throw error;
} finally {await browser?.close();}
const result={status:'passed',scope:'interactive React components with recording command sink; not native IPC integration',routes:cases.length,cases,legacySwitch:true,savedReload:true,runGate:true,providerRequests:0,modelCalls:0,pageErrors:errors,artifact:dir};
await writeFile(join(dir,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify({status:result.status,routes:cases.length,artifact:dir}));
