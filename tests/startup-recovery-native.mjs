// Production native UI/core recovery of durable startup intent before assignment.
// Synthetic evidence only; no API key or real model. Every process is owned here.
import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";
import assert from "node:assert/strict";

const mode = process.argv.find(arg => arg.startsWith("--mode="))?.slice(7) ?? "managed";
assert.ok(["managed", "unmanaged", "missing-job"].includes(mode), "supported --mode");
const managed = mode !== "unmanaged", withJob = mode !== "missing-job";
const reports = resolve("reports");
await mkdir(reports, { recursive: true });
const dir = await mkdtemp(join(reports, `startup-${mode}-native-`));
const data = join(dir, "data");
const runtime = resolve("target/debug/cerulean-core.exe");
const desktop = resolve("target/debug/cerulean-desktop.exe");
const projectId = "project", taskId = "task", attemptId = "startup-native";
const operationId = managed ? "startup-deputy-1" : null;
const reason = "关闭尚未分配的本地启动记录，保留原证据与请求计数，远端会话另行核对";
const errors = [];
let core, app, browser, page, result;

async function startOwnCore() {
  assert.equal(core, undefined, "previous owned core has exited");
  core = spawn(runtime, ["--serve", "--data-dir", data], { windowsHide: true, stdio: "ignore" });
  core.on("error", error => errors.push(`core spawn: ${error.message}`));
  let ready = false;
  for (let i = 0; i < 80; i++) {
    assert.equal(core.exitCode, null, "owned core remains running");
    assert.equal(core.signalCode, null);
    assert.deepEqual(errors, []);
    const probe = spawnSync(runtime, ["--request", "--data-dir", data], {
      windowsHide: true, encoding: "utf8", timeout: 2000,
      input: JSON.stringify({ id: "startup-native-ready", protocolVersion: 1, command: { type: "snapshot" } }),
    });
    if (probe.status === 0) {
      const response = JSON.parse(probe.stdout).response;
      assert.equal(response.error, null);
      assert.ok(response.result, "owned core handshake returns a snapshot");
      ready = true; break;
    }
    await new Promise(done => setTimeout(done, 50));
  }
  assert.ok(ready, "isolated core is ready before launching the desktop");
}

async function waitOwnCoreExit(force) {
  if (!core) return;
  const owned = core;
  if (owned.pid && owned.exitCode === null && owned.signalCode === null) {
    const exiting = once(owned, "exit");
    if (force) owned.kill();
    await Promise.race([
      exiting,
      new Promise((_, reject) => setTimeout(() => reject(Error("owned core exit timed out")), 8000).unref()),
    ]);
  }
  core = undefined;
}

async function launch() {
  await startOwnCore();
  const listener = createServer();
  listener.listen(0, "127.0.0.1");
  await once(listener, "listening");
  const port = listener.address().port;
  await new Promise(done => listener.close(done));
  app = spawn(desktop, [], {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env, CERULEAN_TEST_DATA_DIR: data,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
    },
  });
  app.stdout.resume();
  app.on("error", error => errors.push(`native spawn: ${error.message}`));
  app.stderr.on("data", bytes => {
    if (bytes.toString().includes("panicked")) errors.push(bytes.toString());
  });
  for (let i = 0; i < 80; i++) {
    assert.equal(app.exitCode, null, "native desktop remains running");
    assert.equal(app.signalCode, null);
    assert.deepEqual(errors, []);
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1000 });
      break;
    } catch { await new Promise(done => setTimeout(done, 250)); }
  }
  assert.ok(browser, "native WebView2 connection");
  for (let i = 0; i < 50; i++) {
    page = browser.contexts().flatMap(context => context.pages()).find(candidate => candidate.url().includes("tauri.localhost"));
    if (page) break;
    await new Promise(done => setTimeout(done, 200));
  }
  assert.ok(page, "native production Tauri page");
  page.on("pageerror", error => errors.push(error.message));
  await page.getByText("本地核心已连接", { exact: true }).waitFor({ timeout: 20000 });
  await page.getByRole("button", { name: "需求与计划", exact: false }).click();
  await page.getByRole("heading", { name: "需求与计划", exact: true }).waitFor();
}

async function closeWindow() {
  const exiting = once(app, "exit");
  await page.evaluate(() => window.__TAURI_INTERNALS__.invoke("plugin:window|close", { label: "main" })).catch(() => {});
  await Promise.race([
    exiting,
    new Promise((_, reject) => setTimeout(() => reject(Error("normal window close timed out")), 8000).unref()),
  ]);
  await browser.close();
  app = null; browser = null; page = null;
}
function stopOwnCore() {
  return spawnSync(runtime, ["--stop", "--data-dir", data], { windowsHide: true, encoding: "utf8", timeout: 20000 });
}
async function snapshot() {
  return page.evaluate(() => window.__TAURI_INTERNALS__.invoke("core_request", { command: { type: "snapshot" } }));
}
async function rejectCommand(command) {
  const response = await page.evaluate(async value => {
    try {
      await window.__TAURI_INTERNALS__.invoke("core_request", { command: value });
      return { rejected: false };
    } catch (error) { return { rejected: true, error: String(error) }; }
  }, command);
  assert.equal(response.rejected, true, "invalid disposition must be rejected by production core");
  return response.error;
}

async function files(folder) {
  const names = (await readdir(folder)).sort();
  return Promise.all(names.map(async name => {
    const bytes = await readFile(join(folder, name));
    return { name, sha256: createHash("sha256").update(bytes).digest("hex"), record: JSON.parse(bytes.toString("utf8")) };
  }));
}
async function workflowFiles() { return files(join(data, "workflows", projectId, "events")); }
async function managementFiles() { return files(join(data, "management-decisions")); }
async function startupFiles() { return files(join(data, "codex-startups", projectId)); }
async function readDetail() {
  const response = await page.evaluate(ids => window.__TAURI_INTERNALS__.invoke("core_request", {
    command: { type: "readCodexStartup", ...ids },
  }), { projectId, attemptId });
  assert.equal(response.codexStartupDetail?.summary.attemptId, attemptId);
  assert.equal(response.codexStartupDetail?.summary.projectId, projectId);
  return response.codexStartupDetail;
}
function summary(snap) {
  const found = snap.codexStartups.find(item => item.projectId === projectId && item.attemptId === attemptId);
  assert.ok(found, "startup is discoverable independently of a mutable job");
  return found;
}
function assertClosed(snap, initial) {
  assert.deepEqual(snap.workflows, initial.workflows, "closing startup grants no workflow authority");
  const workflow = snap.workflows.find(item => item.project.id === projectId);
  assert.equal(workflow.sequence, 2);
  assert.equal(workflow.state.tasks[taskId].state, "pending");
  assert.equal(workflow.state.tasks[taskId].fence, 0);
  assert.equal(workflow.state.tasks[taskId].attempt ?? null, null);
  assert.equal(summary(snap).state, "closed");
  assert.equal(summary(snap).intentHash, summary(initial).intentHash);
  assert.equal(snap.codexStartups.length, initial.codexStartups.length);
  assert.equal(snap.managementHistory.total, initial.managementHistory.total, "request allowance is not refunded");
  assert.deepEqual(snap.managementDecisions.map(item => item.operationId).sort(), initial.managementDecisions.map(item => item.operationId).sort());
  if (managed) {
    const management = snap.managementDecisions.find(item => item.operationId === operationId);
    assert.equal(management?.state, "startupClosed");
    assert.equal(management?.canDiscard, false);
    assert.equal(management?.canReply, false);
    assert.equal(management?.hasDiscard, true);
  } else assert.deepEqual(snap.managementDecisions, initial.managementDecisions);
  assert.deepEqual(snap.projectRuns, initial.projectRuns);
  assert.ok(snap.projectRuns.every(run => run.state !== "active"));
  assert.equal(snap.unsentAssignments.length, 0);
  assert.equal(snap.openaiConnection.keyConfigured, false);
  assert.equal(snap.verificationJobs.length, 0);
  assert.equal(snap.codexJobs.length, withJob ? 1 : 0);
  if (withJob) {
    assert.equal(snap.codexJobs[0].attemptId, attemptId);
    assert.equal(snap.codexJobs[0].state, "abandoned");
  }
}

try {
  const seeded = spawnSync("cargo", ["test", "-p", "cerulean-core", "seed_native_startup_recovery_fixture", "--locked", "--", "--ignored", "--nocapture"], {
    windowsHide: true, encoding: "utf8", timeout: 120000,
    env: { ...process.env, CERULEAN_STARTUP_NATIVE_DIR: dir, CERULEAN_STARTUP_MODE: mode },
  });
  await writeFile(join(dir, "seed.log"), `${seeded.stdout ?? ""}\n${seeded.stderr ?? ""}`);
  assert.equal(seeded.status, 0, seeded.error?.message ?? seeded.stderr);
  const fixture = JSON.parse(await readFile(join(dir, "fixture.json"), "utf8"));
  assert.equal(fixture.realModelCalls, 0);
  const beforeWorkflow = await workflowFiles();
  const beforeManagement = await managementFiles();
  const beforeStartup = await startupFiles();
  assert.equal(beforeWorkflow.length, 2);
  assert.ok(beforeWorkflow.every(file => !["assign", "dispatch-intent", "started"].includes(file.record.body.event.command.kind)));
  assert.equal(beforeStartup.length, 1);
  const intent = beforeStartup.find(file => file.name === `${attemptId}.0.json`);
  assert.ok(intent, "durable startup intent exists before any workflow assignment");
  assert.equal(intent.record.body.evidence.kind, "intent");
  assert.equal(intent.record.body.previous, "genesis");
  if (managed) {
    assert.ok(beforeManagement.some(file => file.name === `${operationId}.0.json`));
    assert.ok(beforeManagement.some(file => file.name === `${operationId}.1.json`));
    assert.ok(!beforeManagement.some(file => [`${operationId}.2.json`, `${operationId}.3.json`, `${operationId}.4.json`].includes(file.name)));
  }

  await launch();
  const initial = await snapshot();
  assert.equal(summary(initial).state, "prepared");
  assert.equal(summary(initial).taskId, taskId);
  assert.equal(summary(initial).managementOperation, operationId);
  assert.equal(summary(initial).intentHash, intent.record.hash);
  assert.equal(initial.codexJobs.length, withJob ? 1 : 0);
  if (withJob) assert.equal(initial.codexJobs[0].state, "unknown");
  assert.equal(initial.codexStartupDetail, null, "full startup evidence loads only on request");
  assert.equal(initial.managementDetail, null);
  assert.equal(initial.openaiConnection.keyConfigured, false);
  assert.equal(initial.managementHistory.total, managed ? 2 : 1);
  assert.equal(await page.getByRole("button", { name: "核对原会话", exact: true }).count(), 0);
  const section = page.getByRole("region", { name: "编程启动记录", exact: true });
  const row = section.locator(`[data-startup-attempt="${attemptId}"][data-startup-project="${projectId}"]`);
  await row.getByRole("button", { name: "查看启动详情", exact: true }).click();
  const panel = section.locator(`[data-startup-detail="${attemptId}"]`);
  await panel.getByRole("button", { name: "关闭启动记录…", exact: true }).waitFor();
  await panel.getByText(/可能已经建立远端会话/).waitFor();
  const detail = await readDetail();
  assert.ok(detail.evidenceDigest, "supported startup has a review candidate");
  assert.equal(detail.blockedReason, null);
  assert.equal(detail.reason, null);
  assert.equal(detail.workflowSequence, 2);
  assert.equal(detail.protocol, "codex-durable-before-send-v1");
  assert.notEqual(detail.evidenceDigest, detail.summary.intentHash, "summary intent hash is not sufficient authority to close");
  const command = { type: "abandonCodexStartup", projectId, attemptId, evidenceDigest: detail.evidenceDigest, reason };
  const wrongSummaryError = await rejectCommand({ ...command, evidenceDigest: detail.summary.intentHash });
  const staleDigestError = await rejectCommand({ ...command, evidenceDigest: detail.evidenceDigest === "0".repeat(64) ? "1".repeat(64) : "0".repeat(64) });
  const emptyReasonError = await rejectCommand({ ...command, reason: "   " });
  const nulReasonError = await rejectCommand({ ...command, reason: "unsafe\u0000text" });
  const longReasonError = await rejectCommand({ ...command, reason: "界".repeat(2667) });
  assert.deepEqual(await workflowFiles(), beforeWorkflow);
  assert.deepEqual(await managementFiles(), beforeManagement);
  assert.deepEqual(await startupFiles(), beforeStartup);
  await writeFile(join(dir, "initial-snapshot.json"), JSON.stringify(initial, null, 2));
  await writeFile(join(dir, "initial-detail.json"), JSON.stringify(detail, null, 2));

  await panel.getByRole("button", { name: "关闭启动记录…", exact: true }).click();
  let dialog = page.getByRole("dialog", { name: "确认关闭编程启动记录", exact: true });
  await dialog.waitFor();
  await dialog.getByText(attemptId, { exact: true }).waitFor();
  await dialog.getByText(/不代表|不证明/).waitFor();
  const submit = dialog.getByRole("button", { name: "确认关闭启动记录", exact: true });
  assert.equal(await submit.isDisabled(), true);
  await dialog.getByLabel("启动记录关闭原因", { exact: true }).fill("   ");
  assert.equal(await submit.isDisabled(), true);
  await dialog.getByLabel("启动记录关闭原因", { exact: true }).fill("界".repeat(2667));
  assert.equal(await submit.isDisabled(), true, "UTF-8 byte limit is enforced");
  await dialog.getByLabel("启动记录关闭原因", { exact: true }).fill(reason);
  assert.equal(await submit.isDisabled(), false);
  await dialog.screenshot({ path: join(dir, "startup-close-review.png") });
  await dialog.getByRole("button", { name: "返回", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  assert.deepEqual(await workflowFiles(), beforeWorkflow);
  assert.deepEqual(await managementFiles(), beforeManagement);
  assert.deepEqual(await startupFiles(), beforeStartup, "returning from confirmation writes nothing");

  await panel.getByRole("button", { name: "关闭启动记录…", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "确认关闭编程启动记录", exact: true });
  await dialog.getByLabel("启动记录关闭原因", { exact: true }).fill(reason);
  await dialog.getByRole("button", { name: "确认关闭启动记录", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  await row.getByText("启动记录已关闭", { exact: true }).waitFor();
  await panel.getByText(reason, { exact: true }).waitFor();
  assert.equal(await panel.getByRole("button", { name: "关闭启动记录…", exact: true }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "核对原会话", exact: true }).count(), 0);
  if (withJob) await page.getByRole("region", { name: "编程执行记录", exact: true }).getByText("本次尝试已关闭", { exact: true }).waitFor();
  const saved = await snapshot();
  assertClosed(saved, initial);
  const savedDetail = await readDetail();
  assert.equal(savedDetail.evidenceDigest, null);
  assert.equal(savedDetail.reason, reason);
  assert.equal(savedDetail.summary.state, "closed");
  const afterStartup = await startupFiles();
  assert.equal(afterStartup.length, beforeStartup.length + 1);
  assert.deepEqual(afterStartup.find(file => file.name === intent.name), intent, "original startup identity remains immutable");
  const closed = afterStartup.find(file => file.name === `${attemptId}.1.json`);
  assert.ok(closed);
  assert.equal(closed.record.body.previous, intent.record.hash);
  assert.equal(closed.record.body.evidence.kind, "closure");
  assert.equal(closed.record.body.evidence.closure.reason, reason);
  assert.equal(closed.record.body.evidence.closure.workflowSequence, 2);
  const afterManagement = await managementFiles();
  assert.equal(afterManagement.length, beforeManagement.length + (managed ? 1 : 0));
  for (const original of beforeManagement) assert.deepEqual(afterManagement.find(file => file.name === original.name), original, "original management response remains immutable");
  if (managed) {
    assert.ok(afterManagement.some(file => file.name === `${operationId}.4.json`));
    assert.ok(!afterManagement.some(file => file.name === `${operationId}.2.json` || file.name === `${operationId}.3.json`));
  }
  assert.deepEqual(await workflowFiles(), beforeWorkflow);
  const duplicateError = await rejectCommand(command);
  assert.deepEqual(await startupFiles(), afterStartup, "late duplicate cannot close twice");
  assert.deepEqual(await managementFiles(), afterManagement);
  await new Promise(done => setTimeout(done, 1200));
  assertClosed(await snapshot(), initial);
  assert.deepEqual(await workflowFiles(), beforeWorkflow, "resident tick does not allocate or dispatch");
  await writeFile(join(dir, "saved-snapshot.json"), JSON.stringify(saved, null, 2));
  await writeFile(join(dir, "saved-detail.json"), JSON.stringify(savedDetail, null, 2));
  await panel.screenshot({ path: join(dir, "startup-closed.png") });

  await closeWindow();
  const stopped = stopOwnCore();
  assert.equal(stopped.status, 0, stopped.error?.message ?? stopped.stderr);
  await waitOwnCoreExit(false);
  await launch();
  const reopenedSection = page.getByRole("region", { name: "编程启动记录", exact: true });
  const reopenedRow = reopenedSection.locator(`[data-startup-attempt="${attemptId}"][data-startup-project="${projectId}"]`);
  await reopenedRow.getByText("启动记录已关闭", { exact: true }).waitFor();
  await reopenedRow.getByRole("button", { name: "查看启动详情", exact: true }).click();
  const reopenedPanel = reopenedSection.locator(`[data-startup-detail="${attemptId}"]`);
  await reopenedPanel.getByText(reason, { exact: true }).waitFor();
  assert.equal(await reopenedPanel.getByRole("button", { name: "关闭启动记录…", exact: true }).count(), 0);
  const reopened = await snapshot();
  assertClosed(reopened, initial);
  const reopenedDetail = await readDetail();
  assert.deepEqual(reopenedDetail, savedDetail);
  const restartDuplicateError = await rejectCommand(command);
  assert.deepEqual(await workflowFiles(), beforeWorkflow);
  assert.deepEqual(await managementFiles(), afterManagement, "reopening publishes no fake application or receipt");
  assert.deepEqual(await startupFiles(), afterStartup, "reopening does not rewrite startup ledger");
  await writeFile(join(dir, "reopened-snapshot.json"), JSON.stringify(reopened, null, 2));
  await writeFile(join(dir, "reopened-detail.json"), JSON.stringify(reopenedDetail, null, 2));
  await reopenedPanel.screenshot({ path: join(dir, "startup-reopened.png") });
  assert.deepEqual(errors, []);
  await closeWindow();
  result = {
    status: "passed", at: new Date().toISOString(), mode, managed, jobPresent: withJob,
    scope: "production native UI/core with synthetic durable startup, before assignment; full core restart included",
    realModelCalls: 0, newWorkflowEvents: 0, newDispatchIntents: 0, newStartupRecords: 1, newManagementRecords: managed ? 1 : 0,
    checks: { detailRequiredBeforeReview: true, startupVisible: true,
      summaryAndStaleDigestsRejected: true, invalidReasonsRejected: true,
      cancelledConfirmationWritesNothing: true, localClosureOnly: true,
      requestBudgetPreserved: true, originalEvidencePreserved: true,
      lateAndRestartDuplicatesRejected: true, noAutomaticAllocationOrDispatch: true,
      jobProjectionCorrect: true, invalidReconciliationHidden: true,
      closureAndReasonPersistAfterFullCoreRestart: true, noKeyConfigured: true },
    wrongSummaryError, staleDigestError, emptyReasonError, nulReasonError, longReasonError,
    duplicateError, restartDuplicateError, errors,
  };
} catch (error) {
  await page?.screenshot({ path: join(dir, "failure.png"), fullPage: true }).catch(() => {});
  await writeFile(join(dir, "failure.json"), JSON.stringify({ at: new Date().toISOString(), error: String(error), errors }, null, 2));
  throw error;
} finally {
  await browser?.close().catch(() => {});
  if (app?.exitCode === null && app?.pid) {
    const exiting = once(app, "exit"); app.kill();
    await Promise.race([exiting, new Promise((_, reject) => setTimeout(() => reject(Error("owned desktop exit timed out")), 8000).unref())]);
  }
  const cleanup = stopOwnCore();
  await writeFile(join(dir, "cleanup.json"), JSON.stringify({ status: cleanup.status, signal: cleanup.signal, error: cleanup.error?.message, stderr: cleanup.stderr }, null, 2));
  await waitOwnCoreExit(cleanup.status !== 0);
  if (result) assert.equal(cleanup.status, 0, cleanup.error?.message ?? cleanup.stderr);
  else if (cleanup.status !== 0) console.error("Terminated only the owned fixture core after graceful stop was refused:", cleanup.error?.message ?? cleanup.stderr);
}
await writeFile(join(dir, "result.json"), JSON.stringify(result, null, 2));
console.log(JSON.stringify({ dir, ...result }, null, 2));
