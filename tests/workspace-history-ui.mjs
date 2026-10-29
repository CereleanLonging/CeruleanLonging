import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import assert from "node:assert/strict";
import { build } from "vite";
import { chromium } from "playwright-core";
const dir = await mkdtemp("reports/workspace-history-ui-");
const entry = join(dir, "entry.tsx"),
  cases = [];
await writeFile(
  entry,
  `import React from 'react';import {createRoot} from 'react-dom/client';import {WorkspaceHistory} from '../../apps/desktop/src/WorkspaceHistory';const root=createRoot(document.getElementById('root'));window.reads=[];window.project=null;window.enabled=true;window.render=()=>root.render(<WorkspaceHistory projectId={window.project} enabled={window.enabled} read={async q=>{window.reads.push(q);if(window.fail)throw Error('unavailable');let value=q.type==='history'?{projectId:q.projectId,revision:'v1',offset:q.offset,total:2,nextOffset:q.offset===0?5:null,entries:[{sequence:q.offset===0?1:2,kind:'original',at:'now',principal:null}]}:{projectId:q.projectId,sequence:q.sequence,revision:'v1',originalJson:'{"original":"kept"}'};if(window.fault)value.revision='wrong';if(window.hold)return await new Promise(resolve=>window.finish=()=>resolve({type:q.type,value}));return {type:q.type,value};}}/>);window.render();`,
);
let browser;
const errors = [];
let network = 0;
try {
  await build({
    configFile: false,
    logLevel: "error",
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
    oxc: { jsx: { runtime: "automatic" } },
    build: {
      outDir: join(dir, "bundle"),
      emptyOutDir: false,
      lib: {
        entry,
        name: "HistoryUI",
        formats: ["iife"],
        fileName: () => "ui.js",
      },
    },
  });
  browser = await chromium.launch({
    executablePath:
      "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    headless: true,
  });
  const page = await browser.newPage();
  page.setDefaultTimeout(10000);
  page.on("pageerror", (e) => errors.push(e.message));
  await page.route("**/*", (r) => {
    network++;
    return r.abort();
  });
  await page.setContent('<html><div id="root"></div></html>');
  await page.addScriptTag({
    content: await readFile(join(dir, "bundle/ui.js"), "utf8"),
  });
  const button = (name) => page.getByRole("button", { name, exact: true });
  await button("查看历史 #1 原详情").waitFor();
  await button("下一页").click();
  await button("查看历史 #2 原详情").waitFor();
  assert.equal((await page.evaluate(() => window.reads.at(-1))).offset, 5);
  await button("上一页").click();
  await button("查看历史 #1 原详情").click();
  await page.getByRole("region", { name: "原历史详情" }).waitFor();
  assert.equal(await page.locator("pre").innerText(), '{"original":"kept"}');
  cases.push("server cursor, back page and original detail");
  await page.evaluate(() => (window.fault = true));
  await button("下一页").click();
  await page.getByRole("status").waitFor();
  assert.equal(await page.locator("pre").innerText(), '{"original":"kept"}');
  assert.equal(await button("下一页").isDisabled(), true);
  cases.push("wrong revision retains old content and disables continuation");
  await page.evaluate(() => (window.fault = false));
  await button("刷新历史首页").click();
  await page.locator("pre").waitFor({ state: "detached" });
  await page.evaluate(() => (window.hold = true));
  await button("查看历史 #1 原详情").evaluate((b) => {
    b.click();
    b.click();
  });
  await page.waitForFunction(() => typeof window.finish === "function");
  assert.equal(
    await page.evaluate(
      () => window.reads.filter((q) => q.type === "historyEntry").length,
    ),
    2,
  );
  await page.evaluate(() => {
    window.late = window.finish;
    window.hold = false;
    window.project = "other";
    window.render();
  });
  await page.getByRole("heading", { name: "项目 other 的执行账本" }).waitFor();
  await page.evaluate(() => window.late());
  assert.equal(await page.locator("pre").count(), 0);
  cases.push("double click and late detail after project switch isolated");
  await page.evaluate(() => (window.fail = true));
  await button("刷新历史首页").click();
  await page.getByRole("status").waitFor();
  assert.equal(await button("查看历史 #1 原详情").isDisabled(), true);
  cases.push("unavailable refresh preserves page");
  assert.equal(network, 0);
  assert.deepEqual(errors, []);
  assert.equal(
    await page.evaluate(() =>
      window.reads.some((q) => !["history", "historyEntry"].includes(q.type)),
    ),
    false,
  );
  await writeFile(
    join(dir, "result.json"),
    JSON.stringify(
      {
        status: "passed",
        cases,
        errors,
        network,
        modelCalls: 0,
        userDesktopActions: 0,
        scope: "controlled production WorkspaceHistory; not installed Tauri",
      },
      null,
      2,
    ),
  );
  console.log(
    JSON.stringify({ status: "passed", artifact: dir, cases: cases.length }),
  );
} catch (e) {
  await writeFile(
    join(dir, "failure.json"),
    JSON.stringify({ error: String(e), cases, errors }, null, 2),
  );
  throw e;
} finally {
  await browser?.close();
}
