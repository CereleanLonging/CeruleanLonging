// Real Tauri/local IPC/file restore against an isolated interrupted-task fixture.
// No credentials or model requests. The fixture is created by the ignored Rust test.
import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import assert from "node:assert/strict";
import { assertNativeRuntime } from "./native-runtime.mjs";

await assertNativeRuntime();
await mkdir("reports", { recursive: true });
const dir = await mkdtemp(resolve("reports/omp-file-restore-native-"));
const seeded = spawnSync(
  "cargo",
  [
    "test",
    "-p",
    "cerulean-core",
    "host::omp_restore_host::tests::seed_native",
    "--locked",
    "--",
    "--ignored",
    "--exact",
  ],
  {
    windowsHide: true,
    encoding: "utf8",
    timeout: 120000,
    env: { ...process.env, CERULEAN_OMP_NATIVE_ROOT: dir },
  },
);
await writeFile(join(dir, "seed.log"), seeded.stdout + seeded.stderr);
assert.equal(seeded.status, 0, seeded.stderr + seeded.stdout);
const data = join(dir, "data"),
  file = join(dir, "seed/project/result.txt");
const errors = [],
  screenshots = [];
let core, app, browser, page;
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
async function terminate(child) {
  if (!child || child.exitCode !== null) return;
  const exited = once(child, "exit");
  child.kill();
  await Promise.race([
    exited,
    new Promise((_, reject) =>
      setTimeout(
        () => reject(Error("Owned process did not exit")),
        8000,
      ).unref(),
    ),
  ]);
}
async function launch() {
  core = spawn(
    resolve("target/debug/cerulean-core.exe"),
    ["--serve", "--data-dir", data],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  core.stdout.resume();
  core.stderr.on("data", (b) => {
    if (b.toString().includes("panicked")) errors.push("core panic");
  });
  core.on("error", (e) => errors.push(e.message));
  // Wait for our own service to answer before the desktop can launch a second one.
  let ready = false;
  for (let i = 0; i < 40; i++) {
    assert.equal(core.exitCode, null);
    const r = spawnSync(
      resolve("target/debug/cerulean-core.exe"),
      ["--request", "--data-dir", data],
      {
        windowsHide: true,
        encoding: "utf8",
        timeout: 3000,
        input: JSON.stringify({
          id: "ready",
          protocolVersion: 1,
          command: { type: "snapshot" },
        }),
      },
    );
    if (r.status === 0) {
      ready = true;
      break;
    }
    await pause(100);
  }
  assert.ok(ready, "owned core ready");
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  await new Promise((r) => server.close(r));
  app = spawn(resolve("target/debug/cerulean-desktop.exe"), [], {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      CERULEAN_TEST_DATA_DIR: data,
      CERULEAN_TEST_WEBVIEW_ROOT: join(dir, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
    },
  });
  app.stdout.resume();
  app.on("error", (e) => errors.push(e.message));
  app.stderr.on("data", (b) => {
    if (b.toString().includes("panicked")) errors.push("native panic");
  });
  for (let i = 0; i < 80; i++) {
    assert.equal(app.exitCode, null);
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
        timeout: 1000,
      });
      break;
    } catch {
      await pause(250);
    }
  }
  assert.ok(browser, "native browser connected");
  for (let i = 0; i < 50; i++) {
    page = browser
      .contexts()
      .flatMap((c) => c.pages())
      .find((p) => p.url().includes("tauri.localhost"));
    if (page) break;
    await pause(200);
  }
  assert.ok(page);
  page.on("pageerror", (e) => errors.push(e.message));
  await page
    .getByText("本地核心已连接", { exact: true })
    .waitFor({ timeout: 20000 });
  await page.getByRole("button", { name: "需求与计划", exact: false }).click();
  await page
    .getByRole("region", { name: "OMP 文件恢复", exact: true })
    .waitFor();
}
async function close() {
  if (app && app.exitCode === null) {
    const exited = once(app, "exit");
    await page
      ?.evaluate(() =>
        window.__TAURI_INTERNALS__.invoke("plugin:window|close", {
          label: "main",
        }),
      )
      .catch(() => {});
    await Promise.race([
      exited,
      new Promise((_, reject) =>
        setTimeout(() => reject(Error("native close timed out")), 8000).unref(),
      ),
    ]);
  }
  await browser?.close();
  browser = null;
  page = null;
  app = null;
  await terminate(core);
  core = null;
}
const panel = () =>
  page.getByRole("region", { name: "OMP 文件恢复", exact: true });
const snap = () =>
  page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("core_request", {
      command: { type: "snapshot" },
    }),
  );
async function detail() {
  await panel()
    .getByRole("button", { name: "查看文件记录", exact: false })
    .click();
  await panel().getByRole("button", { name: "核对文件", exact: true }).click();
  await panel()
    .getByRole("heading", { name: "result.txt", exact: true })
    .waitFor();
}
async function openReview() {
  await panel()
    .getByRole("button", { name: "复核并还原", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "确认文件还原" });
  await dialog.waitFor();
  return dialog;
}
async function shot(name, locator = panel()) {
  const path = join(dir, name);
  await locator.screenshot({ path });
  screenshots.push(path);
}
try {
  await launch();
  const before = await snap();
  const workflow = before.workflows.find((w) => w.project.id === "project");
  assert.equal(workflow.state.tasks.task.state, "unknown");
  assert.equal(before.ompFiles, null);
  await detail();
  let dialog = await openReview();
  assert.equal(
    await dialog.getByRole("button", { name: "确认还原文件" }).isDisabled(),
    true,
  );
  await shot("review.png", dialog);
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  assert.equal(await readFile(file, "utf8"), "Interrupted task version\n");
  dialog = await openReview();
  await writeFile(file, "Local edit after review\n");
  await dialog.getByRole("checkbox").check();
  await dialog.getByRole("button", { name: "确认还原文件" }).click();
  await panel()
    .getByText("还原未确认完成。请重新读取记录，核对当前文件和还原结果。", {
      exact: true,
    })
    .waitFor();
  assert.equal(await readFile(file, "utf8"), "Local edit after review\n");
  await detail();
  dialog = await openReview();
  await dialog.getByText("Local edit after review", { exact: false }).waitFor();
  await dialog.getByRole("checkbox").check();
  await dialog.getByRole("button", { name: "确认还原文件" }).click();
  await panel()
    .getByText("文件已还原，任务仍待核对。", { exact: true })
    .waitFor();
  assert.equal(await readFile(file, "utf8"), "Original reviewed source\n");
  assert.equal(
    await panel()
      .getByRole("button", { name: "复核并还原", exact: true })
      .count(),
    0,
  );
  const after = await snap();
  assert.deepEqual(
    after.workflows.find((w) => w.project.id === "project"),
    workflow,
  );
  await shot("restored.png");
  await close();
  await launch();
  await detail();
  await panel()
    .getByText("文件已还原，任务仍待核对。", { exact: true })
    .waitFor();
  assert.equal(await readFile(file, "utf8"), "Original reviewed source\n");
  assert.equal(
    await panel()
      .getByRole("button", { name: "复核并还原", exact: true })
      .count(),
    0,
  );
  assert.deepEqual(
    (await snap()).workflows.find((w) => w.project.id === "project"),
    workflow,
  );
  await shot("reopened.png");
  assert.deepEqual(errors, []);
  await close();
  await writeFile(
    join(dir, "report.json"),
    JSON.stringify(
      {
        status: "passed",
        realModelCalls: 0,
        fullCoreRestarts: 1,
        checks: [
          "review-preview",
          "explicit-confirmation",
          "cancel-no-effect",
          "stale-review-rejected",
          "fresh-review-restores-original",
          "no-workflow-mutation",
          "no-repeat-button",
          "restart-preserves-result",
        ],
        screenshots,
        ownedProcessesStopped: true,
      },
      null,
      2,
    ),
  );
  console.log(JSON.stringify({ status: "passed", dir, screenshots }));
} catch (error) {
  await writeFile(
    join(dir, "failure.json"),
    JSON.stringify({ error: String(error), errors }, null, 2),
  );
  throw error;
} finally {
  await browser?.close().catch(() => {});
  await terminate(app);
  await terminate(core);
}
