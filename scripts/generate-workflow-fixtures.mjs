// Explicit baseline regeneration only. Normal tests read the committed fixture.
import { WorkflowCore } from "../../workbench-probes/src/workflow-core.ts";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
const principals = [
  { id: "user", role: "user" },
  { id: "chief", role: "chief", provider: "openai" },
  { id: "deputy", role: "deputy", provider: "openai" },
  { id: "core", role: "core" },
  { id: "worker-a", role: "worker" },
  { id: "worker-b", role: "worker" },
];
const task = (id, deps = [], workspace = `space-${id}`, resources) => ({
  id,
  dependencies: deps,
  workspaceKey: workspace,
  goal: `实现 ${id} 与独立验证`,
  ...(resources === undefined ? {} : { resourceKeys: resources }),
});
const plan = (tasks, version = 1) => ({ id: "plan", version, tasks });
const op = (principal, command) => ({ principal, command });
const propose = (p) => op("chief", { kind: "propose", plan: p });
const approve = { approveCurrent: true };
const assign = (t = "a", worker = "worker-a", attempt = `attempt-${t}`) =>
  op("deputy", { kind: "assign", task: t, worker, attempt });
const cmd = (kind, t = "a", extra = {}) =>
  op("core", { kind, task: t, attempt: `attempt-${t}`, ...extra });
const start = (t = "a") => [
  assign(t),
  cmd("dispatch-intent", t),
  cmd("started", t),
];
const submit = (t = "a") =>
  op("worker-a", {
    kind: "submit",
    task: t,
    attempt: `attempt-${t}`,
    summary: "产物已完成",
  });
const verify = (t = "a", passed = true) =>
  cmd("verify", t, { passed, evidence: ["test-result-sha256-reference"] });
const accept = (t = "a") =>
  op("chief", { kind: "accept", task: t, attempt: `attempt-${t}` });
const reconcile = (outcome, t = "a", evidence = ["session-receipt"]) =>
  cmd("reconcile", t, { outcome, evidence });
const restore = { restore: true };
const definitions = [
  [
    "dependency-and-verified-acceptance",
    [
      propose(plan([task("a"), task("b", ["a"])])),
      assign(),
      approve,
      ...start(),
      submit(),
      assign("b"),
      accept(),
      verify(),
      accept(),
      assign("b", "worker-b"),
    ],
  ],
  [
    "roles-are-not-self-declared",
    [
      op("worker-a", { kind: "propose", plan: plan([task("a")]) }),
      propose(plan([task("a")])),
      op("deputy", { kind: "approve", digest: "invalid" }),
      approve,
      ...start(),
      op("deputy", {
        kind: "advise",
        task: "a",
        suggestion: "查看失败证据，限制重试",
      }),
      op("deputy", {
        kind: "submit",
        task: "a",
        attempt: "attempt-a",
        summary: "pretend",
      }),
      op("chief", {
        kind: "verify",
        task: "a",
        attempt: "attempt-a",
        passed: true,
        evidence: ["pretend"],
      }),
      op("missing", { kind: "snapshot" }),
    ],
  ],
  [
    "revision-invalidates-approval",
    [
      propose(plan([task("a")])),
      { rememberDigest: true },
      approve,
      propose(plan([task("a")], 2)),
      { approveRemembered: true },
      assign(),
      approve,
      assign(),
      propose(plan([task("a")], 3)),
    ],
  ],
  [
    "unknown-holds-workspace",
    [
      propose(plan([task("a", [], "shared"), task("b", [], "shared")])),
      approve,
      assign(),
      cmd("dispatch-intent"),
      restore,
      cmd("dispatch-intent"),
      assign("b"),
      reconcile("not-started", "a", []),
      reconcile("running"),
      submit(),
      verify(),
      accept(),
      assign("b", "worker-b"),
    ],
  ],
  [
    "unknown-holds-shared-port",
    [
      propose(
        plan([
          task("a", [], "space-a", ["port-1420", "db-main"]),
          task("b", [], "space-b", ["port-1420"]),
        ]),
      ),
      approve,
      ...start(),
      restore,
      assign("b"),
      cmd("dispatch-intent"),
      reconcile("submitted"),
      verify(),
      accept(),
      assign("b", "worker-b"),
    ],
  ],
  [
    "cancel-result-unknown-cannot-retry",
    [
      propose(plan([task("a"), task("b", [], "space-a")])),
      approve,
      ...start(),
      cmd("cancel-intent", "a", { reason: "用户停止" }),
      restore,
      cmd("cancel-intent", "a", { reason: "重复停止" }),
      cmd("dispatch-intent"),
      reconcile("not-started"),
      reconcile("running"),
      restore,
      reconcile("cancelled"),
      assign("b", "worker-b"),
    ],
  ],
  [
    "natural-completion-after-cancel",
    [
      propose(plan([task("a")])),
      approve,
      ...start(),
      cmd("cancel-intent", "a", { reason: "stop" }),
      restore,
      reconcile("submitted"),
      verify(),
      accept(),
    ],
  ],
  [
    "wrong-worker-stale-attempt-failed-verification",
    [
      propose(plan([task("a")])),
      approve,
      ...start(),
      op("worker-b", {
        kind: "submit",
        task: "a",
        attempt: "attempt-a",
        summary: "wrong worker",
      }),
      op("worker-a", {
        kind: "submit",
        task: "a",
        attempt: "old",
        summary: "stale",
      }),
      submit(),
      verify("a", false),
      accept(),
      verify(),
      accept(),
    ],
  ],
  [
    "invalid-graph-and-resource-declarations",
    [
      propose(plan([task("a", ["a"])])),
      propose(plan([task("a", ["missing"])])),
      propose(plan([task("a"), task("a")])),
      propose(plan([task("a", [], "space-a", ["port", "port"])])),
      propose(plan([task("a", [], "bad/path")])),
      propose(plan([task("a", [], "space-a", [])])),
      approve,
    ],
  ],
  [
    "attempt-reuse-refused",
    [
      propose(plan([task("a"), task("b")])),
      approve,
      assign(),
      assign("b", "worker-b", "attempt-a"),
      cmd("dispatch-intent"),
      cmd("started"),
      submit(),
      verify(),
      accept(),
      assign("b", "worker-b", "attempt-a"),
    ],
  ],
  [
    "not-started-proof-allows-deliberate-dispatch",
    [
      propose(plan([task("a")])),
      approve,
      assign(),
      cmd("dispatch-intent"),
      restore,
      reconcile("not-started"),
      cmd("dispatch-intent"),
      cmd("started"),
      submit(),
      verify(),
      accept(),
    ],
  ],
  [
    "multi-task-recovery-order",
    [
      propose(plan([task("z"), task("a"), task("3"), task("1")])),
      approve,
      ...start("z"),
      ...start("a"),
      ...start("3"),
      ...start("1"),
      restore,
    ],
  ],
];
const cases = [];
for (const [name, actions] of definitions) {
  let core = new WorkflowCore(principals),
    remembered;
  const steps = [];
  for (const action of actions) {
    if (action.rememberDigest) {
      remembered = core.state().digest;
      continue;
    }
    let operation = action;
    if (action.approveCurrent)
      operation = op("user", { kind: "approve", digest: core.state().digest });
    if (action.approveRemembered)
      operation = op("user", { kind: "approve", digest: remembered });
    // Invalid unknown command with unknown principal is representable as a known command for Rust.
    if (operation.principal === "missing")
      operation = op("missing", {
        kind: "advise",
        task: "a",
        suggestion: "unknown",
      });
    const before = core.state(),
      journal = core.journal();
    let accepted = true,
      error = null;
    try {
      if (action.restore)
        core = WorkflowCore.restore(principals, core.journal(), "core");
      else core.apply(operation.principal, operation.command);
    } catch (e) {
      accepted = false;
      error = e.message;
    }
    if (
      !accepted &&
      (JSON.stringify(before) !== JSON.stringify(core.state()) ||
        JSON.stringify(journal) !== JSON.stringify(core.journal()))
    )
      throw Error("Prototype rejection mutated state");
    steps.push({
      operation: action.restore ? { restore: true } : operation,
      accepted,
      error,
      state: core.state(),
      journal: core.journal(),
    });
  }
  cases.push({ name, principals, steps });
}
const source = "../../workbench-probes/src/workflow-core.ts";
const fixture = {
  schemaVersion: 1,
  source: {
    path: source,
    sha256: createHash("sha256")
      .update(await readFile(new URL(source, import.meta.url)))
      .digest("hex"),
  },
  cases,
};
await mkdir("tests/fixtures", { recursive: true });
await writeFile(
  "tests/fixtures/workflow-parity-v1.json",
  JSON.stringify(fixture, null, 2),
);
console.log(
  JSON.stringify({
    cases: cases.length,
    steps: cases.reduce((n, c) => n + c.steps.length, 0),
    rejections: cases.flatMap((c) => c.steps).filter((s) => !s.accepted).length,
    source: fixture.source,
  }),
);
