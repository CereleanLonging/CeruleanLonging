// Production host, worker thread, stdio child and Tauri UI; deterministic protocol fixture.
// No credentials or real model. Only processes spawned here are stopped.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdir, mkdtemp, copyFile, readFile, readdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const mode = process.argv.find(a => a.startsWith("--mode="))?.slice(7) ?? "interrupted";
assert.ok(["interrupted", "completed", "ack-lost", "empty-interrupted", "empty-ack-lost"].includes(mode));
const lostAck = mode === "ack-lost" || mode === "empty-ack-lost";
await mkdir("reports", { recursive: true });
const dir = await mkdtemp(resolve("reports/codex-cancel-native-"));
const data = join(dir, "data"), runtime = resolve("target/debug/cerulean-core.exe");
const desktop = resolve("target/debug/cerulean-desktop.exe");
await mkdir(join(dir, "bin"));
await copyFile(resolve("target/debug/examples/codex_protocol_fixture.exe"), join(dir, "bin/codex.exe"));
let core, app, browser, page, fixture, result;
const errors = [], cleanups = [];
const reason = "用户要求停止当前执行，保留已有修改，暂停后续调度";
const ids = { projectId: "project", attemptId: "cancel-native" };
const pause = ms => new Promise(done => setTimeout(done, ms));
async function until(fn, label, count = 120) {
  for (let i = 0; i < count; i++) { const value = await fn(); if (value) return value; await pause(100); }
  throw Error(`Timed out: ${label}`);
}
function call(command) {
  return page.evaluate(command => window.__TAURI_INTERNALS__.invoke("core_request", { command }), command);
}
async function rejected(command) {
  const response = await page.evaluate(async command => {
    try { await window.__TAURI_INTERNALS__.invoke("core_request", { command }); return null; }
    catch (error) { return String(error); }
  }, command);
  assert.ok(response, "invalid request rejected by production core"); return response;
}
async function ledger() {
  const folder = join(data, "workflows/project/events");
  return Promise.all((await readdir(folder)).sort().map(async name => {
    const bytes = await readFile(join(folder, name));
    return { name, hash: createHash("sha256").update(bytes).digest("hex"), value: JSON.parse(bytes) };
  }));
}
function commands(records) { return records.map(r => r.value.body.event.command); }
async function protocolCounts() {
  const lines = (await readFile(fixture.protocolLog, "utf8")).trim().split(/\r?\n/).filter(Boolean).map(JSON.parse);
  return Object.fromEntries(["turn/start", "turn/interrupt", "thread/read"].map(method => [method, lines.filter(line => line.method === method).length]));
}
async function fixtureCore() {
  const build = spawnSync("cargo", ["test", "-p", "cerulean-core", "--lib", "--no-run", "--locked", "--message-format=json"], { windowsHide: true, encoding: "utf8", timeout: 120000 });
  await writeFile(join(dir, "fixture-build.log"), `${build.stdout}\n${build.stderr}`);
  assert.equal(build.status, 0, build.stderr);
  const executable = build.stdout.split(/\r?\n/).filter(Boolean).map(line => { try { return JSON.parse(line); } catch { return {}; } })
    .find(item => item.reason === "compiler-artifact" && item.target?.name === "cerulean_core" && item.profile?.test && item.executable)?.executable;
  assert.ok(executable);
  core = spawn(executable, ["--ignored", "--exact", "host::codex_cancel_native::native_codex_cancel_service_fixture", "--nocapture"], {
    windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, CERULEAN_CANCEL_NATIVE_DIR: dir, CERULEAN_CANCEL_MODE: mode },
  });
  let output = "";
  core.stdout.on("data", b => { output += b; }); core.stderr.on("data", b => { output += b; });
  core.on("error", error => errors.push(`fixture: ${error.message}`));
  core.once("exit", () => { void writeFile(join(dir, "fixture-service.log"), output); });
  fixture = await until(async () => {
    assert.equal(core.exitCode, null, output);
    try { return JSON.parse(await readFile(join(dir, "fixture-service.json"), "utf8")); }
    catch { return null; }
  }, "fixture running", 300);
  assert.equal(fixture.realModelCalls, 0);
  assert.equal(fixture.ownerPid, core.pid, "server is the exact child this script owns");
}
async function restartCore() {
  core = spawn(runtime, ["--serve", "--data-dir", data], { windowsHide: true, stdio: "ignore" });
  core.on("error", error => errors.push(`core: ${error.message}`));
  await until(() => {
    assert.equal(core.exitCode, null);
    const ready = spawnSync(runtime, ["--request", "--data-dir", data], { windowsHide: true, encoding: "utf8", timeout: 2000,
      input: JSON.stringify({ id: "ready", protocolVersion: 1, command: { type: "snapshot" } }) });
    return ready.status === 0;
  }, "core restart");
}
async function launchUi() {
  const listener = createServer().listen(0, "127.0.0.1"); await once(listener, "listening");
  const port = listener.address().port; await new Promise(done => listener.close(done));
  app = spawn(desktop, [], { windowsHide: true, stdio: "ignore", env: { ...process.env, CERULEAN_TEST_DATA_DIR: data,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1` } });
  app.on("error", error => errors.push(`desktop: ${error.message}`));
  await until(async () => {
    assert.equal(app.exitCode, null);
    try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 500 }); return true; }
    catch { return false; }
  }, "WebView2");
  page = await until(() => browser.contexts().flatMap(c => c.pages()).find(p => p.url().includes("tauri.localhost")), "Tauri page");
  page.on("pageerror", error => errors.push(error.message));
  await page.getByText("本地核心已连接", { exact: true }).waitFor({ timeout: 20000 });
  await page.getByRole("button", { name: "需求与计划", exact: false }).click();
}
async function closeUi() {
  if (!app) return;
  const exited = once(app, "exit");
  await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("plugin:window|close", { label: "main" })).catch(() => {});
  await Promise.race([exited, pause(8000).then(() => { throw Error("window exit timed out"); })]);
  await browser.close(); app = null; browser = null; page = null;
}
async function stopCore(label) {
  if (!core) return;
  const owned = core;
  if (owned.exitCode === null && owned.signalCode === null) {
    const exited = once(owned, "exit");
    const stopped = spawnSync(runtime, ["--stop", "--data-dir", data], { windowsHide: true, encoding: "utf8", timeout: 10000 });
    const forced = stopped.status !== 0;
    if (forced) owned.kill(); // Submitted/Unknown deliberately reject clean service exit.
    await Promise.race([exited, pause(8000).then(() => { throw Error("owned core exit timed out"); })]);
    cleanups.push({ label, gracefulStatus: stopped.status, forcedOwnedProcess: forced, pid: owned.pid, exitCode: owned.exitCode, signal: owned.signalCode });
  }
  core = null;
}
try {
  await fixtureCore(); await launchUi();
  const initial = await call({ type: "snapshot" });
  assert.equal(initial.codexJobs[0].state, "executing");
  assert.equal(initial.workflows[0].state.tasks.task.state, "running");
  assert.equal(initial.projectRuns[0].state, "active");
  assert.equal(initial.openaiConnection.keyConfigured, false);
  const candidate = initial.codexCancellations.find(c => c.attemptId === ids.attemptId);
  assert.ok(candidate);
  const command = { type: "cancelCodex", ...ids, evidenceDigest: candidate.evidenceDigest, reason };
  const before = await ledger();
  for (const invalid of [{ ...command, evidenceDigest: "0".repeat(64) }, { ...command, reason: " " }, { ...command, reason: "bad\0reason" }, { ...command, reason: "界".repeat(2667) }, { ...command, attemptId: "other-attempt" }]) await rejected(invalid);
  assert.deepEqual(await ledger(), before);
  assert.equal((await protocolCounts())["turn/interrupt"], 0);
  const row = page.locator('[data-codex-attempt="cancel-native"][data-codex-project="project"]');
  await row.getByRole("button", { name: "请求停止当前任务…", exact: true }).click();
  let dialog = page.getByRole("dialog", { name: "确认请求停止当前任务", exact: true });
  const submit = dialog.getByRole("button", { name: "确认请求停止", exact: true });
  assert.equal(await submit.isDisabled(), true);
  await dialog.getByLabel("停止原因", { exact: true }).fill("界".repeat(2667));
  assert.equal(await submit.isDisabled(), true);
  await dialog.getByLabel("停止原因", { exact: true }).fill(reason);
  await dialog.screenshot({ path: join(dir, "cancel-review.png") });
  await dialog.getByRole("button", { name: "返回检查", exact: true }).click();
  assert.deepEqual(await ledger(), before);
  await row.getByRole("button", { name: "请求停止当前任务…", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "确认请求停止当前任务", exact: true });
  await dialog.getByLabel("停止原因", { exact: true }).fill(reason);
  await dialog.getByRole("button", { name: "确认请求停止", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  await rejected(command);
  const terminalState = mode === "completed" ? "submitted" : lostAck ? "unknown" : "cancelled";
  const finished = await until(async () => { const s = await call({ type: "snapshot" }); return s.codexJobs[0].state === terminalState ? s : null; }, "worker terminal");
  assert.equal(finished.projectRuns[0].state, "paused");
  assert.equal(finished.workflows[0].state.tasks.task.state, terminalState);
  assert.equal(finished.workflows[0].state.tasks.task.verified, false);
  assert.equal(finished.codexCancellations.length, 0);
  const recorded = await ledger();
  if (mode === "empty-interrupted") {
    const proof = recorded.at(-1).value.body.context.execution;
    assert.equal(proof.kind, "started-turn-cancellation");
    assert.equal(proof.proof.observation.itemCount, 0);
    assert.equal(proof.proof.observation.itemsView, "full");
    assert.equal(proof.proof.observation.status, "interrupted");
    assert.equal(proof.proof.anchors.started.sequence, 5);
    assert.equal(proof.proof.anchors.cancellation.sequence, 6);
    assert.ok(!JSON.stringify(proof.proof).includes("promptSha256"));
    assert.ok(!JSON.stringify(proof.proof).includes("clientMessageId"));
  }
  const events = commands(recorded);
  assert.equal(events.filter(c => c.kind === "cancel-intent").length, 1);
  assert.equal(events.find(c => c.kind === "cancel-intent").reason, reason);
  assert.equal(events.filter(c => c.kind === "dispatch-intent").length, 1);
  const counts = await protocolCounts();
  assert.equal(counts["turn/start"], 1); assert.equal(counts["turn/interrupt"], 1);
  assert.deepEqual(recorded.slice(0, before.length), before);
  const badge = terminalState === "submitted" ? "已提交，待最终验收" : terminalState === "unknown" ? "停止结果待核对" : "本次执行已停止";
  await row.getByText(badge, { exact: true }).waitFor();
  await row.screenshot({ path: join(dir, "cancel-outcome.png") });
  await writeFile(join(dir, "initial-snapshot.json"), JSON.stringify(initial, null, 2));
  await writeFile(join(dir, "terminal-snapshot.json"), JSON.stringify(finished, null, 2));
  await closeUi(); await stopCore("first-core-restart");
  await restartCore(); await launchUi();
  const reopened = await call({ type: "snapshot" });
  assert.equal(reopened.codexJobs[0].state, terminalState);
  assert.equal(reopened.projectRuns[0].state, "paused");
  await rejected(command);
  if (lostAck) {
    const restoredRow = page.locator('[data-codex-attempt="cancel-native"][data-codex-project="project"]');
    await restoredRow.getByRole("button", { name: "核对原会话", exact: true }).click();
    await until(async () => (await call({ type: "snapshot" })).codexJobs[0].state === "cancelled", "read-only cancellation reconciliation");
    await restoredRow.getByText("本次执行已停止", { exact: true }).waitFor();
    await restoredRow.screenshot({ path: join(dir, "cancel-reconciled.png") });
  }
  const final = await call({ type: "snapshot" });
  if (mode === "empty-ack-lost") {
    const proof = (await ledger()).at(-1).value.body.context.execution;
    assert.equal(proof.kind, "started-turn-cancellation");
    assert.equal(proof.proof.observation.itemCount, 0);
    assert.equal(proof.proof.observation.itemsView, "full");
    assert.equal(proof.proof.observation.status, "interrupted");
  }
  const finalCounts = await protocolCounts();
  assert.equal(finalCounts["turn/start"], 1); assert.equal(finalCounts["turn/interrupt"], 1);
  assert.equal(commands(await ledger()).filter(c => c.kind === "cancel-intent").length, 1);
  assert.equal(final.codexJobs[0].state, mode === "completed" ? "submitted" : "cancelled");
  assert.equal(final.projectRuns[0].state, "paused");
  assert.equal(final.verificationJobs.length, 0);
  await writeFile(join(dir, "reopened-snapshot.json"), JSON.stringify(final, null, 2));
  assert.deepEqual(errors, []);
  await closeUi(); await stopCore("final-cleanup");
  result = { status: "passed", at: new Date().toISOString(), mode, realModelCalls: 0,
    scope: "production async worker and Tauri, deterministic stdio executable; actual core restart",
    protocolCounts: finalCounts, cancelIntents: 1, newModelDispatchesAfterCancel: 0,
    states: { before: "running", after: terminalState, afterRestartAndReconcile: final.codexJobs[0].state },
    checks: { invalidRequestAndReasonRejected: true, returnWritesNothing: true, runPausedBeforeControl: true,
      duplicateAndStaleRequestRejected: true, onlyOneInterrupt: true, originalEvidencePreserved: true,
      completionNotMislabelledCancelled: mode === "completed", missingAckRetainsUnknown: lostAck,
      noVerificationOrManagementCallAfterPause: true, coreRestartPreservesEvidence: true }, errors };
} catch (error) {
  await page?.screenshot({ path: join(dir, "failure.png"), fullPage: true }).catch(() => {});
  await writeFile(join(dir, "failure.json"), JSON.stringify({ error: String(error), errors }, null, 2)); throw error;
} finally {
  await browser?.close().catch(() => {});
  if (app?.exitCode === null && app?.signalCode === null) { const exited = once(app, "exit"); app.kill(); await exited; }
  await stopCore("failure-or-final-cleanup");
  await writeFile(join(dir, "cleanup.json"), JSON.stringify({ complete: true, ownedCoreStopped: !core, cleanups }, null, 2));
}
await writeFile(join(dir, "result.json"), JSON.stringify(result, null, 2));
console.log(JSON.stringify({ dir, ...result }, null, 2));
