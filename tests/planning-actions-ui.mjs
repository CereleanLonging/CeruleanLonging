import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { build } from "vite";
import { chromium } from "playwright-core";

const run = await mkdtemp(resolve("reports/planning-actions-ui-"));
const entry = join(run, "entry.tsx");
const snapshot = {
  dataDirectory: "synthetic-layout", workflows: [{ project: { id: "project", root: "D:/synthetic/project" }, state: { tasks: {} }, approval: null }],
  projectSetups: [], workerAccountSetups: [], validators: [{ id: "checks", name: "Synthetic checks" }],
  managementDecisions: [], managementHistory: { projects: [] }, openaiConnection: { models: [], keyConfigured: false },
  codexConnection: { models: [], executable: "D:/synthetic/codex.exe" }, providerConnections: [], projectRuns: [], codexJobs: [],
  apiJobs: [], adapters: [], desktopBindings: [],
};
await writeFile(entry, `import React from 'react';import{createRoot}from'react-dom/client';import{flushSync}from'react-dom';import{ProjectPlanning}from'../../apps/desktop/src/ProjectPlanning';
const root=createRoot(document.getElementById('root'));window.commands=[];
window.show=(mode,busy=false,data=${JSON.stringify(snapshot)})=>flushSync(()=>root.render(<ProjectPlanning key={mode+busy} data={data} mode={mode} busy={busy} send={async command=>{window.commands.push(command);return true;}}/>));`);
await build({ configFile: false, logLevel: "error", define: { "process.env.NODE_ENV": JSON.stringify("production") }, oxc: { jsx: { runtime: "automatic" } }, build: { outDir: join(run, "bundle"), emptyOutDir: false, lib: { entry, name: "PlanningActions", formats: ["iife"], fileName: () => "ui.js" } } });
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
  await page.addScriptTag({ path: join(run, "bundle/ui.js") });
  const button = name => page.getByRole("button", { name, exact: true });
  const load = button("载入下一版草稿");
  await page.evaluate(() => window.show("connections"));
  await page.waitForTimeout(100);
  assert.deepEqual(errors, []);
  const check = button("检查并保存接入配置"), next = button("下一步：设置需求与计划");
  await check.waitFor();
  const checkBox = await check.boundingBox(), loadBox = await load.boundingBox(), nextBox = await next.boundingBox();
  assert.equal(checkBox.y, loadBox.y);
  assert.equal(checkBox.y, nextBox.y);
  assert.equal(checkBox.height, nextBox.height);
  assert.equal(checkBox.height, 40);
  assert(loadBox.x >= checkBox.x + checkBox.width + 10);
  assert(nextBox.x > loadBox.x + loadBox.width);
  await page.locator(".planning-actions").screenshot({ path: join(run, "connections-actions.png") });
  await load.click();
  await page.getByText("没有可载入的下一版草稿。", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.commands), []);
  cases.push("Connection check and draft controls align with the next-step button; loading a draft never submits");

  await page.evaluate(() => window.show("all"));
  const save = button("保存需求与执行配置"), generate = button("请求主指挥生成方案");
  await save.waitFor();
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
  await page.locator("#planning-artifacts").fill("result.txt");
  await save.click();
  await page.waitForFunction(() => window.commands.length === 1);
  assert.equal(await page.evaluate(() => window.commands[0].type), "configureProject");
  cases.push("Regrouped save still uses the original form validation and submit handler");

  for (const width of [940, 640]) {
    await page.setViewportSize({ width, height: 900 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    if (width === 640) {
      const boxes = await page.locator(".planning-request-actions").evaluate(element => ({
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
