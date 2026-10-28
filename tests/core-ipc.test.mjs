import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, writeFile, mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";

async function directory() {
  await mkdir("reports", { recursive: true });
  return mkdtemp(resolve("reports/ipc-"));
}
function core(dir) {
  const child = spawn(
    resolve("target/debug/cerulean-core.exe"),
    ["--stdio", "--data-dir", dir],
    { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] },
  );
  const lines = createInterface({ input: child.stdout })[
    Symbol.asyncIterator
  ]();
  let seq = 0;
  return {
    child,
    async call(command, protocolVersion = 1) {
      child.stdin.write(
        JSON.stringify({ id: String(++seq), protocolVersion, command }) + "\n",
      );
      const line = await lines.next();
      assert.equal(line.done, false);
      const r = JSON.parse(line.value);
      assert.equal(r.id, String(seq));
      return r;
    },
    async stop() {
      const exited = once(child, "exit");
      child.stdin.end();
      assert.equal((await exited)[0], 0);
    },
  };
}
test(
  "real process persists approvals, rejects stale digests and replays after restart",
  { timeout: 20000 },
  async () => {
    const dir = await directory();
    const a = core(dir);
    try {
      assert.equal(
        (await a.call({ type: "snapshot" })).result.product,
        "CeruleanLonging",
      );
      assert.equal(
        (
          await a.call(
            {
              type: "createPlan",
              title: "integration",
              tasks: ["write", "verify"],
            },
            99,
          )
        ).error.code,
        "protocolMismatch",
      );
      const plan = (
        await a.call({
          type: "createPlan",
          title: "integration",
          tasks: ["write", "verify"],
        })
      ).result.plans[0];
      assert.ok(
        (await a.call({ type: "approvePlan", id: plan.id, digest: "wrong" }))
          .error,
      );
      assert.equal(
        (
          await a.call({
            type: "approvePlan",
            id: plan.id,
            digest: plan.digest,
          })
        ).result.events.length,
        2,
      );
      assert.equal(
        (
          await a.call({
            type: "approvePlan",
            id: plan.id,
            digest: plan.digest,
          })
        ).result.events.length,
        2,
      );
      await a.stop();
      const b = core(dir);
      try {
        const r = await b.call({ type: "snapshot" });
        assert.equal(r.result.plans[0].approved, true);
        assert.equal(r.result.events.length, 2);
        assert.ok(r.result.adapters.every((x) => x.status === (["qoder","workbuddy"].includes(x.id)?"prototype":"partial")));
        assert.ok(r.result.adapters.every((x) => x.capabilities.length === 10 &&
          x.capabilities.every((c) => c.state === "unverified" && c.testedVersion === null && c.evidenceSha256 === null)));
        await b.stop();
      } finally {
        b.child.kill();
      }
      const event = JSON.parse(
        await readFile(join(dir, "events/00000001.json"), "utf8"),
      );
      event.body.change.plan.title = "tampered";
      await writeFile(join(dir, "events/00000001.json"), JSON.stringify(event));
      const broken = core(dir);
      assert.notEqual((await once(broken.child, "exit"))[0], 0);
    } finally {
      a.child.kill();
    }
  },
);
test(
  "oversized frames fail closed without creating a plan",
  { timeout: 10000 },
  async () => {
    const dir = await directory();
    const a = core(dir);
    const exited = once(a.child, "exit");
    a.child.stdin.on("error", () => {});
    a.child.stdin.end("x".repeat(65537) + "\n");
    assert.notEqual((await exited)[0], 0);
    const b = core(dir);
    try {
      assert.equal((await b.call({ type: "snapshot" })).result.plans.length, 0);
      await b.stop();
    } finally {
      b.child.kill();
    }
  },
);
