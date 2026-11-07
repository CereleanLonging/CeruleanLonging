import {confirmConnectionContinue} from './connection-ui-helpers.mjs';
import assert from "node:assert/strict";
import { mkdtemp,readFile,writeFile } from "node:fs/promises";
import { join,resolve } from "node:path";
import { build } from "vite";
import { chromium } from "playwright-core";
import { openConfiguredConnections,openConnectionMethod } from "./connection-ui-helpers.mjs";

const run=await mkdtemp(resolve("reports/worker-configuration-ui-"));
assert(process.env.CERULEAN_CACHE_SESSION,'Use managed cache runner');
const bundle=join(process.env.CERULEAN_CACHE_SESSION,'worker-configuration-ui');
const entry=join(run,"entry.tsx");
const fixture={dataDirectory:"synthetic-worker-configuration",workflows:[{project:{id:"project",root:"D:/synthetic/project"},state:{tasks:{}},approval:null}],projectSetups:[],workerAccountSetups:[],validators:[{id:"cerulean-objective-checker",name:"Synthetic project checks"}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:"D:/synthetic/codex.exe"},providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[]};
await writeFile(entry,`import React,{useState}from'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';
import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';import{ResourceFields,resourceError}from'../../apps/desktop/src/ResourceFields';
import{suggestedWorkerResource,synchronizeWorkerProfiles,automaticTaskDescription}from'../../apps/desktop/src/workerConfiguration';
const root=createRoot(document.getElementById('root'));window.commands=[];window.resourceWrites=[];window.mode='connections';
window.data=${JSON.stringify(fixture)};window.rules={resourceError,suggestedWorkerResource,synchronizeWorkerProfiles,automaticTaskDescription};
function Resources(){const[value,setValue]=useState(['database']);return <form><ResourceFields id="resource-test" value={value} common={window.common??[]} onChange={next=>{window.resourceWrites.push(next);setValue(next);}}/><button type="submit">Save test resource</button></form>;}
window.resources=()=>flushSync(()=>root.render(<Resources/>));
window.planning=()=>root.render(<ProjectPlanning data={window.data} busy={false} mode={window.mode} send={async command=>{window.commands.push(command);return true;}} onContinue={()=>{window.mode='planning';window.planning();}} onConnections={()=>{window.mode='connections';window.planning();}}/>);`);
await build({configFile:false,logLevel:"error",define:{"process.env.NODE_ENV":JSON.stringify("production")},oxc:{jsx:{runtime:"automatic"}},build:{outDir:bundle,emptyOutDir:false,lib:{entry,name:"WorkerConfiguration",formats:["iife"],fileName:()=>"ui.js"}}});
const browser=await chromium.launch({executablePath:"C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",headless:true});
const cases=[],errors=[];let page;
try{
  page=await browser.newPage({viewport:{width:1280,height:960}});page.setDefaultTimeout(10000);page.on("pageerror",error=>errors.push(error.message));
  const css=await readFile("apps/desktop/src/style.css","utf8");
  await page.route("**/*",route=>route.request().url()==="https://cerulean.test/"?route.fulfill({contentType:"text/html",body:`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>`}):route.abort());
  await page.goto("https://cerulean.test/");await page.addScriptTag({path:join(bundle,"ui.js")});await page.evaluate(()=>window.resources());
  const resource=page.locator("#resource-test");
  for(const invalid of ["包含中文","space key","path/to/file","name:value"]){await resource.fill(invalid);assert.equal(await resource.inputValue(),"database");}
  assert.deepEqual(await page.evaluate(()=>window.resourceWrites),[]);
  await resource.fill("shared\nshared");assert.equal(await resource.evaluate(input=>input.checkValidity()),false);assert.deepEqual(await page.evaluate(()=>window.resourceWrites),[]);
  await resource.fill("a".repeat(81));assert.equal(await resource.evaluate(input=>input.checkValidity()),false);
  await resource.fill(Array.from({length:33},(_,index)=>`r${index}`).join("\n"));assert.equal(await resource.evaluate(input=>input.checkValidity()),false);
  await resource.fill("Database_1\ndatabase-1");assert.equal(await resource.evaluate(input=>input.checkValidity()),true);
  assert.deepEqual(await page.evaluate(()=>window.resourceWrites.at(-1)),["Database_1","database-1"]);
  cases.push("Invalid characters are refused; duplicates, overlong names and excess entries remain uncommitted and block the form; case-sensitive valid keys persist");

  const generated=await page.evaluate(async()=>{
    const profile={workerAdapter:'a6api',workerModel:'real-model-id',codexExecutable:'',apiWorker:{target:{kind:'provider',endpoint:{kind:'a6ApiVip'}},model:'real-model-id'}};
    const a=await window.rules.suggestedWorkerResource(profile), b=await window.rules.suggestedWorkerResource({...profile});
    const c=await window.rules.suggestedWorkerResource({...profile,workerModel:'different-model'});
    return {a,b,c,error:window.rules.resourceError([a]),overflow:window.rules.resourceError([a],Array.from({length:32},(_,i)=>'common'+i))};
  });
  assert.match(generated.a,/^executor-[a-f0-9]{64}$/);assert.equal(generated.a,generated.b);assert.notEqual(generated.a,generated.c);assert.equal(generated.error,"");assert(generated.overflow);
  cases.push("Automatic names are stable by execution identity and use the same format and combined resource-limit validation");

  await page.evaluate(()=>window.planning());await openConfiguredConnections(page);
  await page.locator("#planning-chief").fill("chief-model");await page.locator("#planning-deputy").fill("deputy-model");await page.locator("#planning-worker").fill("primary-model");
  await openConnectionMethod(page,"执行端","key");
  const worker=page.getByRole("region",{name:"执行端",exact:true});
  const isolation=worker.getByLabel('为每个执行模型使用独立工作目录',{exact:true});
  assert(await isolation.isChecked());assert(await isolation.isDisabled());
  assert(await isolation.evaluate(input=>input.readOnly));
  assert.equal(await page.locator('#planning-base-commit').count(),0);
  await worker.getByRole('region',{name:'任务工作目录',exact:true}).screenshot({path:join(run,'readonly-workspace.png')});
  cases.push('Fixed model workspaces display a checked read-only control with matching text');
  for(const [index,model] of [[0,"worker-a"],[1,"worker-b"]]){
    await worker.getByRole("button",{name:"添加Key 管理执行配置",exact:true}).click();
    await page.locator(`#profile-${index}-model`).fill(model);await page.locator(`#profile-${index}-id`).click();
  }
  assert.equal(await worker.locator('[id$="-artifacts"]').count(),0);
  assert.equal(await worker.locator('[id$="-description"]').count(),0);
  assert.equal(await worker.getByLabel('DeepSeek API Key',{exact:true}).count(),1);
  assert.equal(await worker.getByRole('region',{name:'执行端共享凭证',exact:true}).getByRole('button',{name:'测试连通',exact:true}).count(),1);
  const credentials=worker.getByRole('region',{name:'执行端共享凭证',exact:true});
  for(const width of [1280,440]){
    await page.setViewportSize({width,height:960});
    const accessBox=await credentials.boundingBox();
    const settingsBox=await worker.getByRole('heading',{name:'Key 执行管理设置',exact:true}).boundingBox();
    const defaultBox=await worker.getByRole('region',{name:'执行端模型配置',exact:true}).boundingBox();
    const profileBox=await worker.getByRole('region',{name:'执行配置 1',exact:true}).boundingBox();
    assert(accessBox.y+accessBox.height<=settingsBox.y);
    assert(settingsBox.y<defaultBox.y&&defaultBox.y<profileBox.y);
  }
  await worker.locator(':scope > details[data-method="key"]').screenshot({path:join(run,'key-access-before-settings.png')});
  await page.setViewportSize({width:1280,height:960});
  cases.push('Key access and shared model selection precede default and additional execution settings on wide and narrow screens');
  await page.getByRole("button",{name:"下一步：设置需求与计划",exact:true}).click();await confirmConnectionContinue(page);
  await page.locator('#planning-requirement').waitFor();
  assert.equal(await page.locator('[id$="-description"],[id$="-artifacts"]').count(),0);
  assert.equal(await page.getByRole('status').filter({hasText:'请先在接入管理确认本项目的角色模型'}).count(),0);
  assert.equal(await page.getByRole('button',{name:'增加检查',exact:true}).count(),0);
  await page.locator('#planning-requirement').fill('Synthetic project requirement');
  await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  let submitted=await page.evaluate(()=>window.commands.find(c=>c.type==='configureProject')?.draft);assert(submitted);
  assert.deepEqual(submitted.artifactPaths,[]);
  for(const profile of submitted.profiles){assert.deepEqual(profile.artifactPaths,[]);assert.equal(profile.description,await page.evaluate(()=>window.rules.automaticTaskDescription));assert.match(profile.resourceKeys[0],/^executor-[a-f0-9]{64}$/);}
  cases.push('Manual division and deliverable forms are absent; saving works with empty templates for chief-proposed deliverables, automatic resources and unchanged selected models');

  await page.getByRole('button',{name:'修改接入配置',exact:true}).click();await openConnectionMethod(page,'执行端','key');
  assert(await isolation.isChecked());assert(await isolation.isDisabled());
  await page.locator('#profile-1-model').fill('worker-a');await page.locator('#profile-1-id').click();
  await page.locator('#profile-1-id').fill('profile-1');await page.locator('#profile-0-id').click();
  assert.equal(await page.locator('#profile-1-id').inputValue(),'profile-1-1');
  await page.getByRole('button',{name:'下一步：设置需求与计划',exact:true}).click();await confirmConnectionContinue(page);await page.locator('#planning-requirement').waitFor();
  await page.getByRole('button',{name:'保存需求与执行配置',exact:true}).click();
  submitted=await page.evaluate(()=>window.commands.filter(c=>c.type==='configureProject').at(-1)?.draft);
  assert.equal(submitted.validatorId,'cerulean-objective-checker');assert.deepEqual(submitted.acceptance.stages[0].expectedAssertions,[]);
  assert.equal(submitted.chiefDirectory,'D:/synthetic/project-chief');assert.equal(submitted.worktree,undefined);
  for(const profile of submitted.profiles){assert.deepEqual(profile.acceptance,submitted.acceptance);assert.equal(profile.supervisionOverride,undefined);}
  assert.deepEqual(submitted.profiles[0].resourceKeys,submitted.profiles[1].resourceKeys);
  assert.equal(submitted.supervision.deliveryEnabled,true);assert.equal(submitted.supervision.maxAdvicePerAttempt,1);
  cases.push('Returning to access preserves configuration, resolves duplicate IDs and synchronizes resource identities without reintroducing manual file scopes');
  await page.screenshot({path:join(run,"unified-worker-settings.png"),fullPage:true});assert.deepEqual(errors,[]);
  await writeFile(join(run,"result.json"),JSON.stringify({status:"passed",cases,errors,providerCalls:0},null,2));console.log(JSON.stringify({status:"passed",cases:cases.length,report:run}));
}catch(error){await page?.screenshot({path:join(run,"failure.png"),fullPage:true}).catch(()=>{});await writeFile(join(run,"failure.json"),JSON.stringify({error:String(error),cases,errors},null,2));throw error;}
finally{await browser.close();}
