// Production component in an isolated browser; no native IPC or model calls.
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, join } from "node:path";
import assert from "node:assert/strict";
import { build } from "vite";
import { chromium } from "playwright-core";

const dir = resolve("reports/project-timezone-235", process.argv[2] ?? "ui");
await mkdir(dir);
const entry = join(dir, "entry.tsx");
await writeFile(entry, `import React from 'react';import {createRoot} from 'react-dom/client';
import {ProjectRegistration} from '../../../apps/desktop/src/WorkflowPlans';
window.commands=[];window.succeed=true;
createRoot(document.getElementById('root')).render(<ProjectRegistration connected={true} busy={false}
send={async c=>{window.commands.push(c);return window.succeed;}}/>);`);
await build({ configFile: false, logLevel: "error",
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
  oxc: { jsx: { runtime: "automatic" } },
  build: { outDir: join(dir, "bundle"), emptyOutDir: false,
    lib: { entry, name: "Timezone235", formats: ["iife"], fileName: () => "ui.js" } } });
let browser;
const errors = [], cases = [];
let requests = 0;
try {
  browser = await chromium.launch({ executablePath: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(10000);
  page.on("pageerror", e => errors.push(String(e)));
  await page.route("**/*", route => {
    if (route.request().url() === "http://localhost/timezone-fixture")
      return route.fulfill({ contentType: "text/html", body: '<div id="root"></div>' });
    requests++; return route.abort();
  });
  await page.goto("http://localhost/timezone-fixture");
  assert.equal(await page.evaluate(() => window.isSecureContext && typeof crypto.randomUUID === "function"), true);
  await page.addScriptTag({ content: await readFile(join(dir, "bundle/ui.js"), "utf8") });
  const timezone = page.getByLabel("交接文档时区");
  const root = page.getByLabel("项目目录");
  const submit = page.getByRole("button", { name: "关联项目目录" });
  await timezone.waitFor();
  assert.equal(await timezone.inputValue(), "Asia/Shanghai");
  await root.fill("D:\\isolated-project");
  await submit.click();
  await page.waitForFunction(() => window.commands.length === 1);
  assert.equal((await page.evaluate(() => window.commands[0])).project.timezone, "Asia/Shanghai");
  cases.push("default Shanghai reaches registration");
  await root.fill("D:\\new-york-project");
  await timezone.fill("America/New_York");
  await submit.click();
  await page.waitForFunction(() => window.commands.length === 2);
  assert.equal((await page.evaluate(() => window.commands[1])).project.timezone, "America/New_York");
  cases.push("explicit IANA selection reaches registration");
  await page.evaluate(() => { window.succeed = false; });
  await root.fill("D:\\retry-project");
  await timezone.fill("Bad/Zone");
  await submit.click();
  await page.waitForFunction(() => window.commands.length === 3);
  assert.equal(await root.inputValue(), "D:\\retry-project");
  assert.equal(await timezone.inputValue(), "Bad/Zone");
  assert.equal(await page.evaluate(() => window.commands.length), 3);
  cases.push("host refusal retains form without automatic retry");
  assert.deepEqual(errors, []);
  assert.equal(requests, 0);
} catch (error) {
  await writeFile(join(dir, "failure.json"), JSON.stringify({ error: String(error), cases, errors, requests }, null, 2), { flag: "wx" });
  throw error;
} finally {
  if (browser) await browser.close();
  await writeFile(join(dir, "cleanup.json"), JSON.stringify({ browserClosed: Boolean(browser) }), { flag: "wx" });
}
await writeFile(join(dir, "result.json"), JSON.stringify({ cases, errors, requests, browserClosed: true, nativeCalls: 0 }, null, 2), { flag: "wx" });
console.log(`${cases.length} UI checks passed; owned browser closed`);
