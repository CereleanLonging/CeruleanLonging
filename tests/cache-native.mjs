import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { access, mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB, "1", "Use the owned UI runner");
const run = await mkdtemp(resolve("reports/cache-native-794-"));
const data = join(run, "data");
const gui = resolve(process.env.CERULEAN_NATIVE_GUI || "target/debug/cerulean-desktop.exe");
const runtime = resolve(process.env.CERULEAN_NATIVE_RUNTIME || "target/debug/cerulean-runtime.exe");
const source = join(run, "source");
const payload = join(source, ".temp/build-cache/jobs/123/nested");
await mkdir(payload, { recursive: true });
await mkdir(join(source, '.temp/build-cache/jobs/124'), { recursive: true });
await mkdir(join(source, 'target/release/deps'), { recursive: true });
await writeFile(join(source, "Cargo.toml"), "[workspace]\n");
await writeFile(join(source, "package.json"), '{"name":"cerulean-longing"}');
await writeFile(join(source, "source.rs"), "source sentinel");
await writeFile(join(payload, "compiled.rlib"), Buffer.alloc(2048, 3));
let app, browser, page;
const cases = [];
const wait = async predicate => {
  for (let attempt = 0; attempt < 160; attempt++) {
    const result = await predicate();
    if (result) return result;
    await new Promise(resolve => setTimeout(resolve, 150));
  }
  throw Error("Native cache condition timed out");
};
const invoke = (action, projectRoot = source) => page.evaluate(
  args => window.__TAURI_INTERNALS__.invoke("build_cache_monitor", args), { action, projectRoot });
async function launch(round) {
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  app = spawn(gui, [], {
    windowsHide: true, stdio: "ignore", env: { ...process.env, CERULEAN_TEST_DATA_DIR: data,
      CERULEAN_TEST_WEBVIEW_ROOT: join(run, `webviews-${round}`),
      CERULEAN_TEST_ACCESS_ROOT: join(run, "settings"),
      CERULEAN_TEST_MAINTENANCE_ROOT: join(run, "maintenance"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1` },
  });
  browser = await wait(async () => { try { return await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 500 }); } catch { return null; } });
  page = await wait(() => browser.contexts().flatMap(context => context.pages()).find(page => page.url().includes("tauri.localhost")));
  page.setDefaultTimeout(20000);
  await page.getByText("本地核心已连接", { exact: true }).waitFor();
}
async function close() {
  await page.getByRole("button", { name: "退出工作台", exact: true }).click();
  await page.getByRole("button", { name: "确认退出并停止后台", exact: true }).click();
  await wait(() => app.exitCode !== null);
  await browser.close().catch(() => {});
}

try {
  await launch(1);
  await page.locator(".sidebar nav").getByRole("button", { name: "维护与升级", exact: false }).click();
  const panel = page.getByRole("region", { name: "累计缓存监控" });
  await panel.scrollIntoViewIfNeeded();
  await page.getByLabel("工作台源码目录").fill(source);
  await panel.getByRole("button", { name: "扫描全项目临时缓存" }).click();
  await panel.getByText("扫描体积已记录到提炼日志。").waitFor();
  assert.equal((await invoke("status")).totalBytes, 2048);
  assert.equal((await invoke("status")).entries.length, 1);
  assert.equal(await panel.locator('tbody tr').count(), 1);
  cases.push("Real maintenance panel scans the selected source fixture and records its size");
  await panel.screenshot({ path: join(run, "native-cache-before.png") });
  await panel.getByRole("button", { name: "清理剩余缓存" }).click();
  await panel.getByText("已清理 2,048 字节，保留空目录。", { exact: true }).waitFor();
  await assert.rejects(access(join(payload, "compiled.rlib")));
  assert.deepEqual(await readdir(payload), []);
  assert.equal(await readFile(join(source, "source.rs"), "utf8"), "source sentinel");
  cases.push("Native cleanup removes only cache files and preserves empty folders and source");
  const after = await invoke("status");
  assert.equal(after.totalBytes, 0);
  assert.deepEqual(after.entries, []);
  await panel.getByText('当前没有占用空间的缓存。', { exact: true }).waitFor();
  assert.equal(await panel.getByRole('table').count(), 0);
  assert.equal(after.cumulativeCleanedBytes, 2048);
  const logs = await readdir(join(source, "docs/build-cache/cleanup-logs"));
  assert(logs.some(name => name.endsWith("-completed.md")));
  await panel.screenshot({ path: join(run, "native-cache-after.png") });
  cases.push("Distilled local Markdown receipts and cumulative totals reflect actual deletion");
  await assert.rejects(invoke("clean", run));
  cases.push("Native bridge rejects a directory that is not a workbench source checkout");
  await close();
  await launch(2);
  assert.equal((await invoke("status")).cumulativeCleanedBytes, 2048);
  await close();
  cases.push("A fresh native process reads the persisted counters and exits normally");
  const result = { status: "passed", run, cases, providerCalls: 0, syntheticData: true, ownedGuiExited: true };
  await writeFile(join(run, "result.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} finally {
  if (app?.exitCode === null && page && !page.isClosed()) {
    await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("close_workbench")).catch(() => {});
    await wait(() => app.exitCode !== null).catch(() => {});
  }
  await browser?.close().catch(() => {});
  spawnSync(runtime, ["--workbench", "--data-dir", data, "service", "stop"], { windowsHide: true, encoding: "utf8", timeout: 20000 });
}
