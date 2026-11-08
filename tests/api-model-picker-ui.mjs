import {confirmConnectionContinue} from './connection-ui-helpers.mjs';
import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { build } from "vite";
import { chromium } from "playwright-core";
import { openConnectionMethod } from "./connection-ui-helpers.mjs";

const run = await mkdtemp(resolve("reports/api-model-picker-ui-"));
assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'api-model-picker-ui');
const models = ["deepseek-ai/DeepSeek-V3.2", "DeepSeek-R1", "qwen3-32b", "Qwen3-235B-A22B", "claude-sonnet-4-5", "gpt-4.1",
  ...Array.from({length:144},(_,index)=>`synthetic-catalog-model-${index}`)];
const snapshot = {
  dataDirectory:"synthetic-model-picker", workflows:[{project:{id:"project",root:"D:/synthetic/project"},state:{tasks:{}},approval:null}],
  projectSetups:[], workerAccountSetups:[], validators:[{id:"cerulean-objective-checker",name:"Fixture checks"}], managementDecisions:[],
  managementHistory:{projects:[]}, openaiConnection:{state:"notConfigured",models:[],keyConfigured:false},
  codexConnection:{models:[],executable:"D:/synthetic/codex.exe"},
  providerConnections:[{provider:"a6api",target:{kind:"a6ApiVip"},state:"notConfigured",keyConfigured:false,models:[],listLimited:false}],
  projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[],
};
const entry = join(run,"entry.tsx");
await writeFile(entry, `import React from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';
import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';import{IntegrationHub}from'../../apps/desktop/src/IntegrationHub';
import{matchesApiModel}from'../../apps/desktop/src/apiModels';
const root=createRoot(document.getElementById('root'));window.snapshot=${JSON.stringify(snapshot)};window.calls=[];window.mode='connections';window.hub=false;window.models=${JSON.stringify(models)};window.matches=matchesApiModel;
window.render=()=>flushSync(()=>root.render(window.hub?<IntegrationHub data={window.snapshot} busy={false} send={send} configure={()=>{}}/>:<ProjectPlanning data={window.snapshot} busy={false} send={send} mode={window.mode} onContinue={()=>{window.mode='planning';window.render();}} onConnections={()=>{window.mode='connections';window.render();}}/>));
window.patch=patch=>{window.snapshot={...window.snapshot,...patch};window.render();};
async function send(command,onError){window.calls.push(command);const data=structuredClone(window.snapshot);let status=data.providerConnections[0];
if(command.type==='setProviderKey'){status.keyConfigured=true;status.state='unchecked';status.models=[];}
if(command.type==='removeProviderKey'){status.keyConfigured=false;status.state='notConfigured';status.models=[];}
if(command.type==='checkProviderModels'){status.target=command.target;status.state=window.fail?'failed':'listed';status.models=window.fail?[]:window.models;status.error=window.fail?'fixture directory failure':null;status.listLimited=!!window.limited;}
if(command.type==='setOpenAiKey'){data.openaiConnection={state:'unchecked',keyConfigured:true,models:[]};}
if(command.type==='checkOpenAi'){data.openaiConnection={state:'available',keyConfigured:true,models:['gpt-4.1','gpt-4.1-mini']};}
window.snapshot=data;window.render();return true;}
window.__TAURI_INTERNALS__={invoke:async(name)=>{if(name==="model_reasoning_options")return [];throw Error(name);}};window.render();`);
await build({configFile:false,logLevel:"error",define:{"process.env.NODE_ENV":JSON.stringify("production")},oxc:{jsx:{runtime:"automatic"}},
  build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:"ModelPicker",formats:["iife"],fileName:()=>"ui.js"}}});
const browser = await chromium.launch({executablePath:"C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",headless:true});
const cases=[], errors=[];
let page;
try {
  page=await browser.newPage({viewport:{width:1280,height:960}});page.setDefaultTimeout(8000);
  page.on("pageerror",error=>errors.push(error.message));
  const css=await readFile("apps/desktop/src/style.css","utf8");
  await page.route("**/*",route=>route.request().url()==="https://cerulean.test/"?route.fulfill({contentType:"text/html",body:`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>`}):route.abort());
  await page.goto("https://cerulean.test/");await page.addScriptTag({path:join(bundle,"ui.js")});
  const modal=()=>page.getByRole("dialog",{name:"选择具体模型",exact:true});
  const confirm=()=>modal().getByRole("button",{name:"下一步 · 思考强度",exact:true});
  const close=()=>modal().getByRole("button",{name:"稍后选择",exact:true}).click();
  const select=async(model,query)=>{await modal().getByRole("searchbox").fill(query);await modal().getByRole("radio",{name:model,exact:true}).check();await confirm().click();await modal().getByRole("button",{name:"确认模型与思考强度",exact:true}).click();await modal().waitFor({state:"hidden"});};
  const region=name=>page.getByRole("region",{name,exact:true});
  const chief=region("主指挥"), deputy=region("副指挥"), worker=region("执行端");
  await openConnectionMethod(page,"主指挥","api");await page.locator("#planning-chief-source").selectOption("provider:a6ApiVip");
  await openConnectionMethod(page,"主指挥","key");
  await chief.getByLabel("A6API API Key",{exact:true}).fill("synthetic-offline-model-key");
  await chief.getByRole("button",{name:"加密保存 A6API Key",exact:true}).click();
  await modal().waitFor();assert(await confirm().isDisabled());
  assert.deepEqual(await page.evaluate(()=>window.calls.map(c=>c.type)),["setProviderKey"]);
  await modal().getByRole("button",{name:"读取模型目录",exact:true}).click();
  await modal().getByRole("radio",{name:"DeepSeek-R1",exact:true}).waitFor();
  assert.equal(await modal().getByRole("radio").count(),150);
  assert.deepEqual(await page.evaluate(()=>window.calls.at(-1)),{type:"checkProviderModels",target:{kind:"a6ApiVip"}});
  cases.push("Saving a key opens one picker; listing requires the explicit directory action and exact selected endpoint");

  for (const [query,expected] of [["DSR1","DeepSeek-R1"],["deepseek-ai/DeepSeek-V3.2","deepseek-ai/DeepSeek-V3.2"],["千问３ ３２Ｂ","qwen3-32b"],["QWEN3 235B A22B","Qwen3-235B-A22B"],["c s 45","claude-sonnet-4-5"],["GPT4.1","gpt-4.1"]]) {
    await modal().getByRole("searchbox").fill(query);assert(await modal().getByRole("radio",{name:expected,exact:true}).isVisible(),query);
    assert(await confirm().isDisabled());
  }
  await modal().getByRole("searchbox").fill("missing-model-999");assert.equal(await modal().getByRole("radio").count(),0);assert(await confirm().isDisabled());
  await modal().getByRole("searchbox").fill("ds");assert.equal(await modal().getByRole("radio").count(),2);
  await page.screenshot({path:join(run,"model-search.png")});
  await select("deepseek-ai/DeepSeek-V3.2","DS V3.2");
  assert.equal(await page.locator("#planning-chief").inputValue(),"deepseek-ai/DeepSeek-V3.2");
  assert.equal(await page.locator("#planning-deputy").inputValue(),"");
  cases.push("Aliases, full names, separators, full-width text and mixed letters/digits match; explicit confirmation preserves the canonical ID");

  await openConnectionMethod(page,"副指挥","api");await page.locator("#planning-deputy-source").selectOption("provider:a6ApiVip");
  await openConnectionMethod(page,"副指挥","key");await modal().waitFor();await select("claude-sonnet-4-5","克劳德45");
  assert.equal(await page.locator("#planning-deputy").inputValue(),"claude-sonnet-4-5");
  await deputy.getByRole("button",{name:"测试连通",exact:true}).click();await modal().waitFor();await close();
  assert.equal(await page.locator("#planning-deputy").inputValue(),"claude-sonnet-4-5");
  cases.push("Existing keys prompt the deputy when its Key section opens; a successful recheck permits reselection and cancel preserves the prior model");

  await openConnectionMethod(page,"执行端","key");
  await page.locator("#planning-adapter").selectOption("a6api");await page.locator("#planning-api-relay-origin").selectOption("a6ApiVip");
  await modal().waitFor();await select("qwen3-32b","qw3 32b");
  assert.equal(await page.locator("#planning-worker").inputValue(),"qwen3-32b");
  await worker.getByRole("button",{name:"添加Key 管理执行配置",exact:true}).click();await page.locator("#profile-0-adapter").selectOption("a6api");
  await page.locator("#profile-0-api-relay-origin").selectOption("a6ApiVip");
  const profile=worker.getByRole("region",{name:"执行配置 1",exact:true});
  const shared=worker.getByRole("region",{name:"执行端共享凭证",exact:true});
  if(!await modal().isVisible())await shared.getByRole("button",{name:"选择或修改模型",exact:true}).click();
  await modal().waitFor();await modal().getByLabel("应用到",{exact:true}).selectOption("profile:0");await select("DeepSeek-R1","dsr1");
  assert.equal(await worker.getByLabel("A6API API Key",{exact:true}).count(),1);
  assert.equal(await profile.locator('input[type="password"]').count(),0);
  assert.equal(await worker.locator('summary').filter({hasText:/^API 连接$/}).count(),0);
  assert.equal(await worker.locator('summary').filter({hasText:/^Key 管理$/}).count(),1);
  assert.equal(await page.locator("#profile-0-model").inputValue(),"DeepSeek-R1");
  const draftKey=JSON.stringify(["cerulean-access-draft",1,snapshot.dataDirectory,"project"]);
  const stored=async()=>page.evaluate(key=>JSON.parse(localStorage.getItem(key)).selection,draftKey);
  await page.waitForFunction(key=>JSON.parse(localStorage.getItem(key))?.selection.profiles?.[0]?.apiWorker?.model==="DeepSeek-R1",draftKey);
  const selectedDraft=await stored();assert.equal(selectedDraft.apiWorker.model,"qwen3-32b");assert.equal(selectedDraft.workerModel,"qwen3-32b");
  assert.equal(selectedDraft.profiles[0].workerModel,"DeepSeek-R1");assert.equal(selectedDraft.chiefModel,"deepseek-ai/DeepSeek-V3.2");
  cases.push("Default and additional workers write both workerModel and apiWorker.model, independently of management roles");

  await page.getByRole("button",{name:"下一步：设置需求与计划",exact:true}).click();await confirmConnectionContinue(page);

  await page.locator("#planning-requirement").fill("Verify exact selected models reach the saved setup");
  await page.getByRole("button",{name:"保存需求与执行配置",exact:true}).click();
  const submitted=await page.evaluate(()=>window.calls.find(command=>command.type==='configureProject')?.draft);
  assert(submitted);assert.equal(submitted.chiefModel,"deepseek-ai/DeepSeek-V3.2");assert.equal(submitted.deputyModel,"claude-sonnet-4-5");
  assert.equal(submitted.apiWorker.model,"qwen3-32b");assert.equal(submitted.profiles[0].apiWorker.model,"DeepSeek-R1");
  await page.evaluate(()=>{window.mode='connections';window.render();});
  cases.push("The normal Next and Save flow submits each exact model through configureProject");

  await openConnectionMethod(page,"执行端","key");
  await shared.getByRole("button",{name:"选择或修改模型",exact:true}).click();
  await modal().getByLabel("应用到",{exact:true}).selectOption("profile:0");
  await select("gpt-4.1","gpt41");
  assert.equal(await page.locator("#planning-worker").inputValue(),"qwen3-32b");assert.equal(await page.locator("#profile-0-model").inputValue(),"gpt-4.1");
  await openConnectionMethod(page,"执行端","key");await page.locator("#profile-0-api-relay-origin").selectOption("a6Api");
  await openConnectionMethod(page,"执行端","key");
  assert.equal(await worker.locator(':scope > details[data-method="key"] .api-model-selection').count(),1);
  assert.equal(await worker.getByLabel("A6API API Key",{exact:true}).count(),1);
  await shared.getByLabel("凭证检查目标",{exact:true}).selectOption(JSON.stringify(['a6Api',null]));
  await shared.getByRole("button",{name:"选择或修改模型",exact:true}).click();
  assert.equal(await modal().getByRole('radio').count(),0);assert(await confirm().isDisabled());await close();
  cases.push("One shared credential card serves each provider; assignments and distinct endpoint catalogues remain isolated");

  await openConnectionMethod(page,"主指挥","api");await page.locator("#planning-chief-source").selectOption("provider:a6Api");
  await chief.getByRole("button",{name:"选择或修改模型",exact:true}).click();assert.equal(await modal().getByRole("radio").count(),0);assert(await confirm().isDisabled());
  await page.evaluate(()=>window.fail=true);await modal().getByRole("button",{name:"读取模型目录",exact:true}).click();
  await modal().getByRole("status").filter({hasText:"读取失败"}).waitFor();assert(await confirm().isDisabled());
  await page.evaluate(()=>{window.fail=false;window.models=[];});await modal().getByRole("button",{name:"读取模型目录",exact:true}).click();
  await modal().getByRole("status").filter({hasText:"目录为空"}).waitFor();assert(await confirm().isDisabled());
  await page.evaluate(models=>{window.models=models;window.limited=true;},models);await modal().getByRole("button",{name:"刷新模型目录",exact:true}).click();
  await modal().getByText(/当前返回部分目录/).waitFor();
  await modal().getByRole("radio",{name:"DeepSeek-R1",exact:true}).check();
  await page.evaluate(()=>{const data=structuredClone(window.snapshot);data.providerConnections[0].state='checking';window.patch(data);});
  assert(await confirm().isDisabled());
  await page.evaluate(()=>{const data=structuredClone(window.snapshot);data.providerConnections[0].state='listed';data.providerConnections[0].models=['replacement-model'];window.patch(data);});
  assert(await confirm().isDisabled());await close();
  cases.push("Changed endpoints, failed/empty directories, in-progress refresh and removed models cannot confirm stale IDs; partial lists are identified");

  await openConnectionMethod(page,"副指挥","api");await page.locator("#planning-deputy-source").selectOption("openAi");await openConnectionMethod(page,"副指挥","key");
  await deputy.getByLabel("OpenAI API Key",{exact:true}).fill("synthetic-openai-model-key");await deputy.getByRole("button",{name:"加密保存 Key",exact:true}).click();await modal().waitFor();
  await modal().getByRole("button",{name:"读取模型目录",exact:true}).click();await select("gpt-4.1-mini","GPT41MINI");
  assert.equal(await page.locator("#planning-deputy").inputValue(),"gpt-4.1-mini");
  cases.push("OpenAI credentials use the same searchable picker and write the actual deputy field");

  await deputy.getByRole("button",{name:"选择或修改模型",exact:true}).click();await page.setViewportSize({width:390,height:740});
  const bounds=await modal().boundingBox();assert(bounds.x>=0 && bounds.x+bounds.width<=391);assert(bounds.height<=741);
  assert.equal(await page.evaluate(()=>document.activeElement?.getAttribute('type')),"search");
  await page.screenshot({path:join(run,"model-picker-narrow.png")});await page.keyboard.press("Escape");await modal().waitFor({state:"hidden"});
  assert.equal(await page.locator("#planning-deputy").inputValue(),"gpt-4.1-mini");await page.setViewportSize({width:1280,height:960});
  await page.reload();await page.addScriptTag({path:join(bundle,"ui.js")});
  assert.equal(await page.locator("#planning-deputy").inputValue(),"gpt-4.1-mini");assert.equal(await page.locator("#planning-worker").inputValue(),"qwen3-32b");
  cases.push("Modal fits narrow screens, focuses search, supports Escape, and confirmed project selections survive reload");

  await page.evaluate(()=>{window.hub=true;window.snapshot.workflows=[];window.render();});
  await openConnectionMethod(page,"主指挥","key");await chief.getByLabel("凭证供应商",{exact:true}).selectOption("a6api");
  await chief.getByLabel("A6API API Key",{exact:true}).fill("synthetic-default-key");await chief.getByRole("button",{name:"加密保存 A6API Key",exact:true}).click();await modal().waitFor();
  await modal().getByRole("button",{name:"读取模型目录",exact:true}).click();await select("DeepSeek-R1","dsr1");
  await page.evaluate(()=>{window.hub=false;window.mode='connections';window.snapshot.workflows=[{project:{id:'new',root:'D:/synthetic/new'},state:{tasks:{}},approval:null}];window.render();});
  await page.waitForFunction(()=>document.getElementById('planning-chief')?.value==='DeepSeek-R1');
  assert.equal(await page.locator("#planning-chief-source").inputValue(),"provider:a6ApiVip");
  cases.push("A pre-project selection persists as a role default and populates the actual new project configuration");
  assert.deepEqual(errors,[]);
  assert.equal(await page.evaluate(()=>window.calls.some(c=>/start|requestChief|approve|execute|Trial/i.test(c.type))),false);
  await writeFile(join(run,"result.json"),JSON.stringify({status:"passed",cases,externalRequests:0,errors},null,2));
  console.log(JSON.stringify({status:"passed",cases:cases.length,report:run}));
} catch(error) {
  await page?.screenshot({path:join(run,"failure.png"),fullPage:true}).catch(()=>{});
  await writeFile(join(run,"failure.json"),JSON.stringify({error:String(error),cases,errors},null,2));throw error;
} finally { await browser.close(); }
