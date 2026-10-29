// Real native UI/core: explicitly close a prepared application that has no workflow event.
// Fixture-only state, no keys or API requests. Every process is owned by this script.
import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";
import assert from "node:assert/strict";

const reports = resolve("reports");
await mkdir(reports, { recursive: true });
const withoutJob = process.argv.includes("--without-job");
const dir = await mkdtemp(join(reports, withoutJob ? "application-no-job-native-" : "application-recovery-native-"));
const data = join(dir, "data");
const runtime = resolve("target/debug/cerulean-core.exe");
const desktop = resolve("target/debug/cerulean-desktop.exe");
const operationId = "application-fixture", projectId = "project", taskId = "task";
const attemptId = "prepared-native", sessionId = "fixture-prepared-session";
const reason = "关闭尚未提交工作流的应用事务，保留原记录与请求计数，暂不恢复调度";
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
      input: JSON.stringify({ id: "application-native-ready", protocolVersion: 1, command: { type: "snapshot" } }),
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
async function readDetail() {
  const response = await page.evaluate(id => window.__TAURI_INTERNALS__.invoke("core_request", { command: { type: "readManagementDetail", operationId: id } }), operationId);
  assert.equal(response.managementDetail?.operationId, operationId);
  return response.managementDetail;
}
function summary(snap) {
  const found = snap.managementDecisions.find(item => item.operationId === operationId);
  assert.ok(found); return found;
}
function assertClosed(snap, initial) {
  assert.deepEqual(snap.workflows, initial.workflows, "closure adds no workflow event or authority");
  const workflow = snap.workflows.find(item => item.project.id === projectId);
  assert.equal(workflow.sequence, 2);
  assert.equal(workflow.state.tasks[taskId].state, "pending");
  assert.equal(workflow.state.tasks[taskId].fence, 0);
  assert.equal(snap.managementHistory.total, initial.managementHistory.total, "request budget is not refunded");
  assert.deepEqual(snap.managementDecisions.map(item => item.operationId).sort(), initial.managementDecisions.map(item => item.operationId).sort());
  assert.equal(summary(snap).state, "abandoned");
  assert.equal(summary(snap).hasDiscard, true);
  assert.equal(summary(snap).canDiscard, false);
  assert.equal(summary(snap).canReply, false);
  assert.deepEqual(snap.projectRuns, initial.projectRuns);
  assert.ok(snap.projectRuns.every(run => run.state !== "active"));
  assert.equal(snap.unsentAssignments.length, 0, "closure does not create an assignment");
  assert.equal(snap.openaiConnection.keyConfigured, false);
  assert.equal(snap.verificationJobs.length, 0);
  assert.equal(snap.codexJobs.length, withoutJob ? 0 : 1);
  if (!withoutJob) {
    assert.equal(snap.codexJobs[0].attemptId, attemptId);
    assert.equal(snap.codexJobs[0].state, "abandoned");
  }
}

try {
  const seeded = spawnSync("cargo", ["test", "-p", "cerulean-core", "seed_native_application_recovery_fixture", "--locked", "--", "--ignored", "--nocapture"], {
    windowsHide: true, encoding: "utf8", timeout: 120000,
    env: { ...process.env, CERULEAN_APPLICATION_NATIVE_DIR: dir, CERULEAN_APPLICATION_NO_JOB: withoutJob ? "1" : "0" },
  });
  await writeFile(join(dir, "seed.log"), `${seeded.stdout ?? ""}\n${seeded.stderr ?? ""}`);
  assert.equal(seeded.status, 0, seeded.error?.message ?? seeded.stderr);
  const beforeWorkflow = await workflowFiles();
  const beforeManagement = await managementFiles();
  assert.equal(beforeWorkflow.length, 2);
  assert.ok(beforeWorkflow.every(file => !["assign", "dispatch-intent", "started"].includes(file.record.body.event.command.kind)));
  assert.ok(beforeManagement.some(file => file.name === `${operationId}.2.json`));
  assert.ok(!beforeManagement.some(file => file.name === `${operationId}.3.json` || file.name === `${operationId}.4.json`));

  await launch();
  const initial = await snapshot();
  assert.equal(summary(initial).state, "applying");
  assert.equal(summary(initial).canDiscard, false);
  assert.equal(summary(initial).canReply, false);
  assert.equal(initial.codexJobs.length, withoutJob ? 0 : 1);
  if (!withoutJob) assert.equal(initial.codexJobs[0].state, "unknown");
  assert.equal(initial.managementDetail, null, "recovery details are fetched on demand");
  assert.equal(initial.openaiConnection.keyConfigured, false);
  assert.equal(await page.getByRole("button", { name: "核对原会话", exact: true }).count(), 0, "prepared Pending workflow does not offer invalid reconciliation");
  const section = page.getByRole("region", { name: "主副指挥请求记录" });
  const row = section.locator(`[data-management-operation="${operationId}"]`);
  await row.getByRole("button", { name: "查看完整记录", exact: true }).click();
  const panel = section.locator(`[data-management-detail="${operationId}"]`);
  await panel.getByRole("button", { name: "关闭未提交应用…", exact: true }).waitFor();
  await panel.getByText(sessionId, { exact: true }).waitFor();
  const detail = await readDetail();
  const candidate = detail.applicationRecovery;
  assert.ok(candidate, "core proves application has no committed workflow event");
  assert.equal(candidate.targetKind, "assign");
  assert.equal(candidate.taskId, taskId);
  assert.equal(candidate.attemptId, attemptId);
  assert.equal(candidate.sessionId, sessionId);
  assert.equal(candidate.workflowSequence, 2);
  assert.notEqual(candidate.evidenceDigest, detail.evidenceDigest, "application evidence has a distinct scope from the summary chain hash");
  await writeFile(join(dir, "initial-snapshot.json"), JSON.stringify(initial, null, 2));
  await writeFile(join(dir, "initial-detail.json"), JSON.stringify(detail, null, 2));
  const staleSummaryError = await rejectCommand({ type: "abandonManagementApplication", operationId, evidenceDigest: detail.evidenceDigest, reason });
  const staleDigestError = await rejectCommand({ type: "abandonManagementApplication", operationId, evidenceDigest: candidate.evidenceDigest === "0".repeat(64) ? "1".repeat(64) : "0".repeat(64), reason });
  assert.deepEqual(await managementFiles(), beforeManagement);
  assert.deepEqual(await workflowFiles(), beforeWorkflow);

  await panel.getByRole("button", { name: "关闭未提交应用…", exact: true }).click();
  let dialog = page.getByRole("dialog", { name: "确认关闭未提交应用", exact: true });
  await dialog.waitFor();
  await dialog.getByText(sessionId, { exact: true }).waitFor();
  await dialog.getByText(attemptId, { exact: true }).waitFor();
  await dialog.getByText(/请求额度不会退回/).waitFor();
  const submit = dialog.getByRole("button", { name: "确认关闭未提交应用", exact: true });
  assert.equal(await submit.isDisabled(), true);
  await dialog.getByLabel("关闭原因", { exact: true }).fill("   ");
  assert.equal(await submit.isDisabled(), true);
  await dialog.getByLabel("关闭原因", { exact: true }).fill(reason);
  await dialog.screenshot({ path: join(dir, "application-close-review.png") });
  await dialog.getByRole("button", { name: "返回", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  assert.deepEqual(await workflowFiles(), beforeWorkflow);
  assert.deepEqual(await managementFiles(), beforeManagement, "cancelled confirmation publishes nothing");

  await panel.getByRole("button", { name: "关闭未提交应用…", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "确认关闭未提交应用", exact: true });
  await dialog.getByLabel("关闭原因", { exact: true }).fill(reason);
  await dialog.getByRole("button", { name: "确认关闭未提交应用", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  await row.getByText("应用事务已关闭", { exact: true }).waitFor();
  await panel.getByText(reason, { exact: true }).waitFor();
  assert.equal(await panel.getByRole("button", { name: "关闭未提交应用…", exact: true }).count(), 0);
  assert.equal(await page.getByRole("button", { name: "核对原会话", exact: true }).count(), 0);
  if (!withoutJob) await page.getByRole("region", { name: "编程执行记录" }).getByText("本次尝试已关闭", { exact: true }).waitFor();
  const saved = await snapshot();
  assertClosed(saved, initial);
  const savedDetail = await readDetail();
  assert.equal(savedDetail.applicationRecovery, null);
  assert.equal(savedDetail.discardReason, reason);
  assert.equal(savedDetail.state, "abandoned");
  const afterManagement = await managementFiles();
  assert.equal(afterManagement.length, beforeManagement.length + 1, "exactly one management resolution publication");
  for (const original of beforeManagement) assert.deepEqual(afterManagement.find(file => file.name === original.name), original, "prior intent, response and application records are immutable");
  const closed = afterManagement.find(file => file.name === `${operationId}.4.json`);
  assert.ok(closed);
  assert.equal(closed.record.body.previous, detail.evidenceDigest);
  assert.equal(closed.record.hash, savedDetail.evidenceDigest);
  assert.deepEqual(await workflowFiles(), beforeWorkflow, "closure adds no workflow or dispatch event");
  const lateCommand = { type: "abandonManagementApplication", operationId, evidenceDigest: candidate.evidenceDigest, reason };
  const duplicateError = await rejectCommand(lateCommand);
  assert.deepEqual(await managementFiles(), afterManagement, "late duplicate cannot close twice");
  await new Promise(done => setTimeout(done, 1200));
  assertClosed(await snapshot(), initial);
  assert.deepEqual(await workflowFiles(), beforeWorkflow, "resident tick does not apply or dispatch");
  await writeFile(join(dir, "saved-snapshot.json"), JSON.stringify(saved, null, 2));
  await writeFile(join(dir, "saved-detail.json"), JSON.stringify(savedDetail, null, 2));
  await panel.screenshot({ path: join(dir, "application-closed.png") });

  await closeWindow();
  const stopped = stopOwnCore();
  assert.equal(stopped.status, 0, stopped.error?.message ?? stopped.stderr);
  await waitOwnCoreExit(false);
  await launch();
  const reopenedSection = page.getByRole("region", { name: "主副指挥请求记录" });
  const reopenedRow = reopenedSection.locator(`[data-management-operation="${operationId}"]`);
  await reopenedRow.getByText("应用事务已关闭", { exact: true }).waitFor();
  await reopenedRow.getByRole("button", { name: "查看完整记录", exact: true }).click();
  const reopenedPanel = reopenedSection.locator(`[data-management-detail="${operationId}"]`);
  await reopenedPanel.getByText(reason, { exact: true }).waitFor();
  assert.equal(await reopenedPanel.getByRole("button", { name: "关闭未提交应用…", exact: true }).count(), 0);
  const reopened = await snapshot();
  assertClosed(reopened, initial);
  const reopenedDetail = await readDetail();
  assert.deepEqual(reopenedDetail, savedDetail);
  const restartDuplicateError = await rejectCommand(lateCommand);
  assert.deepEqual(await workflowFiles(), beforeWorkflow);
  assert.deepEqual(await managementFiles(), afterManagement, "restart does not append a fake applied receipt");
  await writeFile(join(dir, "reopened-snapshot.json"), JSON.stringify(reopened, null, 2));
  await writeFile(join(dir, "reopened-detail.json"), JSON.stringify(reopenedDetail, null, 2));
  await reopenedPanel.screenshot({ path: join(dir, "application-reopened.png") });
  assert.deepEqual(errors, []);
  await closeWindow();
  result = {
    status: "passed", at: new Date().toISOString(), jobPresent: !withoutJob,
    scope: "production native UI/core with synthetic prepared application and no workflow commit; full core restart included",
    realModelCalls: 0, newWorkflowEvents: 0, newDispatchIntents: 0, newManagementRecords: 1,
    checks: { detailRequiredBeforeReview: true, targetTaskAttemptSessionVisible: true,
      wrongSummaryDigestRejected: true, staleCandidateDigestRejected: true,
      emptyAndBlankReasonRejected: true, cancelledConfirmationWritesNothing: true,
      localClosureOnly: true, requestBudgetPreserved: true, originalEvidencePreserved: true,
      lateAndRestartDuplicatesRejected: true, noAutomaticApplyOrDispatch: true,
      jobProjectionCorrect: true, invalidReconciliationHidden: true, noKeyConfigured: true },
    staleSummaryError, staleDigestError, duplicateError, restartDuplicateError, errors,
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

