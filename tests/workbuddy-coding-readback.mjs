// Read-only recovery of the single approved coding attempt. Never sends or focuses.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const source=resolve('reports/workbuddy-coding-send-B3MM5u');
const review=JSON.parse(await readFile(join(source,'task-0/task-review.json'),'utf8'));
const report=await mkdtemp(resolve('reports/workbuddy-coding-readback-'));
const harness=join(report,'Readback.cs');
await writeFile(harness,`using System;using System.Linq;using System.Windows.Automation;
public partial class DesktopObserver {
 static object CodingCapture(TaskReview review) {
  ReviewTask(review,true);var view=ReadView(review.observation.window,true);Continuity(view,review);
  if(view.observation.modelLabel!=review.observation.modelLabel)throw new InvalidOperationException("trial_view_changed");
  string panel=view.observation.documentRuntimeId+".4.0.1.0.0.0.1",list=panel+".1.0.0.0.0.0.0";
  if(view.nodes.Count(n=>Runtime(n)==panel+".0.0.0"&&n.Cached.Name=="工作台接入验证")!=1)throw new InvalidOperationException("trial_view_changed");
  var turns=view.nodes.Where(n=>Runtime(n).StartsWith(list+".")&&!Runtime(n).Substring(list.Length+1).Contains('.')).ToArray();
  if(turns.Length<4)throw new InvalidOperationException("trial_history_unavailable");
  string user=Runtime(turns[turns.Length-2]),reply=Runtime(turns[turns.Length-1]);
  var texts=view.nodes.Where(n=>Runtime(n).StartsWith(user+".0.0.0.")&&n.Cached.ControlType==ControlType.Text&&!String.IsNullOrWhiteSpace(n.Cached.Name)).ToArray();
  if(texts.Length!=1||texts[0].Cached.Name!=review.prompt||view.nodes.Count(n=>Runtime(n)==reply+".0.1"&&n.Cached.Name=="WorkBuddy")!=1)throw new InvalidOperationException("trial_history_changed");
  var rounds=TaskRounds(view);var retained=Baseline(rounds.Take(rounds.Length-1).ToArray());
  bool history=retained.Length>=2&&retained.Length<=review.baseline.Length&&retained.SequenceEqual(review.baseline.Skip(review.baseline.Length-retained.Length));
  return new {observation=view.observation,replyRoot=reply,promptMatched=true,historyMatched=history,terminalReply=rounds.Last().reply,completed=rounds.Last().completed&&!rounds.Last().cancelled,nodes=view.nodes.Where(n=>Runtime(n)==reply||Runtime(n).StartsWith(reply+".")).Select(n=>new {id=Runtime(n),type=n.Cached.ControlType.ProgrammaticName,name=n.Cached.Name??""}).ToArray()};
 }
}`);
const original=await readFile('crates/core/native/DesktopObserver.cs','utf8');
await writeFile(join(report,'Observer.cs'),original.replace('else if(request.op=="trialprepare")','else if(request.op=="codingcapture")result=CodingCapture(request.taskReview);\n            else if(request.op=="trialprepare")'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319'),exe=join(report,'Observer.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'Observer.cs'),harness,...['DesktopTrial','DesktopStop'].map(n=>resolve('crates/core/native/'+n+'.cs'))],{encoding:'utf8',windowsHide:true,timeout:30000});
assert.equal(build.status,0,build.stdout+build.stderr);
const results={};
for(const op of ['codingcapture','taskread']){
 const run=spawnSync(exe,[JSON.stringify({version:1,op,taskReview:review})],{encoding:'utf8',windowsHide:true,timeout:45000});
 assert.equal(run.error,undefined,run.error?.message);
 results[op]=JSON.parse(run.stdout.replace(/^\uFEFF/,''));
 await writeFile(join(report,op+'.json'),JSON.stringify(results[op],null,2)+'\n');
}
const planBytes=await readFile('tests/fixtures/workbuddy-coding-plan.json'),plan=JSON.parse(planBytes);
const approved=JSON.parse(await readFile(join(source,'approved-scope.json'),'utf8'));
const hash=b=>createHash('sha256').update(b).digest('hex');assert.equal(hash(planBytes),approved.planHash);
assert.equal(review.prompt,plan.messages[0]);
const requestedMarker=plan.messages[0].match(/CL_TASK_[0-9a-f]+/g);assert.equal(requestedMarker.length,1);
const capture=results.codingcapture.result;
const originalOutcome=JSON.parse(await readFile(join(source,'result.json'),'utf8'));
const artifact=await readFile(join(plan.projectDirectory,plan.artifact));
const code=capture?.nodes.filter(n=>n.type==='ControlType.Text'&&n.id.startsWith(capture.replyRoot+'.0.7.0.0.')).map(n=>n.name).join('');
const result={readOnly:true,sendAttempts:0,originalAttempt:source,capturePassed:results.codingcapture.ok,replyProven:results.taskread.ok,error:results.taskread.error??null,historyMatched:capture?.historyMatched??false,completed:capture?.completed??false,requestedMarkerMatches:capture?.terminalReply===requestedMarker[0],reviewMarkerMatches:review.marker===requestedMarker[0],requestedMarkerHexLength:requestedMarker[0].length-8,reviewMarkerHexLength:review.marker.length-8,artifactStillMatches:hash(artifact)===originalOutcome.artifact.sha256,displayedCodeMatchesArtifact:code===artifact.toString('utf8').replace(/\r\n/g,'\n'),managementTested:false,durableWorkflowTested:false};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({report,...result}));
