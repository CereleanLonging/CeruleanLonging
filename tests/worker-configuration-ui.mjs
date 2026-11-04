import assert from "node:assert/strict";
import { mkdtemp,readFile,writeFile } from "node:fs/promises";
import { join,resolve } from "node:path";
import { build } from "vite";
import { chromium } from "playwright-core";
import { openConfiguredConnections,openConnectionMethod } from "./connection-ui-helpers.mjs";

const run=await mkdtemp(resolve("reports/worker-configuration-ui-"));
const entry=join(run,"entry.tsx");
const fixture={dataDirectory:"synthetic-worker-configuration",workflows:[{project:{id:"project",root:"D:/synthetic/project"},state:{tasks:{}},approval:null}],projectSetups:[],workerAccountSetups:[],validators:[{id:"checks",name:"Synthetic project checks"}],managementDecisions:[],managementHistory:{projects:[]},openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:"D:/synthetic/codex.exe"},providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[]};
await writeFile(entry,`import React,{useState}from'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';
import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';import{ResourceFields,resourceError}from'../../apps/desktop/src/ResourceFields';
import{suggestedWorkerResource,synchronizeWorkerProfiles,automaticTaskDescription}from'../../apps/desktop/src/workerConfiguration';
const root=createRoot(document.getElementById('root'));window.commands=[];window.resourceWrites=[];window.mode='connections';
window.data=${JSON.stringify(fixture)};window.rules={resourceError,suggestedWorkerResource,synchronizeWorkerProfiles,automaticTaskDescription};
function Resources(){const[value,setValue]=useState(['database']);return <form><ResourceFields id="resource-test" value={value} common={window.common??[]} onChange={next=>{window.resourceWrites.push(next);setValue(next);}}/><button type="submit">Save test resource</button></form>;}
window.resources=()=>flushSync(()=>root.render(<Resources/>));
window.planning=()=>root.render(<ProjectPlanning data={window.data} busy={false} mode={window.mode} send={async command=>{window.commands.push(command);return true;}} onContinue={()=>{window.mode='planning';window.planning();}}/>);`);
await build({configFile:false,logLevel:"error",define:{"process.env.NODE_ENV":JSON.stringify("production")},oxc:{jsx:{runtime:"automatic"}},build:{outDir:join(run,"bundle"),emptyOutDir:false,lib:{entry,name:"WorkerConfiguration",formats:["iife"],fileName:()=>"ui.js"}}});
const browser=await chromium.launch({executablePath:"C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",headless:true});
const cases=[],errors=[];let page;
try{
  page=await browser.newPage({viewport:{width:1280,height:960}});page.setDefaultTimeout(10000);page.on("pageerror",error=>errors.push(error.message));
  const css=await readFile("apps/desktop/src/style.css","utf8");
  await page.route("**/*",route=>route.request().url()==="https://cerulean.test/"?route.fulfill({contentType:"text/html",body:`<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>`}):route.abort());
  await page.goto("https://cerulean.test/");await page.addScriptTag({path:join(run,"bundle/ui.js")});await page.evaluate(()=>window.resources());
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
  await openConnectionMethod(page,"执行端","api");
  const worker=page.getByRole("region",{name:"执行端",exact:true});
  for(const [index,model] of [[0,"worker-a"],[1,"worker-b"]]){
    await worker.getByRole("button",{name:"添加API 连接执行配置",exact:true}).click();
    assert.equal(await page.locator(`#profile-${index}-description`).inputValue(),await page.evaluate(()=>window.rules.automaticTaskDescription));
    await page.locator(`#profile-${index}-model`).fill(model);await page.locator(`#profile-${index}-description`).click();
    await page.waitForFunction(index=>/^executor-[a-f0-9]{64}$/.test(document.getElementById('profile-'+index+'-resources')?.value??''),index);
  }
  assert.equal(await page.locator('[id^="profile-"][id*="-check-"]').count(),0);
  assert.equal(await page.getByRole("button",{name:"增加检查",exact:true}).count(),0);
  assert.equal(await worker.getByText("未启用异常监督",{exact:true}).count(),0);
  await page.locator("#profile-0-description").fill("手动优先负责数据处理");
  await page.locator("#profile-0-artifacts").fill("src/owned.ts");await page.locator("#profile-0-description").click();
  await page.locator("#profile-1-artifacts").fill("SRC/owned.ts");
  assert.equal(await page.locator("#profile-0-artifacts").inputValue(),"src/owned.ts");
  await page.locator("#profile-1-description").click();
  assert.equal(await page.locator("#profile-0-artifacts").inputValue(),"");
  await page.getByRole("button",{name:"下一步：设置需求与计划",exact:true}).click();assert.equal(await page.locator("#planning-requirement").isVisible(),false);
  await page.locator("#profile-0-artifacts").fill("src/other.ts");await page.locator("#profile-0-description").click();
  cases.push("New workers receive editable automatic assignment descriptions and resources, inherit project checks, and transfer conflicting file ownership only on committed edits");

  await page.locator("#profile-1-model").fill("worker-a");await page.locator("#profile-1-description").click();
  await page.waitForFunction(()=>document.getElementById('profile-0-resources').value===document.getElementById('profile-1-resources').value);
  await page.locator("#profile-0-resources").fill("database\nport_8080");
  await page.waitForFunction(()=>document.getElementById('profile-1-resources').value==='database\nport_8080');
  await page.locator("#profile-0-resources").fill("database");await page.waitForFunction(()=>document.getElementById('profile-1-resources').value==='database');
  await page.locator("#profile-1-id").fill("profile-1");await page.locator("#profile-1-description").click();
  assert.equal(await page.locator("#profile-1-id").inputValue(),"profile-1-1");
  cases.push("Manual resources synchronize across the same executor, removals synchronize too, and duplicate configuration IDs receive an available ID");

  const next=page.getByRole("button",{name:"下一步：设置需求与计划",exact:true});await next.click();
  await page.locator("#planning-requirement").fill("Synthetic project requirement");await page.locator("#planning-artifacts").fill("primary.txt");
  await page.getByLabel("必须通过的断言 ID（每行一个）",{exact:true}).fill("project_assertion");
  await page.getByRole("button",{name:"保存需求与执行配置",exact:true}).click();
  const submitted=await page.evaluate(()=>window.commands.find(command=>command.type==='configureProject')?.draft);assert(submitted);
  for(const profile of submitted.profiles){assert.deepEqual(profile.acceptance,submitted.acceptance);assert.equal(profile.supervisionOverride,undefined);assert.deepEqual(profile.resourceKeys,['database']);}
  assert.equal(submitted.profiles[0].description,"手动优先负责数据处理");assert.equal(submitted.supervision.deliveryEnabled,true);assert.equal(submitted.supervision.maxAdvicePerAttempt,1);
  cases.push("The actual save command carries shared project check settings, independent task preferences, validated resources and the deputy supervision policy");
  await page.screenshot({path:join(run,"unified-worker-settings.png"),fullPage:true});assert.deepEqual(errors,[]);
  await writeFile(join(run,"result.json"),JSON.stringify({status:"passed",cases,errors,providerCalls:0},null,2));console.log(JSON.stringify({status:"passed",cases:cases.length,report:run}));
}catch(error){await page?.screenshot({path:join(run,"failure.png"),fullPage:true}).catch(()=>{});await writeFile(join(run,"failure.json"),JSON.stringify({error:String(error),cases,errors},null,2));throw error;}
finally{await browser.close();}
