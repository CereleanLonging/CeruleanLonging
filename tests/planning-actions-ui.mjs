import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { build } from "vite";
import { chromium } from "playwright-core";
import { openConfiguredConnections, confirmConnectionContinue } from "./connection-ui-helpers.mjs";

const run = await mkdtemp(resolve("reports/planning-actions-ui-"));
assert(process.env.CERULEAN_CACHE_SESSION);
const bundle = join(process.env.CERULEAN_CACHE_SESSION, "planning-actions-ui");
const entry = join(run, "entry.tsx");
const snapshot = {
  dataDirectory: "synthetic-layout", workflows: [{ project: { id: "project", root: "D:/synthetic/project" }, state: { tasks: {} }, approval: null }],
  projectSetups: [], workerAccountSetups: [], validators: [{ id: "cerulean-objective-checker", name: "Synthetic checks" }],
  managementDecisions: [], managementHistory: { projects: [] }, openaiConnection: { models: [], keyConfigured: false },
  codexConnection: { models: [], executable: "D:/synthetic/codex.exe" }, providerConnections: [], projectRuns: [], codexJobs: [],
  apiJobs: [], adapters: [], desktopBindings: [],
};
await writeFile(entry, `import React from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';
const root=createRoot(document.getElementById('root'));window.commands=[];window.continues=0;
window.show=(mode,busy=false,data=${JSON.stringify(snapshot)})=>flushSync(()=>root.render(<ProjectPlanning key={mode+busy} data={data} mode={mode} busy={busy} onContinue={()=>window.continues++} send={async command=>{window.commands.push(command);return true;}}/>));`);
await build({ configFile: false, logLevel: "error", define: { "process.env.NODE_ENV": JSON.stringify("production") }, oxc: { jsx: { runtime: "automatic" } }, build: { outDir: bundle, emptyOutDir: false, lib: { entry, name: "PlanningActions", formats: ["iife"], fileName: () => "ui.js" } } });
const browser = await chromium.launch({ executablePath: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", headless: true });
const cases = [], errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.on("pageerror", error => errors.push(String(error)));
  const css = await readFile("apps/desktop/src/style.css", "utf8");
  await page.route("**/*", route => route.request().url() === "https://cerulean.test/"
    ? route.fulfill({ contentType: "text/html", body: `<html lang="zh-CN"><meta charset="utf-8"><style>${css}body{padding:24px}#root{max-width:1080px;margin:auto}</style><div id="root"></div></html>` })
    : route.abort());
  await page.goto("https://cerulean.test/");
  await page.addScriptTag({ path: join(bundle, "ui.js") });
  const button = name => page.getByRole("button", { name, exact: true });
  const load = button("载入下一版草稿");
  await page.evaluate(() => window.show("connections"));
  await page.waitForTimeout(100);
  assert.deepEqual(errors, []);
  const next = button("下一步：设置需求与计划");
  await next.waitFor();
  assert.equal(await button("检查并保存接入配置").count(), 0);
  const loadBox = await load.boundingBox(), nextBox = await next.boundingBox();
  assert.equal(loadBox.y, nextBox.y);
  assert.equal(loadBox.height, nextBox.height);
  assert.equal(nextBox.height, 40);
  assert(nextBox.x >= loadBox.x + loadBox.width + 10);
  assert(nextBox.x <= loadBox.x + loadBox.width + 16);
  await page.locator(".planning-actions").screenshot({ path: join(run, "connections-actions.png") });
  await load.click();
  await page.getByText("没有可载入的下一版草稿。", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.commands), []);
  cases.push("A single Next action sits beside the draft button with matching height and compact spacing");

  await next.click();
  assert.equal(await page.getByRole('dialog').count(), 0);
  assert.equal(await page.locator('#planning-chief').evaluate(element=>element.validity.valueMissing), true);
  await openConfiguredConnections(page);
  await page.locator('#planning-chief').fill('chief');
  await page.locator('#planning-deputy').fill('deputy');
  await page.locator('#planning-worker').fill('worker');
  const storageKey = JSON.stringify(['cerulean-access-draft',1,'synthetic-layout','project']);
  await next.click();
  const dialog = page.getByRole('dialog',{name:'确认接入配置',exact:true});
  await dialog.waitFor();
  assert.equal(await page.evaluate(()=>window.continues), 0);
  assert.equal(await page.evaluate(key=>JSON.parse(localStorage.getItem(key)).fieldsChecked,storageKey), false);
  await dialog.screenshot({path:join(run,'confirm-access.png')});
  await dialog.getByRole('button',{name:'返回修改',exact:true}).click();
  assert.equal(await page.locator('#planning-worker').inputValue(),'worker');
  await next.click();await page.keyboard.press('Escape');
  await dialog.waitFor({state:'hidden'});
  assert.equal(await next.evaluate(element=>element===document.activeElement),true);
  assert.equal(await page.evaluate(()=>window.continues),0);
  cases.push("Missing fields stay on configuration; return and Escape preserve the draft and restore focus");

  await next.click();
  await page.evaluate(()=>{window.originalSetItem=Storage.prototype.setItem;Storage.prototype.setItem=function(){throw new DOMException('fixture quota','QuotaExceededError');};});
  await dialog.getByRole('button',{name:'确认保存并继续',exact:true}).click();
  await dialog.getByRole('alert').waitFor();
  assert.equal(await page.evaluate(()=>window.continues),0);
  assert(await dialog.isVisible());
  await page.evaluate(()=>Storage.prototype.setItem=window.originalSetItem);
  await confirmConnectionContinue(page);
  assert.equal(await page.evaluate(()=>window.continues),1);
  assert.equal(await page.evaluate(key=>JSON.parse(localStorage.getItem(key)).fieldsChecked,storageKey),true);
  assert.deepEqual(await page.evaluate(()=>window.commands),[]);
  cases.push("Confirmation waits for persistence; write failure remains visible and retry advances exactly once");

  for (const width of [940,640,440]) {
    await page.setViewportSize({width,height:900});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.locator('.planning-actions').screenshot({path:join(run,`connections-actions-${width}.png`)});
  }
  await next.click();
  assert(await dialog.evaluate(element=>element.getBoundingClientRect().right<=innerWidth));
  await dialog.screenshot({path:join(run,'confirm-access-narrow.png')});
  await dialog.getByRole('button',{name:'返回修改',exact:true}).click();
  await page.setViewportSize({width:1280,height:900});
  cases.push("The merged controls and confirmation dialog fit wide and narrow windows");

  await page.evaluate(() => window.show("all"));
  const save = button("保存需求与执行配置"), generate = button("请求主指挥生成方案");
  await save.waitFor();
  assert.equal(await page.getByRole('region', {name:'异常监督配置',exact:true}).count(),0);
  assert.equal(await page.locator('#supervision-role').count(),0);
  for (const name of ['验收机制只读关联','唯一客观检查入口']) assert.equal(await page.getByRole('region',{name,exact:true}).count(),0);
  assert.equal(await page.locator('#planning-review-tool,#planning-functional-chief,#workbench-checker').count(),0);
  assert.equal((await save.boundingBox()).y, (await load.boundingBox()).y);
  assert.equal((await save.boundingBox()).height, (await generate.boundingBox()).height);
  assert.equal(await save.evaluate(element => element.form?.tagName), "FORM");
  const budgetSpacing = await page.locator(".currency-budget-actions").evaluate(element => ({
    before: element.getBoundingClientRect().top - element.previousElementSibling.getBoundingClientRect().bottom,
    after: element.nextElementSibling.getBoundingClientRect().top - element.getBoundingClientRect().bottom,
  }));
  assert(budgetSpacing.before >= 16 && budgetSpacing.after >= 24);
  await page.locator(".currency-budget-actions").screenshot({ path: join(run, "budget-action.png") });
  await page.locator(".planning-request-actions").screenshot({ path: join(run, "plan-request.png") });
  cases.push("Budget action has section spacing; save and draft share a row; plan generation has a separate aligned area");

  await page.locator("#planning-requirement").fill("Synthetic layout regression");
  await page.locator("#planning-chief").fill("chief");
  await page.locator("#planning-deputy").fill("deputy");
  await page.locator("#planning-worker").fill("worker");
  await page.getByLabel('单项客观检查时限（秒）',{exact:true}).fill('70');

  await save.click();
  await page.waitForFunction(() => window.commands.length === 1);
  assert.equal(await page.evaluate(() => window.commands[0].type), "configureProject");
  assert.deepEqual(await page.evaluate(() => window.commands[0].draft.supervision), {schemaVersion:1,repeatedFailureThreshold:2,maxAdvicePerAttempt:1,deliveryEnabled:true});
  assert.equal(await page.evaluate(() => window.commands[0].draft.validatorId),'cerulean-objective-checker');
  assert.equal(await page.evaluate(() => window.commands[0].draft.acceptance.stages[0].timeoutSeconds),70);
  cases.push("Both duplicate tool cards are removed, while the fixed checker binding and relocated timeout remain in the submitted plan");
  cases.push("Duplicate supervision card is removed while the deputy supervision policy is still saved");
  cases.push("Regrouped save still uses the original form validation and submit handler");

  for (const width of [940, 640]) {
    await page.setViewportSize({ width, height: 900 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    if (width === 640) {
      const boxes = await page.locator(".planning-request-heading").evaluate(element => ({
        text: element.firstElementChild.getBoundingClientRect().bottom,
        button: element.lastElementChild.getBoundingClientRect().top,
      }));
      assert(boxes.button >= boxes.text + 16);
    }
    await page.locator(".planning-request-actions").screenshot({ path: join(run, `plan-request-${width}.png`) });
  }
  cases.push("Minimum desktop and narrow layouts wrap without overflow or overlapping descriptions");
  await page.evaluate(() => window.show("all", true));
  assert(await save.isDisabled());
  assert(await load.isDisabled());
  assert(await generate.isDisabled());
  cases.push("Busy-state restrictions still cover all action buttons");
  assert.deepEqual(errors, []);
  const report = { passed: true, cases, errors, scope: "Production React with synthetic project data", modelCalls: 0 };
  await writeFile(join(run, "result.json"), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ ...report, run }));
} finally {
  await browser.close();
}
