// Read-only launch check of the branded release binary. No sample plans are created.
import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer, createConnection } from "node:net";
import { writeFile, mkdir } from "node:fs/promises";
import { resolve, join } from "node:path";
import assert from "node:assert/strict";
const reportDirectory = resolve(process.argv[2] ?? "reports");
const runtime = resolve("target/release/cerulean-runtime.exe");
const residentDirectory = join(process.env.APPDATA,"local.ceruleanlonging.workbench","workspace");
const endpointProbe = spawnSync(runtime,["--endpoint","--data-dir",residentDirectory],{windowsHide:true,encoding:"utf8",timeout:10000});
assert.equal(endpointProbe.status,0,endpointProbe.stderr);
const hadResident = await new Promise((done)=>{
  const probe=createConnection(endpointProbe.stdout.trim());
  probe.once("connect",()=>{probe.destroy();done(true);});
  probe.once("error",e=>{probe.destroy();done(e.code!=="ENOENT");});
});
await mkdir(reportDirectory, { recursive: true });
const server = createServer();
server.listen(0, "127.0.0.1");
await once(server, "listening");
const port = server.address().port;
await new Promise((r) => server.close(r));
const app = spawn(resolve("target/release/CeruleanLonging.exe"), [], {
  windowsHide: true,
  stdio: ["ignore", "pipe", "pipe"],
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
  },
});
let browser, page;
const errors = [];
app.stderr.on("data", (b) => {
  if (b.toString().includes("panicked")) errors.push(b.toString());
});
try {
  for (let i = 0; i < 80; i++) {
    if (app.exitCode !== null) throw Error(`Release exited ${app.exitCode}`);
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
        timeout: 1000,
      });
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  assert.ok(browser);
  for (let i = 0; i < 50; i++) {
    page = browser
      .contexts()
      .flatMap((c) => c.pages())
      .find((p) => p.url().includes("tauri.localhost"));
    if (page) break;
    await new Promise((r) => setTimeout(r, 200));
  }
  assert.ok(page);
  page.on("pageerror", (e) => errors.push(e.message));
  await page
    .getByText("本地核心已连接", { exact: true })
    .waitFor({ timeout: 20000 });
  assert.equal(await page.title(), "CeruleanLonging");
  const snap = await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("core_request", {
      command: { type: "snapshot" },
    }),
  );
  assert.equal(snap.product, "CeruleanLonging");
  assert.equal(snap.protocolVersion, 1);
  assert.equal(snap.adapters.length, 10);
  assert.ok(snap.adapters.every((adapter) => adapter.capabilities.length === 10 &&
    adapter.capabilities.every((capability) => capability.state === "unverified")));
  await page.screenshot({
    path: join(reportDirectory, "release-overview.png"),
    fullPage: true,
  });
  const exit = once(app, "exit");
  await page
    .evaluate(() =>
      window.__TAURI_INTERNALS__.invoke("plugin:window|close", {
        label: "main",
      }),
    )
    .catch(() => {});
  await Promise.race([
    exit,
    new Promise((_, reject) =>
      setTimeout(() => reject(Error("normal close timeout")), 8000).unref(),
    ),
  ]);
  assert.deepEqual(errors, []);
  const resident=spawnSync(runtime,["--request","--data-dir",residentDirectory],{
    windowsHide:true,encoding:"utf8",timeout:20000,input:JSON.stringify({id:"release-after-close",protocolVersion:1,command:{type:"snapshot"}}),
  });
  assert.equal(resident.status,0,resident.stderr);
  assert.equal(JSON.parse(resident.stdout).response.result.plans.length,snap.plans.length);
  const result = {
    at: new Date().toISOString(),
    status: "passed",
    binary: resolve("target/release/CeruleanLonging.exe"),
    dataDirectory: snap.dataDirectory,
    product: snap.product,
    protocol: snap.protocolVersion,
    plans: snap.plans.length,
    newPlansCreated: 0,
    normalWindowClose: true,
    residentSurvivesWindowClose: true,
    preexistingResidentPreserved: hadResident,
    errors,
  };
  await writeFile(
    join(reportDirectory, "release-launch.json"),
    JSON.stringify(result, null, 2),
  );
  console.log(JSON.stringify(result, null, 2));
} finally {
  await browser?.close().catch(() => {});
  if (app.exitCode === null) app.kill();
  if(!hadResident){
    const stopped=spawnSync(runtime,["--stop","--data-dir",residentDirectory],{windowsHide:true,encoding:"utf8",timeout:20000});
    assert.equal(stopped.status,0,stopped.stderr);
  }
}
