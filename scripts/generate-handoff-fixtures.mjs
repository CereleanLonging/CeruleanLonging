// Explicit regeneration against the preserved prototype; never run during normal tests.
import { projectHandoffs } from "../../workbench-probes/src/handoff-projection.ts";
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
const event = (sequence, patch = {}) => ({
  sequence, messageId: `m-${sequence}`, taskId: `task-${sequence}`, attemptId: "attempt", actor: "worker",
  receivedAt: "2026-10-07T10:00:00.000Z", occurredAt: "2026-10-07T10:00:00.000Z",
  status: "running", summary: "实际变更与证据", nextAction: "独立验证", evidence: [], ...patch,
});
const thirty = Array.from({length:30}, (_,i) => event(i+1));
const definitions = [
  ["empty", []],
  ["thirty-and-updates", [...thirty, event(31,{taskId:"task-1"}),event(32),event(33,{taskId:"task-1"})]],
  ["duplicates-unordered", [event(3),event(1),event(2),event(1)]],
  ["duplicate-conflict", [event(1),event(1,{summary:"conflict"})]],
  ["gap", [event(1),event(3)]],
  ["sequence-collision", [event(1),event(1,{messageId:"different"})]],
  ["midnight-late-and-unfinished", [event(1,{taskId:"open",receivedAt:"2026-10-07T15:59:59Z"}),
    event(2,{taskId:"closed",status:"accepted",receivedAt:"2026-10-07T15:59:59Z"}),
    event(3,{taskId:"open",receivedAt:"2026-10-07T16:00:00Z"})]],
  ["byte-rollover", [event(1,{summary:"文".repeat(200)}),event(2,{summary:"文".repeat(200)})], {maxBytes:1500}],
  ["oversize", [event(1,{summary:"文".repeat(1000)})], {maxBytes:1500}],
  ["literal-markdown", [event(1,{summary:"<script>& [link](file:///secret)\r\n*bold* ! ~ \\ ` # _ { } | + . -",evidence:["a&b","[x](y)"]})]],
  ["invalid-path", [event(1,{taskId:"../x"})]],
  ["timezone-missing", [event(1,{receivedAt:"2026-10-07T10:00:00"})]],
  ["clock-regression", [event(1),event(2,{receivedAt:"2026-10-07T09:59:00Z"})]],
  ["submitted-blocked-carry", [event(1,{status:"submitted"}),event(2,{status:"blocked"}),event(3,{status:"cancelled"}),event(4,{receivedAt:"2026-10-08T00:00:00Z"})]],
  ["offset-and-precision", [event(1,{receivedAt:"2026-10-07T12:00:00.1+02:00"}),event(2,{receivedAt:"2026-10-07T10:00:00.12Z"})]],
  ["count-limit", [event(1),event(2),event(3,{taskId:"task-1"}),event(4)],{maxNewTasks:1}],
  ["invalid-limit", [event(1)],{maxNewTasks:0}],
  ["carry-index-budget", [...Array.from({length:12},(_,i)=>event(i+1,{taskId:`long-task-name-${i}`,summary:""})),event(13,{receivedAt:"2026-10-08T00:00:00Z",summary:""})],{maxBytes:950}],
];
const cases = definitions.map(([name,events,options={}])=>{
  try { return {name,events,options,expected:projectHandoffs(events,options)}; }
  catch(error){ return {name,events,options,error:String(error.message)}; }
});
const source = await readFile(new URL("../../workbench-probes/src/handoff-projection.ts", import.meta.url));
await writeFile(new URL("../tests/fixtures/handoff-parity-v1.json", import.meta.url), JSON.stringify({
  schemaVersion:1, source:"workbench-probes/src/handoff-projection.ts", sourceSha256:createHash("sha256").update(source).digest("hex"), cases,
},null,2)+"\n");
console.log(JSON.stringify({cases:cases.length,accepted:cases.filter(c=>c.expected).length,rejected:cases.filter(c=>c.error).length}));
