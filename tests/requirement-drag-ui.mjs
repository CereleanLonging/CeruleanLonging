import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { build } from "vite";
import { chromium } from "playwright-core";

assert(process.env.CERULEAN_CACHE_SESSION, "Use the managed UI entry point");
const run = await mkdtemp(resolve("reports/requirement-drag-ui-"));
const bundle = join(process.env.CERULEAN_CACHE_SESSION, "requirement-drag-ui");
const entry = join(run, "entry.tsx");
await writeFile(entry, `
import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';
import {RequirementComposer} from '../../apps/desktop/src/RequirementComposer';
import {ProjectPlanning} from '../../apps/desktop/src/ProjectPlanning';
import {newApiWorker} from '../../apps/desktop/src/ApiWorkerFields';
import {defaultSupervision} from '../../apps/desktop/src/SupervisionFields';
const root=createRoot(document.getElementById('root'));
let epoch=0;
const originalRead=File.prototype.arrayBuffer;
File.prototype.arrayBuffer=function(){
  if(!window.holdRead)return originalRead.call(this);
  const file=this;
  return new Promise(resolve=>{window.releaseRead=async()=>resolve(await originalRead.call(file));});
};
window.__TAURI_INTERNALS__={invoke:async(name,args)=>{
  window.native.push({name,args});
  if(name==='file_import_window'){
    const request=args.request;
    if(request.operation==='start'){
      if(window.delayOpen)return new Promise(resolve=>window.releaseOpen=()=>resolve({session:'fixture-import'}));
      return {session:'fixture-import'};
    }
    if(request.operation==='poll')return window.holdImport?{status:'waiting'}:{status:'ready',name:window.importName||'窗口.md',bytes:new TextEncoder().encode(window.importText||'# 窗口需求\\n## 目标\\n导入 5 项。').length};
    if(request.operation==='read'){
      if(window.badImport)throw Error('导入文件摘要不符，内容已拒绝');
      const bytes=Array.from(new TextEncoder().encode(window.importText||'# 窗口需求\\n## 目标\\n导入 5 项。'));
      return {bytes,offset:bytes.length,done:true};
    }
    if(request.operation==='finish'||request.operation==='cancel')return {status:'closed'};
    throw Error('Unexpected import operation');
  }
  if(name!=='review_workflow_template')throw Error('Unexpected IPC '+name);
  JSON.parse(args.input);
  return {requirement:'工作流的预览需求',template:{name:'测试工作流',workflowType:'coding',steps:[{id:'one'}]}};
}};
function Composer(){
  const [text,setText]=useState('保留已有草稿'),[disabled,setDisabled]=useState(false);
  window.edit=setText;window.disable=setDisabled;
  const apply=value=>{window.applied.push(value);setText(value);};
  return <><label htmlFor="planning-requirement">项目需求</label><RequirementComposer contextKey={text}
    disabled={disabled} value={text} onChange={setText} onWorkflow={apply}/>
    <p id="outside">输入框以外</p></>;
}
window.showComposer=()=>{epoch++;window.native=[];window.applied=[];window.holdRead=false;window.releaseRead=undefined;
  window.holdImport=false;window.delayOpen=false;window.releaseOpen=undefined;window.badImport=false;window.importName=undefined;window.importText=undefined;
  flushSync(()=>root.render(<Composer key={epoch}/>));};
window.showPlan=mode=>{
  epoch++;window.commands=[];window.native=[];
  const chief=mode==='codex'?{kind:'codexSession',sessionId:'fixture-chief'}:{kind:'provider',endpoint:{kind:'deepSeek'}};
  const setup={projectId:'project',version:1,requirement:'已保存的需求',chiefDirectory:'D:/synthetic/project-chief',
    managementSources:{chief,deputy:{kind:'openAi'}},workerAdapter:'deepseek',
    apiWorker:{...newApiWorker('deepseek'),model:'worker'},chiefModel:'chief',deputyModel:'deputy',
    validatorId:'cerulean-objective-checker',check:{args:[],timeoutSeconds:60},artifactPaths:[],
    managementMaxRequests:10,policy:{maxRequests:3,maxSeconds:180,
      supervision:{...defaultSupervision,maxAdvicePerAttempt:1,deliveryEnabled:true}}};
  const data={dataDirectory:'synthetic-drag-'+epoch,workflows:[{project:{id:'project',root:'D:/synthetic/project'},state:{tasks:{}},approval:null}],
    projectSetups:mode==='unsaved'?[]:[{digest:'a'.repeat(64),setup}],workerAccountSetups:[],
    validators:[{id:'cerulean-objective-checker',name:'Fixture checker'}],managementDecisions:[],managementHistory:{projects:[]},
    openaiConnection:{models:[],keyConfigured:false},codexConnection:{models:[],executable:''},
    providerCredentials:[{provider:'deepseek',keyConfigured:mode==='api-ready'}],providerConnections:[],projectRuns:[],codexJobs:[],apiJobs:[],adapters:[],desktopBindings:[]};
  flushSync(()=>root.render(<ProjectPlanning key={epoch} mode="planning" data={data} busy={false}
    send={async command=>{window.commands.push(command);return true;}}/>));
};
`);
await build({ configFile: false, logLevel: "error", define: { "process.env.NODE_ENV": JSON.stringify("production") },
  oxc: { jsx: { runtime: "automatic" } },
  build: { outDir: bundle, emptyOutDir: false, lib: { entry, name: "RequirementDrag", formats: ["iife"], fileName: () => "ui.js" } } });
const browser = await chromium.launch({ executablePath: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", headless: true });
const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
page.setDefaultTimeout(10000);
const cases = [], errors = [];
let network = 0;
page.on("pageerror", error => errors.push(error.message));
async function transfer(type, files, selector = ".requirement-composer", directory = false) {
  return page.evaluate(({ type, files, selector, directory }) => {
    const dataTransfer = new DataTransfer();
    for (const file of files) dataTransfer.items.add(new File([file.text], file.name, { type: "text/plain" }));
    const originalEntry = Object.getOwnPropertyDescriptor(DataTransferItem.prototype, "webkitGetAsEntry");
    if (directory) Object.defineProperty(DataTransferItem.prototype, "webkitGetAsEntry", { configurable: true, value: () => ({ isDirectory: true }) });
    try {
      const event = new DragEvent(type, { dataTransfer, bubbles: true, cancelable: true });
      document.querySelector(selector).dispatchEvent(event);
      return event.defaultPrevented;
    } finally {
      if (directory && originalEntry) Object.defineProperty(DataTransferItem.prototype, "webkitGetAsEntry", originalEntry);
    }
  }, { type, files, selector, directory });
}
const md = [{ name: "需求.MD", text: "# 导入项目\n## 目标\n保留 5 个工作目录。\n## 验收\n367 项检查通过。" }];
const confirm = () => page.getByRole("button", { name: "确认读入草稿", exact: true });
const requirement = () => page.locator("#planning-requirement");
const request = () => page.getByRole("button", { name: "请求主指挥生成方案", exact: true });
try {
  const css = await readFile("apps/desktop/src/style.css", "utf8");
  await page.route("**/*", route => route.request().url() === "https://cerulean.test/"
    ? route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}</style><div id="root"></div></html>` })
    : (network++, route.abort()));
  await page.goto("https://cerulean.test/");
  await page.addScriptTag({ path: join(bundle, "ui.js") });
  await page.evaluate(() => window.showComposer());
  assert.equal(await transfer("dragenter", md), true);
  await page.locator(".requirement-composer.is-dragging").waitFor();
  await page.locator(".requirement-composer").screenshot({ path: join(run, "drag-hover.png") });
  await transfer("drop", md, "#planning-requirement");
  await confirm().waitFor();
  assert.equal(await requirement().inputValue(), "保留已有草稿");
  assert.equal(await page.locator(".is-dragging").count(), 0);
  assert.deepEqual(await page.evaluate(() => window.native), []);
  await confirm().click();
  assert((await requirement().inputValue()).includes("367"));
  cases.push("Markdown dropped on the textarea uses local extraction and only updates the draft after confirmation");

  await transfer("drop", [{ name: "flow.JSON", text: "{}" }], ".requirement-composer-toolbar");
  await page.getByRole("button", { name: "使用此工作流需求", exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.native.length), 1);
  assert.equal(await page.evaluate(() => window.native[0].name), "review_workflow_template");
  await page.getByRole("button", { name: "使用此工作流需求", exact: true }).click();
  assert.equal(await requirement().inputValue(), "工作流的预览需求");
  cases.push("Workflow JSON dropped on the toolbar uses the existing validation and confirmation flow");

  for (const files of [[{ name: "wrong.txt", text: "text" }], [...md, ...md]]) {
    await transfer("drop", files);
    await page.locator(".requirement-import-error").waitFor();
    assert.equal(await requirement().inputValue(), "工作流的预览需求");
    assert.equal(await page.getByRole("dialog").count(), 0);
  }
  await transfer("drop", [{ name: "folder.md", text: "" }], ".requirement-composer", true);
  await page.getByRole("alert").filter({ hasText: "请一次拖入一个" }).waitFor();
  cases.push("Unsupported files, multiple files and directories preserve the existing draft and display a local error");

  await page.evaluate(() => window.disable(true));
  await transfer("dragenter", md);
  await transfer("drop", md);
  assert.equal(await page.getByRole("dialog").count(), 0);
  assert.equal(await page.locator(".is-dragging").count(), 0);
  await page.evaluate(() => window.disable(false));
  await transfer("dragenter", md);
  assert.equal(await transfer("drop", md, "#outside"), true);
  assert.equal(await page.locator(".is-dragging").count(), 0);
  assert.equal(await page.getByRole("dialog").count(), 0);
  const textPrevented = await page.evaluate(() => {
    const dataTransfer = new DataTransfer();
    dataTransfer.setData("text/plain", "文本");
    const event = new DragEvent("drop", { dataTransfer, bubbles: true, cancelable: true });
    document.querySelector("#planning-requirement").dispatchEvent(event);
    return event.defaultPrevented;
  });
  assert.equal(textPrevented, false);
  cases.push("Disabled input rejects files, outside drops cannot navigate away, and ordinary text dragging remains available");

  await page.evaluate(() => { window.showComposer(); window.holdRead = true; });
  await transfer("drop", md);
  await page.waitForFunction(() => !!window.releaseRead);
  await page.evaluate(() => window.edit("较新的草稿"));
  await page.evaluate(() => window.releaseRead());
  await page.waitForTimeout(80);
  assert.equal(await requirement().inputValue(), "较新的草稿");
  assert.equal(await page.getByRole("dialog").count(), 0);
  cases.push("A delayed dropped-file read cannot overwrite a newer draft");

  await page.evaluate(() => window.showComposer());
  await transfer("drop", [{ name: "big.md", text: "# 大文档\n" + "保留正文。".repeat(16000) }]);
  await confirm().waitFor();
  assert((await page.getByLabel("提炼内容，可修改", { exact: true }).inputValue()).length > 65536);
  await page.keyboard.press("Escape");
  await transfer("drop", [{ name: "big.json", text: " ".repeat(65537) }]);
  await page.getByRole("alert").filter({ hasText: "64 KiB" }).waitFor();
  assert.equal(await page.evaluate(() => window.native.length), 0);
  await page.keyboard.press("Escape");
  assert.equal(await requirement().inputValue(), "保留已有草稿");
  cases.push("Markdown retains its uncapped input policy, workflow size validation still applies, and Escape cancels both imports");

  await page.evaluate(() => window.showComposer());
  const importWindow = () => page.getByRole("button", { name: "打开拖拽导入窗口", exact: true });
  await importWindow().click();
  await confirm().waitFor();
  assert.equal(await requirement().inputValue(), "保留已有草稿");
  assert.deepEqual(await page.evaluate(() => window.native.map(c => c.args.request.operation)), ["start", "poll", "read", "finish"]);
  await confirm().click();
  assert((await requirement().inputValue()).includes("导入 5 项"));
  cases.push("The isolated-window handoff finishes native integrity verification before local preview and requires draft confirmation");

  await page.evaluate(() => { window.showComposer(); window.importName='flow.json';window.importText='{}'; });
  await importWindow().click();
  await page.getByRole("button", { name: "使用此工作流需求", exact: true }).waitFor();
  assert.equal(await page.evaluate(() => window.native.at(-1).name), "review_workflow_template");
  await page.keyboard.press("Escape");
  assert.equal(await requirement().inputValue(), "保留已有草稿");
  cases.push("The separate picker routes workflow JSON through the existing confirmation dialog");

  await page.evaluate(() => { window.showComposer(); window.holdImport=true; });
  await importWindow().click();
  await page.getByRole("button", { name: "取消导入", exact: true }).click();
  await page.waitForFunction(() => !Array.from(document.querySelectorAll('button')).find(b => b.textContent==='打开拖拽导入窗口').disabled);
  assert((await page.evaluate(() => window.native)).some(c => c.args.request.operation==='cancel'));
  assert.equal(await page.getByRole('dialog').count(), 0);
  cases.push("Cancelling the helper closes its native session and preserves the draft");

  await page.evaluate(() => { window.showComposer(); window.delayOpen=true; });
  await importWindow().click();
  await page.waitForFunction(() => !!window.releaseOpen);
  await page.evaluate(() => window.edit('切换后的草稿'));
  await page.evaluate(() => window.releaseOpen());
  await page.waitForFunction(() => window.native.some(c => c.args.request.operation==='cancel'));
  assert.equal(await requirement().inputValue(), '切换后的草稿');
  assert.equal(await page.getByRole('dialog').count(), 0);
  cases.push("A late native launch after a context change is cancelled without reading the selected file");

  await page.evaluate(() => { window.showComposer(); window.badImport=true; });
  await importWindow().click();
  await page.getByRole('alert').filter({hasText:'摘要不符'}).waitFor();
  assert.equal(await page.getByRole('dialog').count(), 0);
  assert.equal(await requirement().inputValue(), '保留已有草稿');
  assert((await page.evaluate(() => window.native)).some(c => c.args.request.operation==='cancel'));
  cases.push("A failed native digest check displays a local error, cancels the handoff and leaves the draft intact");

  await page.evaluate(() => window.showPlan("unsaved"));
  await page.getByText("请先保存需求与执行配置，保存成功后再请求主指挥生成方案。", { exact: true }).waitFor();
  assert.equal(await request().isEnabled(), false);
  assert.equal(await page.locator("#planning-request-issue").innerText(), "请先保存需求与执行配置，保存成功后再请求主指挥生成方案。");
  cases.push("An unsaved setup explains the save prerequisite instead of reporting a missing credential from an absent source");

  for (const mode of ["codex", "api-ready"]) {
    await page.evaluate(mode => window.showPlan(mode), mode);
    await page.waitForFunction(() => !Array.from(document.querySelectorAll("button")).find(button => button.textContent === "请求主指挥生成方案").disabled);
    await request().click();
    assert.equal(await page.evaluate(() => window.commands.length), 1);
    assert.equal(await page.evaluate(() => window.commands[0].type), "requestChiefPlan");
    await requirement().fill("修改后的需求");
    assert.equal(await request().isEnabled(), false);
    await page.getByText("请先保存需求与执行配置，保存成功后再请求主指挥生成方案。", { exact: true }).waitFor();
  }
  cases.push("Saved Codex and credentialed API setups enable one plan request, while edited drafts must be saved again");

  await page.evaluate(() => window.showPlan("api-missing"));
  await page.getByText("请在接入管理中保存所选主指挥的 Key，然后请求生成方案。", { exact: true }).waitFor();
  assert.equal(await request().isEnabled(), false);
  cases.push("A saved API source checks its own provider credential and explains a real missing Key");
  assert.deepEqual(errors, []);
  assert.equal(network, 0);
  await writeFile(join(run, "result.json"), JSON.stringify({ passed: true, cases, errors, networkRequests: network,
    realModelCalls: 0, scope: "Production React and local extraction; synthetic browser file drops and recorded workflow IPC" }, null, 2));
  console.log(JSON.stringify({ passed: true, cases: cases.length, report: run }));
} catch (error) {
  await writeFile(join(run, "failure.json"), JSON.stringify({ error: String(error.stack), cases, errors, network }, null, 2));
  await page.screenshot({ path: join(run, "failure.png"), fullPage: true }).catch(() => {});
  throw error;
} finally {
  await browser.close();
}
