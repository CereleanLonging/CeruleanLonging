// Real bound child protocol with synthetic evidence; no project execution or network.
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {createHash} from "node:crypto";
import {mkdtempSync, writeFileSync} from "node:fs";
import {join, resolve} from "node:path";
const executable=process.env.CERULEAN_VERIFIER_TEST_EXE||process.argv[2];
assert(executable,"Explicit verifier executable required");
const dir=mkdtempSync(resolve("reports/independent-checks-native-"));
// Rust's BTreeMap-free struct field order is explicit in these protocol fixtures.
const hash=value=>createHash("sha256").update(JSON.stringify(value)).digest("hex");
const call=operation=>{
  const request={protocol:"cerulean-independent-verifier-v1",operation};
  const run=spawnSync(executable,["--stdio-once"],{input:JSON.stringify(request),encoding:"utf8",windowsHide:true,timeout:15000,maxBuffer:2*1024*1024});
  assert.equal(run.status,0,run.stderr);assert.equal(run.stderr,"");
  const response=JSON.parse(run.stdout);assert.notEqual(response.processId,process.pid);
  assert.equal(response.protocol,request.protocol);
  return response.result;
};
const facts=[{id:"check",commandDigest:"a".repeat(64),toolDigest:"b".repeat(64),passed:true,reason:"approved synthetic check passed"}];
const record={schemaVersion:1,projectId:"project",revision:"c".repeat(64),taskId:"task",attemptId:"attempt",operationId:"check-1",sourceDigest:"d".repeat(64),manifestDigest:"e".repeat(64),reportDigest:"f".repeat(64),observedAt:"2026-11-05T01:00:00Z",checks:facts};
const archive={schemaVersion:1,projectId:record.projectId,revision:record.revision,finalTask:record.taskId,finalSource:record.sourceDigest,records:[record]};
const policy={version:1,instructionShare:5000,checkShare:5000,minimumSeat:8000,minimumOverall:9000,calibration:"unknown"};
const material={protocol:"cerulean-blind-review-v1",requirement:"Verify the synthetic task",criteria:Array.from({length:10},(_,i)=>`criterion ${i+1}`),fragments:[{id:"f1",file:"file1",part:1,content:"return a + b;"}],objectiveChecks:1};
const seats=["chief","deputy","worker"].map(role=>({id:role,role,target:{kind:"openAi"},model:"fixture",allowedModels:["fixture"]}));
const votes=()=>seats.map(s=>{const ballot={assessments:material.criteria.map((_,i)=>({criterion:i+1,aligned:true,citations:["f1"]})),vote:"pass",citations:["f1"],reason:"synthetic approved criterion evidence"};return {seat:s.id,model:s.model,ballot,toolAudit:{status:"clean",source:"api-response",request:"request-1",evidenceDigest:"a".repeat(64),tools:[],reason:"complete synthetic trace"},originalBallot:structuredClone(ballot)};});
const cases=[];
const evidence = call({kind:"attachCheckEvidence", material, archive}).Ok.material;
assert.deepEqual(evidence.independentCheckEvidence, {
  taskDeliveries:1, checksTotal:1, checksPassed:1, sourceFiles:1, assembledDelivery:false,
});
assert(!JSON.stringify(evidence).includes("approved synthetic check passed"));
assert(call({kind:"attachCheckEvidence", material:evidence, archive}).Err);
const failedArchive = structuredClone(archive);
failedArchive.records[0].checks[0].passed = false;
assert(call({kind:"attachCheckEvidence", material, archive:failedArchive}).Err);
cases.push("Independent process attaches anonymous verified facts and refuses failed or duplicate evidence");
const archived=call({kind:"archiveChecks",archive});assert.equal(archived.Ok.digest,hash(archive));assert.equal(archived.Ok.basisPoints,10000);cases.push("Independent child validates exact report digest and all check facts");
const tally=ballots=>call({kind:"weightedTally",policy,archive,material,seats,ballots});
let ballots=votes();assert.equal(tally(ballots).Ok.result.passed,true);cases.push("Complete independent ballots and checks meet both gates");
for(let i=0;i<2;i++){ballots[i].ballot.assessments.length=6;ballots[i].originalBallot=structuredClone(ballots[i].ballot);}
const aggregate=tally(ballots).Ok.result;assert.equal(aggregate.passed,false);assert.equal(aggregate.overallBasisPoints,8666);assert(aggregate.seats.every(s=>s.passed));cases.push("Aggregate gate rejects 80/80/100 despite every seat meeting its gate");
ballots=votes();ballots[0].ballot.assessments=[];ballots[0].originalBallot=structuredClone(ballots[0].ballot);assert.equal(tally(ballots).Ok.result.seats[0].totalBasisPoints,5000);cases.push("Omitted instruction evidence earns zero instruction credit");
ballots=votes();ballots[0].toolAudit.status="toolCallObserved";ballots[0].toolAudit.tools=["web_search"];
const audited=call({kind:"auditBallot",material,seat:seats[0],model:"fixture",ballot:ballots[0].originalBallot,audit:ballots[0].toolAudit});ballots[0].ballot=audited.Ok.ballot;
const veto=tally(ballots).Ok.result;assert.equal(veto.seats[0].totalBasisPoints,0);assert.equal(veto.seats.length,3);assert.equal(veto.passed,false);cases.push("Tool request creates zero effective weight and retains the seat");
ballots[0].ballot.vote="pass";assert(tally(ballots).Err);cases.push("Forged pass disagrees with the independently audited ballot");
const changed=structuredClone(archive);changed.records[0].revision="a".repeat(64);assert(call({kind:"archiveChecks",archive:changed}).Err);cases.push("Cross-revision report is rejected before archiving");
writeFileSync(join(dir,"result.json"),JSON.stringify({passed:true,cases,executable,realProviderCalls:0,scope:"real offline independent executable; synthetic report and model evidence"},null,2));
console.log(JSON.stringify({passed:true,cases:cases.length,report:dir}));
