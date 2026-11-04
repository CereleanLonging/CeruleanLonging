import {openConfiguredConnections,openConnectionMethod} from "./connection-ui-helpers.mjs";
// Offline UI checks for role grouping, credential boundaries, and path-action layout.
import { writeFile, mkdtemp } from "node:fs/promises";
import { resolve, join } from "node:path";
import assert from "node:assert/strict";
import { build } from "vite";
import { chromium } from "playwright-core";

const dir=await mkdtemp(resolve("reports/integration-hub-ui-774-"));
const entry=join(dir,"entry.tsx");
await writeFile(entry,`import React from 'react';import {createRoot} from 'react-dom/client';import {flushSync} from 'react-dom';import {IntegrationHub} from '../../apps/desktop/src/IntegrationHub';import {ManagementSourceFields} from '../../apps/desktop/src/ManagementSourceFields';
const root=createRoot(document.getElementById('root'));let epoch=0;window.commands=[];window.routes=[];window.scanCalls=[];
window.__TAURI_INTERNALS__={invoke:async(name,args)=>{window.scanCalls.push({name,args});return {candidates:[],warnings:[],limited:false};}};
window.show=data=>flushSync(()=>root.render(<IntegrationHub key={++epoch} data={data} busy={false} send={async c=>{window.commands.push(c);return true;}} configure={role=>window.routes.push(role)}/>));
window.showManagement=draft=>{window.draft=draft;flushSync(()=>root.render(<ManagementSourceFields draft={draft} onChange={next=>window.showManagement(next)}/>));};`);
await build({configFile:false,logLevel:"error",define:{"process.env.NODE_ENV":JSON.stringify("production")},oxc:{jsx:{runtime:"automatic"}},build:{outDir:join(dir,"bundle"),emptyOutDir:false,lib:{entry,name:"IntegrationHubUI",formats:["iife"],fileName:()=>"ui.js"}}});
const data={workflows:[{project:{id:"fixture",root:"D:\\fixture"}}],adapters:[],desktopAccess:null,codexConnection:null,openaiConnection:{state:"available",keyConfigured:true,models:["fixture-model"],error:null},providerConnections:[{provider:"a6api",target:{kind:"a6ApiVip"},state:"listed",keyConfigured:true,models:[],listLimited:false,error:null,endpoint:"https://api.a6api.vip/v1",checkedAt:null}]};
let browser;
const cases=[],errors=[];
try {
  browser=await chromium.launch({executablePath:"C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",headless:true});
  const page=await browser.newPage({viewport:{width:1100,height:1000}});page.on("pageerror",e=>errors.push(String(e)));
  await page.setContent('<html><meta charset="utf-8"><main style="max-width:1000px;margin:24px auto;padding:0 12px"><div id="root"></div></main></html>');
  await page.addStyleTag({path:resolve("apps/desktop/src/style.css")});await page.addScriptTag({path:join(dir,"bundle/ui.js")});await page.evaluate(data=>window.show(data),data);
  for(const name of ["主指挥","副指挥","执行端"])assert.equal(await page.getByRole("heading",{level:2,name,exact:true}).count(),1);
  const chief=page.getByRole("region",{name:"主指挥",exact:true}),deputy=page.getByRole("region",{name:"副指挥",exact:true}),worker=page.getByRole("region",{name:"执行端",exact:true});
  assert.equal(await deputy.locator("details[open]").count(),0);assert.equal(await worker.locator("details[open]").count(),0);
  assert.equal(await chief.locator('input[type="password"]').count(),0);
  assert.equal(await page.getByRole("button",{name:"扫描本地 Codex",exact:true}).isVisible(),false);
  assert.equal(await page.evaluate(()=>window.commands.length),0);
  await page.screenshot({path:join(dir,"role-overview.png"),fullPage:true});
  cases.push("Three separate role headings; API/Key/CLI/desktop groups start collapsed without requests");

  await openConnectionMethod(page,'主指挥','key');
  assert.equal(await chief.locator('input[type="password"]').count(),1);
  assert.equal(await chief.getByRole("button",{name:"检查官方 API 连接",exact:true}).count(),0);
  await chief.getByLabel("OpenAI API Key",{exact:true}).fill("synthetic-test-value-12345");
  await openConnectionMethod(page,'主指挥','api');
  await openConnectionMethod(page,'主指挥','key');
  assert.equal(await chief.getByLabel("OpenAI API Key",{exact:true}).inputValue(),"");
  await openConnectionMethod(page,'主指挥','desktop');
  assert.equal(await chief.locator('input[type="password"]').count(),0);
  assert.equal(await chief.getByRole("button",{name:"扫描本地 Codex",exact:true}).count(),0);

  cases.push("Chief method panels are collapsed and exclusive; Key and API operations remain separate");

  await openConnectionMethod(page,'主指挥','api');
  await chief.getByLabel("API 服务",{exact:true}).selectOption("a6api");
  assert.equal(await chief.getByLabel("中转站地址（须与 Key 来源一致）").inputValue(),"a6ApiVip");
  await chief.getByRole("button",{name:"读取 A6API 模型列表",exact:true}).click();
  assert.deepEqual(await page.evaluate(()=>window.commands),[{type:"checkProviderModels",target:{kind:"a6ApiVip"}}]);
  assert.equal(await page.evaluate(()=>window.scanCalls.length),0);
  cases.push("Saved relay origin is retained; explicit API check sends only the selected endpoint");

  await deputy.locator("summary").filter({hasText:/^API 连接$/}).click();
  await deputy.locator("summary").filter({hasText:/^Key 管理$/}).click();
  await worker.locator("summary").filter({hasText:/^Key 管理$/}).click();
  const ids=await page.locator("[id]").evaluateAll(elements=>elements.map(e=>e.id));
  assert.equal(new Set(ids).size,ids.length);
  await deputy.getByRole("button",{name:"配置副指挥模型",exact:true}).click();
  assert.deepEqual(await page.evaluate(()=>window.routes),["deputy"]);
  assert.equal(await page.evaluate(()=>window.commands.length),1);
  cases.push("Role controls have unique IDs; model configuration navigation never saves or dispatches a task");

  await worker.locator("summary").filter({hasText:/^CLI 接入$/}).click();
  const scan=worker.getByRole("button",{name:"扫描本地 Codex",exact:true});
  assert.equal(await scan.isVisible(),true);
  const pathBox=await worker.getByLabel("Codex 程序完整路径").boundingBox(),scanBox=await scan.boundingBox(),checkBox=await worker.getByRole("button",{name:"检查现有 Codex 登录",exact:true}).boundingBox();
  assert.ok(scanBox.x>pathBox.x&&Math.abs(scanBox.y-pathBox.y)<3);assert.ok(checkBox.y>scanBox.y);assert.ok(Math.abs(checkBox.x-pathBox.x)<3);
  await worker.getByLabel("CLI 程序").selectOption("omp");assert.equal(await scan.count(),0);
  await worker.getByLabel("CLI 程序").selectOption("codex");
  await page.screenshot({path:join(dir,"cli-layout.png"),fullPage:true});
  await page.setViewportSize({width:390,height:844});
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
  await page.screenshot({path:join(dir,"narrow-layout.png"),fullPage:true});
  cases.push("Scanning is shown only for Codex CLI; path and scan align, login check follows below, narrow view fits");

  await page.evaluate(()=>window.showManagement({chiefModel:"chief-original",deputyModel:"deputy-original",managementSources:{chief:{kind:"openAi"},deputy:{kind:"openAi"}}}));
  await page.getByLabel("主指挥接入",{exact:true}).selectOption("codexSession");
  assert.equal(await page.getByLabel("主指挥模型 ID",{exact:true}).getAttribute("readonly"),"");
  assert.equal(await page.evaluate(()=>window.draft.deputyModel),"deputy-original");
  await page.getByLabel("副指挥接入",{exact:true}).selectOption("deepSeek");
  assert.equal(await page.evaluate(()=>window.draft.managementSources.chief.kind),"codexSession");
  assert.equal(await page.evaluate(()=>window.draft.chiefModel),"external-codex-session");
  assert.equal(await page.evaluate(()=>window.draft.deputyModel),"deepseek-flash");
  cases.push("Separate role model fields preserve the other role and existing Codex-session behavior");
  assert.deepEqual(errors,[]);await writeFile(join(dir,"result.json"),JSON.stringify({passed:true,cases,errors,scope:"offline production components; no credentials saved and no live service requests"},null,2));
  console.log(JSON.stringify({passed:true,cases:cases.length,report:dir}));
} catch(e) {await writeFile(join(dir,"failure.json"),JSON.stringify({error:String(e),cases,errors},null,2));throw e;}
finally {await browser?.close();}
