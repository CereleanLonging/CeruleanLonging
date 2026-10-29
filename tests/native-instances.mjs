// Native process isolation regression: real WebView2 and core IPC; zero model calls.
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer, createConnection } from 'node:net';
import { mkdir, mkdtemp, readdir, readFile, writeFile, symlink, stat } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import assert from 'node:assert/strict';

await mkdir(resolve('reports'), { recursive: true });
const dir = await mkdtemp(resolve('reports/native-instances-'));
const data = join(dir, 'data');
const cacheRoot = join(process.env.LOCALAPPDATA, 'local.ceruleanlonging.workbench', 'webview-instances-v1');
const releaseData = join(process.env.APPDATA, 'local.ceruleanlonging.workbench', 'workspace');
const debug = resolve('target/debug/cerulean-desktop.exe');
const release = resolve('target/release/CeruleanLonging.exe');
const runtime = resolve('target/debug/cerulean-core.exe');
const releaseRuntime = resolve('target/release/cerulean-runtime.exe');
const instances = [];
const errors = [];
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const hadReleaseResident = await residentExists(releaseRuntime, releaseData);

async function residentExists(binary, directory) {
  const result = spawnSync(binary, ['--endpoint', '--data-dir', directory], {
    windowsHide: true, encoding: 'utf8', timeout: 10000,
  });
  assert.equal(result.status, 0, result.stderr);
  return new Promise((done) => {
    const socket = createConnection(result.stdout.trim());
    socket.once('connect', () => { socket.destroy(); done(true); });
    socket.once('error', (error) => { socket.destroy(); done(error.code !== 'ENOENT'); });
  });
}

async function launch(label, binary = debug, overrideRoot) {
  const listener = createServer();
  listener.listen(0, '127.0.0.1');
  await once(listener, 'listening');
  const port = listener.address().port;
  await new Promise((done) => listener.close(done));
  const env = {
    ...process.env,
    CERULEAN_TEST_DATA_DIR: data,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
  };
  // Exercise debug + branded release with the same production cache root.
  delete env.CERULEAN_TEST_WEBVIEW_ROOT;
  delete env.WEBVIEW2_USER_DATA_FOLDER;
  if (overrideRoot) env.CERULEAN_TEST_WEBVIEW_ROOT = overrideRoot;
  const app = spawn(binary, [], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env });
  const instance = { label, app, port, stderr: '', binary, cacheRoot: overrideRoot ?? cacheRoot };
  instances.push(instance);
  app.stderr.on('data', (chunk) => {
    instance.stderr = (instance.stderr + chunk).slice(-65536);
  });
  app.stdout.resume();
  for (let attempt = 0; attempt < 80; attempt++) {
    assert.equal(app.exitCode, null, `${label} exited: ${instance.stderr}`);
    try {
      instance.browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1000 });
      break;
    } catch { await sleep(250); }
  }
  assert.ok(instance.browser, `${label} CDP connects`);
  for (let attempt = 0; attempt < 50; attempt++) {
    instance.page = instance.browser.contexts().flatMap((context) => context.pages())
      .find((page) => page.url().includes('tauri.localhost'));
    if (instance.page) break;
    await sleep(200);
  }
  assert.ok(instance.page, `${label} native page`);
  instance.page.on('pageerror', (error) => errors.push({ label, error: error.message }));
  await instance.page.getByText('本地核心已连接', { exact: true }).waitFor({ timeout: 20000 });
  const prefix = `instance-${app.pid.toString(16).padStart(8, '0')}-`;
  const candidates = (await readdir(instance.cacheRoot)).filter((name) => name.startsWith(prefix));
  assert.equal(candidates.length, 1, `${label} owns exactly one fresh profile`);
  instance.profile = join(instance.cacheRoot, candidates[0]);
  return instance;
}

function snapshot(instance) {
  return instance.page.evaluate(() => window.__TAURI_INTERNALS__.invoke('core_request', { command: { type: 'snapshot' } }));
}

async function close(instance) {
  if (!instance || instance.closed) return;
  if (instance.app.exitCode === null) {
    const exiting = once(instance.app, 'exit');
    await instance.page?.evaluate(() => window.__TAURI_INTERNALS__.invoke('plugin:window|close', { label: 'main' })).catch(() => {});
    await Promise.race([
      exiting,
      new Promise((_, reject) => setTimeout(() => reject(Error(`${instance.label} did not close normally`)), 10000).unref()),
    ]);
  }
  await instance.browser?.close();
  instance.closed = true;
}

function coreSnapshot() {
  const result = spawnSync(runtime, ['--request', '--data-dir', data], {
    windowsHide: true, encoding: 'utf8', timeout: 20000,
    input: JSON.stringify({ id: 'native-instances-surviving-core', protocolVersion: 1, command: { type: 'snapshot' } }),
  });
  assert.equal(result.status, 0, result.stderr);
  const response = JSON.parse(result.stdout).response;
  assert.equal(response.error, null);
  return response.result;
}

try {
  const [first, second, branded] = await Promise.all([
    launch('debug-first'), launch('debug-second'), launch('release', release),
  ]);
  assert.equal(new Set([first.port, second.port, branded.port]).size, 3);
  assert.equal(new Set([first.profile, second.profile, branded.profile]).size, 3);
  const releaseBefore = await snapshot(branded);
  const before = await snapshot(first);
  assert.equal(before.plans.length, 0);
  assert.equal((await snapshot(second)).dataDirectory, before.dataDirectory);
  await first.page.getByRole('button', { name: '新建计划' }).click();
  await first.page.getByLabel('计划名称').fill('并行窗口共享核心验收');
  await first.page.getByLabel('具体任务').fill('并行窗口共享同一计划\n关闭与重开保留审批');
  await first.page.getByRole('button', { name: '保存计划', exact: true }).click();
  await first.page.getByRole('button', { name: '审阅并批准', exact: true }).waitFor();
  // Pull current state via the native IPC: the other window owns no copy of data.
  const shared = await snapshot(second);
  assert.equal(shared.plans.length, 1);
  assert.equal(shared.plans[0].approved, false);
  await second.page.reload();
  await second.page.getByText('本地核心已连接', { exact: true }).waitFor({ timeout: 20000 });
  await second.page.getByRole('button', { name: '需求与计划' }).click();
  await second.page.getByRole('button', { name: '审阅并批准', exact: true }).click();
  await second.page.getByRole('button', { name: '确认批准', exact: true }).click();
  await second.page.getByText('批准已写入本地账本。', { exact: true }).waitFor();
  assert.equal((await snapshot(first)).plans[0].approved, true);
  await second.page.screenshot({ path: join(dir, 'parallel-approved.png'), fullPage: true });
  await close(first);
  assert.equal((await snapshot(second)).plans[0].approved, true);
  const reopened = await launch('debug-reopened');
  assert.notEqual(reopened.profile, first.profile);
  assert.equal((await snapshot(reopened)).plans[0].digest, shared.plans[0].digest);
  assert.equal((await snapshot(reopened)).plans[0].approved, true);
  assert.equal((await stat(second.profile)).isDirectory(), true, 'active sibling cache is preserved');
  assert.equal((await stat(branded.profile)).isDirectory(), true, 'active release cache is preserved');
  assert.deepEqual((await snapshot(branded)).plans, releaseBefore.plans, 'release remains read-only');
  await reopened.page.screenshot({ path: join(dir, 'reopened.png'), fullPage: true });
  await close(second);
  await close(reopened);
  assert.equal(coreSnapshot().plans[0].approved, true, 'resident core survives all its windows');
  await close(branded);

  // A retired, correctly marked cache with a junction must remain untouched.
  // Every fixture is beneath this report directory; no shell deletion is used.
  const guardRoot = join(dir, 'guard-profiles');
  const outside = join(dir, 'outside-profile');
  await mkdir(outside);
  await writeFile(join(outside, 'sentinel.txt'), 'preserve this unrelated file');
  const retired = await launch('guard-owner', debug, guardRoot);
  await close(retired);
  await symlink(outside, join(retired.profile, 'do-not-follow'), 'junction');
  await mkdir(join(guardRoot, 'unrecognized-directory'));
  await writeFile(join(guardRoot, 'unrecognized-directory', 'sentinel.txt'), 'foreign cache');
  const guard = await launch('guard-reopen', debug, guardRoot);
  assert.equal((await stat(retired.profile)).isDirectory(), true);
  assert.equal(await readFile(join(outside, 'sentinel.txt'), 'utf8'), 'preserve this unrelated file');
  assert.equal(await readFile(join(guardRoot, 'unrecognized-directory', 'sentinel.txt'), 'utf8'), 'foreign cache');
  await close(guard);
  assert.deepEqual(errors, []);
  for (const instance of instances) {
    assert.ok(!/panicked|0x8007139F|failed to create webview/i.test(instance.stderr), instance.stderr);
  }
  const result = {
    at: new Date().toISOString(), status: 'passed', root: dir, modelCalls: 0,
    checks: {
      simultaneousDebugAndRelease: true, distinctPortsAndProfiles: true,
      sharedCorePlanAndApproval: true, siblingSurvivesNormalClose: true,
      freshProfileReopenPreservesApproval: true, residentSurvivesAllWindows: true,
      activeProfilesPreserved: true, retiredReparsePointRefused: true,
      foreignDirectoryPreserved: true, releaseBusinessDataUnchanged: true,
      noWebViewEnvironmentConflict: true,
    },
    originalFailurePreserved: resolve('reports/desktop-smoke-WNq5RT/failure.json'),
    instances: instances.map(({ label, app, port, profile, binary }) => ({ label, pid: app.pid, port, profile, binary })),
    preexistingReleaseResidentPreserved: hadReleaseResident,
  };
  await writeFile(join(dir, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
} catch (error) {
  for (const instance of instances) {
    await instance.page?.screenshot({ path: join(dir, `${instance.label}-failure.png`), fullPage: true }).catch(() => {});
  }
  await writeFile(join(dir, 'failure.json'), JSON.stringify({ error: String(error), errors,
    instances: instances.map(({ label, app, stderr, profile }) => ({ label, pid: app.pid, exitCode: app.exitCode, stderr, profile })),
  }, null, 2));
  throw error;
} finally {
  for (const instance of instances) {
    await close(instance).catch(() => {});
    await instance.browser?.close().catch(() => {});
    if (instance.app.exitCode === null) instance.app.kill();
  }
  const stop = (binary, directory) => spawnSync(binary, ['--stop', '--data-dir', directory], { windowsHide: true, encoding: 'utf8', timeout: 20000 });
  const stopped = stop(runtime, data);
  if (stopped.status !== 0) console.error('Isolated core cleanup:', stopped.stderr);
  if (!hadReleaseResident) {
    const stoppedRelease = stop(releaseRuntime, releaseData);
    if (stoppedRelease.status !== 0) console.error('New release core cleanup:', stoppedRelease.stderr);
  }
}
