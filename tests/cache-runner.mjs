import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const manager = resolve("target/cache-tools/cerulean-cache.exe");
const run = await mkdtemp(resolve("reports/cache-runner-794-"));
const root = join(run, "source");
await mkdir(root);
await writeFile(join(root, "package.json"), '{"name":"cerulean-longing"}');
await writeFile(join(root, "Cargo.toml"), "[workspace]\n");
await mkdir(join(root, "target/release"), { recursive: true });
await writeFile(join(root, "target/release/program.exe"), "retained output");
const query = action => {
  const result = spawnSync(manager, [action, root], { encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
};
const cases = [];
const failed = spawnSync(manager, ["run", root, process.execPath, "-e", `require('fs').writeFileSync(require('path').join(process.env.CERULEAN_CACHE_SESSION,'failed.tmp'),'failure data');process.exit(7)`], { encoding: "utf8", windowsHide: true });
assert.equal(failed.status, 7, failed.stderr);
assert.equal(query("status").totalBytes, 0);
assert.equal(query("status").cumulativeCleanedBytes, 12);
cases.push("failed child retains its exit code and automatically clears its cache");

const childScript = `const fs=require('fs'),path=require('path');fs.writeFileSync(path.join(process.env.CERULEAN_CACHE_SESSION,'active.tmp'),'active data');console.log('ready');setTimeout(()=>{},2500)`;
const task = spawn(manager, ["run", root, process.execPath, "-e", childScript], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
const exited = once(task, "exit");
await once(task.stdout, "data");
const active = query("scan");
assert.equal(active.totalBytes, 11);
assert.equal(active.availableBytes, 0);
assert(active.entries.some(entry => entry.active));
assert.equal(query("clean").totalBytes, 11);
assert.equal((await exited)[0], 0);
assert.equal(query("status").totalBytes, 0);
cases.push("a second process cannot clean an active job and cleanup runs after completion");

const orphan = spawn(manager, ["run", root, process.execPath, "-e", `require('fs').writeFileSync(require('path').join(process.env.CERULEAN_CACHE_SESSION,'orphan.tmp'),'orphan');console.log('ready');setInterval(()=>{},1000)`], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
await once(orphan.stdout, "data");
const stopped = once(orphan, "exit");
orphan.kill();
await stopped;
await new Promise(resolve => setTimeout(resolve, 500));
assert.equal(query("status").availableBytes, 6);
assert.equal(query("clean").totalBytes, 0);
cases.push("interrupted runner releases its lease and leaves a reclaimable job");

assert.equal(await readFile(join(root, "target/release/program.exe"), "utf8"), "retained output");
const jobs = await readdir(join(root, ".temp/build-cache/jobs"));
assert.equal(jobs.length, 3);
for (const job of jobs) assert((await readdir(join(root, ".temp/build-cache/jobs", job))).includes("scratch"));
assert.equal(query("status").cumulativeCleanedBytes, 29);
assert.equal(query("status").pendingOperations, 0);
cases.push("empty job directories and final output remain, persisted totals do not double count");
const result = { status: "passed", cases, run, providerCalls: 0, terminatedOwnedTestRunner: true };
await writeFile(join(run, "result.json"), JSON.stringify(result, null, 2));
console.log(JSON.stringify(result));
