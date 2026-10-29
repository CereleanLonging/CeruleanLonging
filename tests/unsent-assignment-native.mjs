// Production native UI/core against a synthetic Assigned ledger with zero dispatch history.
// All mutations target this test's fresh data directory. No credentials or provider calls.
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
const dir = await mkdtemp(join(reports, withoutJob ? "unsent-no-job-native-" : "unsent-assignment-native-"));
const data = join(dir, "data");
const runtime = resolve("target/debug/cerulean-core.exe");
const desktop = resolve("target/debug/cerulean-desktop.exe");
const projectId = "project", taskId = "task", attemptId = "unsent-native";
const reason = "确认该分配尚未派发，关闭原尝试后保持待分配，暂不恢复调度";
const ledger = join(data, "workflows", projectId, "events");
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
      input: JSON.stringify({ id: "unsent-native-ready", protocolVersion: 1, command: { type: "snapshot" } }),
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
async function events() {
  const names = (await readdir(ledger)).filter(name => name.endsWith(".json")).sort();
  return Promise.all(names.map(async name => ({ name, record: JSON.parse(await readFile(join(ledger, name), "utf8")) })));
}
async function managementHashes() {
  const root = join(data, "management-decisions");
  const names = (await readdir(root)).sort();
  return Promise.all(names.map(async name => ({ name, sha256: createHash("sha256").update(await readFile(join(root, name))).digest("hex") })));
}
function workflow(snap) {
  const found = snap.workflows.find(item => item.project.id === projectId);
  assert.ok(found); return found;
}
function job(snap) {
  const found = snap.codexJobs.find(item => item.projectId === projectId && item.attemptId === attemptId);
  assert.ok(found); return found;
}
function assertClosed(snap, initial) {
  const state = workflow(snap).state.tasks[taskId];
  assert.equal(state.state, "pending");
  assert.equal(state.fence, workflow(initial).state.tasks[taskId].fence, "fence is retained");
  assert.equal(state.fence, 1);
  assert.equal(state.attempt ?? null, null);
  assert.equal(state.worker ?? null, null);
  assert.equal(state.verified, false);
  assert.equal(snap.unsentAssignments.length, 0);
  if (withoutJob) assert.equal(snap.codexJobs.length, 0, "missing job is not fabricated during disposition");
  else assert.equal(job(snap).state, "abandoned");
  assert.equal(snap.codexJobs.length, initial.codexJobs.length, "no replacement worker attempt");
  assert.deepEqual(snap.projectRuns, initial.projectRuns, "no automatic run authorization");
  assert.ok(snap.projectRuns.every(run => run.state !== "active"));
  assert.equal(snap.openaiConnection.keyConfigured, false);
  assert.equal(snap.verificationJobs.length, 0);
  assert.deepEqual(workflow(snap).approval, workflow(initial).approval, "user approval is preserved");
  assert.equal(workflow(snap).revisionDigest, workflow(initial).revisionDigest);
  assert.deepEqual(snap.managementDecisions, initial.managementDecisions, "original deputy evidence remains resolved");
  assert.equal(snap.managementHistory.total, initial.managementHistory.total);
}

try {
  const seeded = spawnSync("cargo", ["test", "-p", "cerulean-core", "seed_native_unsent_assignment_fixture", "--locked", "--", "--ignored", "--nocapture"], {
    windowsHide: true, encoding: "utf8", timeout: 120000,
    env: { ...process.env, CERULEAN_UNSENT_NATIVE_DIR: dir, CERULEAN_UNSENT_NO_JOB: withoutJob ? "1" : "0" },
  });
  await writeFile(join(dir, "seed.log"), `${seeded.stdout ?? ""}\n${seeded.stderr ?? ""}`);
  assert.equal(seeded.status, 0, seeded.error?.message ?? seeded.stderr);
  const beforeEvents = await events();
  const originalManagement = await managementHashes();
  assert.equal(beforeEvents.filter(item => item.record.body.event.command.kind === "assign").length, 1);
  assert.equal(beforeEvents.filter(item => item.record.body.event.command.kind === "dispatch-intent").length, 0);

  await launch();
  const section = page.getByRole("region", { name: "编程执行记录" });
  const card = section.locator(`[data-codex-project="${projectId}"][data-codex-attempt="${attemptId}"]`);
  await card.getByText("已分配但尚未派发", { exact: true }).waitFor();
  assert.equal(await card.getByRole("button", { name: "核对原会话", exact: true }).count(), 0, "unsent assignment never offers invalid reconciliation");
  const initial = await snapshot();
  assert.equal(workflow(initial).state.tasks[taskId].state, "assigned");
  if (withoutJob) assert.equal(initial.codexJobs.length, 0, "assigned entry is visible without any job file");
  else assert.equal(job(initial).state, "unknown");
  assert.equal(initial.unsentAssignments.length, 1);
  assert.equal(initial.managementHistory.total, 2);
  assert.deepEqual(initial.managementDecisions.map(item => item.operationId).sort(), ["assign-fixture", "ui-plan-fixture-1"].sort());
  assert.ok(initial.managementDecisions.every(item => item.state === "applied"));
  assert.equal(initial.openaiConnection.keyConfigured, false);
  assert.ok(initial.projectRuns.every(run => run.state !== "active"));
  const review = initial.unsentAssignments[0];
  assert.equal(review.projectId, projectId); assert.equal(review.taskId, taskId); assert.equal(review.attemptId, attemptId);
  await writeFile(join(dir, "initial-snapshot.json"), JSON.stringify(initial, null, 2));
  const staleDigestError = await rejectCommand({ type: "abandonUnsentAttempt", ...review, evidenceDigest: review.evidenceDigest === "0".repeat(64) ? "1".repeat(64) : "0".repeat(64), reason });
  assert.deepEqual(await events(), beforeEvents, "stale digest does not write an event");

  await card.getByRole("button", { name: "放弃未派发尝试…", exact: true }).click();
  let dialog = page.getByRole("dialog", { name: "确认放弃未派发尝试", exact: true });
  await dialog.waitFor();
  assert.equal(await dialog.getByRole("button", { name: "确认放弃未派发尝试", exact: true }).isDisabled(), true);
  await dialog.getByText(/不会自动恢复调度或重新发送任务/).waitFor();
  await dialog.getByLabel("放弃未派发尝试的原因", { exact: true }).fill("   ");
  assert.equal(await dialog.getByRole("button", { name: "确认放弃未派发尝试", exact: true }).isDisabled(), true);
  await dialog.getByLabel("放弃未派发尝试的原因", { exact: true }).fill(reason);
  await dialog.screenshot({ path: join(dir, "unsent-review.png") });
  await dialog.getByRole("button", { name: "返回检查", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  assert.deepEqual(await events(), beforeEvents, "returning from the dialog preserves assignment");
  assert.equal(workflow(await snapshot()).state.tasks[taskId].state, "assigned");

  await card.getByRole("button", { name: "放弃未派发尝试…", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "确认放弃未派发尝试", exact: true });
  await dialog.getByLabel("放弃未派发尝试的原因", { exact: true }).fill(reason);
  await dialog.getByRole("button", { name: "确认放弃未派发尝试", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  if (withoutJob) await card.waitFor({ state: "hidden" });
  else await card.getByText("本次尝试已关闭", { exact: true }).waitFor();
  assert.equal(await card.getByRole("button", { name: "核对原会话", exact: true }).count(), 0);
  assert.equal(await card.getByRole("button", { name: "放弃未派发尝试…", exact: true }).count(), 0);
  const saved = await snapshot();
  assertClosed(saved, initial);
  const afterEvents = await events();
  assert.equal(afterEvents.length, beforeEvents.length + 1, "exactly one new workflow event");
  assert.deepEqual(afterEvents.slice(0, beforeEvents.length), beforeEvents, "original assignment/approval evidence preserved");
  const abandonment = afterEvents.at(-1).record.body.event;
  assert.equal(abandonment.principal, "user");
  assert.equal(abandonment.command.kind, "abandon-unsent");
  assert.equal(abandonment.command.task, taskId);
  assert.equal(abandonment.command.attempt, attemptId);
  assert.equal(abandonment.command.reason, reason);
  assert.equal(afterEvents.filter(item => item.record.body.event.command.kind === "dispatch-intent").length, 0);
  assert.deepEqual(await managementHashes(), originalManagement, "management journal bytes were not rewritten");
  const duplicateError = await rejectCommand({ type: "abandonUnsentAttempt", ...review, reason });
  assert.deepEqual(await events(), afterEvents, "duplicate command adds no event");
  await new Promise(done => setTimeout(done, 1200));
  assertClosed(await snapshot(), initial);
  assert.deepEqual(await events(), afterEvents, "resident ticks do not dispatch or reassign");
  await writeFile(join(dir, "saved-snapshot.json"), JSON.stringify(saved, null, 2));
  if (withoutJob) await page.screenshot({ path: join(dir, "unsent-abandoned.png"), fullPage: true });
  else await section.screenshot({ path: join(dir, "unsent-abandoned.png") });

  await closeWindow();
  const stopped = stopOwnCore();
  assert.equal(stopped.status, 0, stopped.error?.message ?? stopped.stderr);
  await waitOwnCoreExit(false);
  await launch();
  const reopenedSection = page.getByRole("region", { name: "编程执行记录" });
  if (withoutJob) assert.equal(await reopenedSection.count(), 0, "no obsolete recovery entry after disposition");
  else await reopenedSection.getByText("本次尝试已关闭", { exact: true }).waitFor();
  const reopened = await snapshot();
  assertClosed(reopened, initial);
  assert.deepEqual(await events(), afterEvents, "full core restart does not add workflow events");
  assert.deepEqual(await managementHashes(), originalManagement);
  if (withoutJob) await page.screenshot({ path: join(dir, "unsent-reopened.png"), fullPage: true });
  else await reopenedSection.screenshot({ path: join(dir, "unsent-reopened.png") });
  await writeFile(join(dir, "reopened-snapshot.json"), JSON.stringify(reopened, null, 2));
  assert.deepEqual(errors, []);
  await closeWindow();
  result = {
    status: "passed", at: new Date().toISOString(),
    scope: "production native UI/core with synthetic assigned, never-dispatched evidence; full core restart included",
    jobPresent: !withoutJob,
    realModelCalls: 0, newDispatchIntents: 0, newWorkflowEvents: 1,
    checks: {
      assignedUnsentEntry: true, invalidReconciliationHidden: true,
      emptyAndBlankReasonRejected: true, staleDigestRejected: true,
      cancelDialogDoesNotMutate: true, explicitUserDisposition: true,
      pendingWithoutNewAttempt: true, fencePreserved: true,
      jobAbandoned: withoutJob ? undefined : true, missingJobSupported: withoutJob ? true : undefined,
      duplicateDispositionRejected: true, originalAssignmentAndManagementPreserved: true,
      restartDoesNotResend: true, noKeyConfigured: true,
    },
    staleDigestError, duplicateError, errors,
  };
} catch (error) {
  await page?.screenshot({ path: join(dir, "failure.png"), fullPage: true }).catch(() => {});
  await writeFile(join(dir, "failure.json"), JSON.stringify({ at: new Date().toISOString(), error: String(error), errors }, null, 2));
  throw error;
} finally {
  await browser?.close().catch(() => {});
  if (app?.exitCode === null && app?.pid) {
    const exiting = once(app, "exit");
    app.kill();
    await Promise.race([exiting, new Promise((_, reject) => setTimeout(() => reject(Error("owned desktop exit timed out")), 8000).unref())]);
  }
  const cleanup = stopOwnCore();
  await writeFile(join(dir, "cleanup.json"), JSON.stringify({ status: cleanup.status, signal: cleanup.signal, error: cleanup.error?.message, stderr: cleanup.stderr }, null, 2));
  // A still-Assigned fixture may reject graceful stop after a failed assertion.
  // This handle belongs to the exact process we created; never kill by process name.
  await waitOwnCoreExit(cleanup.status !== 0);
  if (result) assert.equal(cleanup.status, 0, cleanup.error?.message ?? cleanup.stderr);
  else if (cleanup.status !== 0) console.error("Owned test core was terminated after graceful stop was refused:", cleanup.error?.message ?? cleanup.stderr);
}
await writeFile(join(dir, "result.json"), JSON.stringify(result, null, 2));
console.log(JSON.stringify({ dir, ...result }, null, 2));
