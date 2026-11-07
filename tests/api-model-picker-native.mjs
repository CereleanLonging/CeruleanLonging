import {confirmConnectionContinue} from './connection-ui-helpers.mjs';
// Native UI, encrypted synthetic credential and durable selections. Only the
// provider catalogue is a synthetic fixture, intercepted before any native send.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { chromium } from "playwright-core";
import { openConnectionMethod } from "./connection-ui-helpers.mjs";

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,"1");
assert(process.env.CERULEAN_CACHE_SESSION,"Managed temporary WebView storage required");
const run=await mkdtemp(resolve("reports/api-model-picker-native-"));
const data=join(run,"data"), project=join(run,"project"), marker="synthetic-offline-model-picker-key";
await mkdir(project);
const cases=[], processes=[], errors=[];
let app,browser,page,launchNumber=0;
async function until(predicate) {
  for (let attempt=0;attempt<160;attempt++) {
    const value=await predicate();if(value)return value;
    await new Promise(resolve=>setTimeout(resolve,150));
  }
  throw Error("Native model-picker wait expired");
}
const invoke=(name,args)=>page.evaluate(({name,args})=>window.__TAURI_INTERNALS__.invoke(name,args),{name,args});
async function launch() {
  const server=createServer().listen(0,"127.0.0.1");await once(server,"listening");const port=server.address().port;await new Promise(resolve=>server.close(resolve));
  app=spawn(process.env.CERULEAN_NATIVE_GUI,[],{windowsHide:true,stdio:"ignore",env:{...process.env,
    CERULEAN_TEST_DATA_DIR:data,CERULEAN_TEST_WEBVIEW_ROOT:join(process.env.CERULEAN_CACHE_SESSION,`${basename(run)}-webview-${++launchNumber}`),
    CERULEAN_TEST_ACCESS_ROOT:join(run,"settings"),CERULEAN_TEST_MAINTENANCE_ROOT:join(run,"maintenance"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});
  processes.push(app.pid);
  browser=await until(async()=>{assert.equal(app.exitCode,null);try{return await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:500});}catch{return null;}});
  page=await until(()=>browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().includes("tauri.localhost")));
  page.setDefaultTimeout(12000);page.on("pageerror",error=>errors.push(error.message));
  await page.getByText("本地核心已连接",{exact:true}).waitFor();
  await page.evaluate(()=>{
    // Native Tauri makes invoke non-writable. Intercept its custom-protocol
    // fetch instead, and probe the interception before saving any credential.
    const actual=window.fetch.bind(window);
    const response=value=>new Response(JSON.stringify(value),{headers:{'Content-Type':'application/json','Tauri-Response':'ok'}});
    window.catalogChecks=[];window.nativeModelCommands=[];window.catalogTarget=null;
    window.fetch=async(url,options)=>{
      const endpoint=new URL(String(url));
      if(endpoint.hostname!=='ipc.localhost')return actual(url,options);
      const name=decodeURIComponent(endpoint.pathname.slice(1));
      if(name==='model_picker_fixture_probe')return response({intercepted:true});
      const args=JSON.parse(options.body);
      if(name==='core_submit') {
        const command=args.command;
        if(command.type==='checkProviderModels') {
          window.catalogChecks.push(command);window.catalogTarget=command.target;
          return response({type:'receipt',value:{requestId:'offline-catalogue'}});
        }
        if(!['setProviderKey','registerProject','snapshot'].includes(command.type))return new Response('Unexpected native test mutation',{headers:{'Content-Type':'text/plain','Tauri-Response':'error'}});
        window.nativeModelCommands.push(command.type);
      }
      const result=await actual(url,options);
      if(name==='core_request'&&args.command.type==='snapshot'&&window.catalogTarget) {
        const value=await result.clone().json();
        const provider=value.providerConnections.find(item=>item.provider==='a6api');
        if(provider?.keyConfigured) Object.assign(provider,{state:'listed',target:window.catalogTarget,models:['DeepSeek-R1','qwen3-32b','claude-sonnet-4-5'],listLimited:false,error:null});
        return response(value);
      }
      return result;
    };
  });
  assert.deepEqual(await invoke('model_picker_fixture_probe',{}),{intercepted:true});
  await page.locator(".sidebar nav").getByRole("button",{name:"接入管理",exact:false}).click();
}
const modal=()=>page.getByRole("dialog",{name:"选择具体模型",exact:true});
async function choose(model,query) {
  await modal().waitFor();
  const read=modal().getByRole("button",{name:"读取模型目录",exact:true});
  if(await read.isVisible()) await read.click();
  await modal().getByRole("searchbox").fill(query);
  await modal().getByRole("radio",{name:model,exact:true}).check();
  await modal().getByRole("button",{name:"确认使用此模型",exact:true}).click();
  await modal().waitFor({state:"hidden"});
}
async function close() {
  if(await modal().isVisible()){
    await modal().getByRole('button',{name:'稍后选择',exact:true}).click();
    await modal().waitFor({state:'hidden'});
  }
  await page.getByRole("button",{name:"退出工作台",exact:true}).click();
  await page.getByRole("button",{name:"确认退出并停止后台",exact:true}).click();
  await until(()=>app.exitCode!==null);await browser.close();browser=null;app=null;page=null;
}
try {
  await launch();
  const role=name=>page.getByRole("region",{name,exact:true});
  await openConnectionMethod(page,"主指挥","key");
  await role("主指挥").getByLabel("凭证供应商",{exact:true}).selectOption("a6api");
  await role("主指挥").getByLabel("A6API API Key",{exact:true}).fill(marker);
  await role("主指挥").getByRole("button",{name:"加密保存 A6API Key",exact:true}).click();await modal().waitFor();
  assert(await modal().getByRole("button",{name:"确认使用此模型",exact:true}).isDisabled());
  assert.deepEqual(await page.evaluate(()=>window.catalogChecks),[]);
  const envelope=await readFile(join(data,"credentials/a6api.dpapi"));assert(!envelope.includes(Buffer.from(marker)));
  cases.push("Real native credential receipt opens the modal without a provider request; DPAPI envelope contains no synthetic plaintext key");
  await choose("DeepSeek-R1","dsr1");
  for (const [name,model,query] of [["副指挥","claude-sonnet-4-5","克劳德45"],["执行端","qwen3-32b","qw3 32b"]]) {
    await openConnectionMethod(page,name,"key");await role(name).getByLabel("凭证供应商",{exact:true}).selectOption("a6api");
    await choose(model,query);
  }
  const stored=await until(async()=>{
    const entries=await invoke("access_settings_load",{});
    return Object.keys(entries).filter(key=>JSON.parse(key)[3].startsWith('model-default:')).length===3 ? entries : null;
  });
  for (const [name,model] of [["chief","DeepSeek-R1"],["deputy","claude-sonnet-4-5"],["worker","qwen3-32b"]]) {
    const item=Object.entries(stored).find(([key])=>JSON.parse(key)[3]===`model-default:${name}`);assert(item,name);assert.equal(JSON.parse(JSON.parse(item[1])).model,model);
  }
  cases.push("All three independent role defaults are acknowledged by the real native settings whitelist and durable store");
  await invoke("core_submit",{command:{type:"registerProject",project:{id:"fixture",root:project,timezone:"UTC"}}});
  await page.locator(".sidebar nav").getByRole("button",{name:"需求与计划",exact:false}).click();
  await page.locator(".sidebar nav").getByRole("button",{name:"接入管理",exact:false}).click();
  await page.locator("#planning-chief").waitFor({state:"attached"});
  assert.equal(await page.locator("#planning-chief").inputValue(),"DeepSeek-R1");assert.equal(await page.locator("#planning-deputy").inputValue(),"claude-sonnet-4-5");
  assert.equal(await page.locator("#planning-worker").inputValue(),"qwen3-32b");
  await openConnectionMethod(page,"执行端","api");
  await role("执行端").getByRole("button",{name:"将API 连接设为默认执行端",exact:true}).click();
  const ompKey=page.getByRole('dialog',{name:'补充 OMP API Key',exact:true});
  await ompKey.waitFor();
  assert.equal(await page.locator('#planning-adapter').inputValue(),'omp');
  const checksBefore=await page.evaluate(()=>window.catalogChecks.length);
  await ompKey.getByLabel('DeepSeek API Key',{exact:true}).fill('synthetic-omp-credential-gate');
  await ompKey.getByRole('button',{name:'加密保存 DeepSeek Key',exact:true}).click();
  await ompKey.getByRole('status').filter({hasText:'Key 已加密保存'}).waitFor();
  assert.equal(await page.evaluate(()=>window.catalogChecks.length),checksBefore);
  const ompEnvelope=await readFile(join(data,'credentials/deepseek.dpapi'));
  assert(!ompEnvelope.includes(Buffer.from('synthetic-omp-credential-gate')));
  await ompKey.getByRole('button',{name:'返回配置',exact:true}).click();await ompKey.waitFor({state:'hidden'});
  await openConnectionMethod(page,"执行端","key");
  await role("执行端").getByRole("button",{name:"将Key 管理设为默认执行端",exact:true}).click();
  await modal().waitFor();await modal().getByRole('button',{name:'稍后选择',exact:true}).click();await modal().waitFor({state:'hidden'});
  await page.locator('#planning-adapter').selectOption('a6api');
  await page.locator('#planning-api-relay-origin').selectOption('a6ApiVip');
  await choose('qwen3-32b','qw3 32b');
  assert.equal(await page.locator('#planning-executable').count(),0);
  await page.getByRole('button',{name:'下一步：设置需求与计划',exact:true}).click();await confirmConnectionContinue(page);
  await page.getByRole('button',{name:'修改接入配置',exact:true}).click();
  await openConnectionMethod(page,"执行端","key");
  assert.equal(await page.locator('#planning-adapter').inputValue(),'a6api');
  assert(await role('执行端').getByLabel('A6API API Key',{exact:true}).isVisible());
  cases.push('Native OMP missing-Key dialog saves only encrypted credentials; switching to Key direct avoids OMP paths through Next/Back');
  await page.screenshot({path:join(run,"native-role-defaults.png")});await close();
  cases.push("A newly registered real project inherits the selected role models before normal shutdown");
  await launch();
  await page.waitForFunction(()=>document.querySelector('#planning-chief')?.value==='DeepSeek-R1'&&document.querySelector('#planning-deputy')?.value==='claude-sonnet-4-5'&&document.querySelector('#planning-worker')?.value==='qwen3-32b');
  assert.equal(await page.locator("#planning-chief").inputValue(),"DeepSeek-R1");assert.equal(await page.locator("#planning-deputy").inputValue(),"claude-sonnet-4-5");assert.equal(await page.locator("#planning-worker").inputValue(),"qwen3-32b");
  await openConnectionMethod(page,"副指挥","key");
  await role("副指挥").getByRole("button",{name:"选择或修改模型",exact:true}).click();
  assert.equal(await modal().getByRole("radio").count(),0);assert(await modal().getByRole("button",{name:"确认使用此模型",exact:true}).isDisabled());
  await choose("DeepSeek-R1","dsr1");
  assert.equal(await page.locator("#planning-deputy").inputValue(),"DeepSeek-R1");assert.equal(await page.locator("#planning-worker").inputValue(),"qwen3-32b");
  await role("副指挥").getByRole("button",{name:"选择或修改模型",exact:true}).click();await modal().getByRole("searchbox").fill("ds");
  await page.screenshot({path:join(run,"native-model-picker.png")});await page.keyboard.press("Escape");
  await close();cases.push("Fresh WebView process restores all choices, requires a fresh catalogue and persists an independent deputy edit");
  assert.deepEqual(errors,[]);
  const result={status:"passed",cases,run,processes,allGuiProcessesExited:true,providerCalls:0,syntheticCatalogue:true,syntheticCredentialsOnly:true,errors};
  await writeFile(join(run,"result.json"),JSON.stringify(result,null,2));console.log(JSON.stringify({status:"passed",cases:cases.length,report:run}));
} catch(error) {
  await page?.screenshot({path:join(run,"failure.png")}).catch(()=>{});
  await writeFile(join(run,"failure.json"),JSON.stringify({error:String(error),cases,errors},null,2));throw error;
} finally {
  if(app?.exitCode===null&&page&&!page.isClosed()){
    await page.keyboard.press('Escape').catch(()=>{});
    await close();
  }
  await browser?.close().catch(()=>{});
  if(app)await until(()=>app.exitCode!==null);
}
