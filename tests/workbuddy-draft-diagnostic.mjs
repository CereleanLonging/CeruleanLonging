// No focus, input, Invoke, or raw draft output. Compare the unresolved approved attempt only.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-draft-diagnostic-'));
const review=JSON.parse(await readFile('reports/workbuddy-send-live-7CgjNy/task-0/task-review.json','utf8'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
const source=await readFile('crates/core/native/DesktopObserver.cs','utf8');
const needle='else if(request.op=="trialprepare")';assert.equal(source.split(needle).length,2);
await writeFile(join(report,'Observer.cs'),source.replace(needle,'else if(request.op=="draftdiagnostic")result=DraftDiagnostic(request.taskReview);\n            '+needle));
await writeFile(join(report,'Draft.cs'),`using System;using System.Linq;
public partial class DesktopObserver {
 static object DraftDiagnostic(TaskReview r){
  ReviewTask(r,true);var view=ReadView(r.observation.window,false);Continuity(view,r);
  if(view.observation.modelLabel!=r.observation.modelLabel)throw new InvalidOperationException("trial_view_changed");
  var baseline=Baseline(TaskRounds(view));
  return new{draftMatchesApproved=view.draft==r.prompt,draftLength=view.draft==null?-1:view.draft.Length,draftEmpty=view.observation.draftEmpty,busy=view.observation.busy,historyRounds=baseline.Length,originalBaselineUnchanged=baseline.SequenceEqual(r.baseline),promptInHistory=TaskRounds(view).Any(round=>round.user.Contains(r.prompt)),readOnly=true};
 }
}`);
const exe=join(report,'Draft.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'Observer.cs'),join(report,'Draft.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(build.status,0,build.stdout+build.stderr);
const run=spawnSync(exe,[JSON.stringify({version:1,op:'draftdiagnostic',taskReview:review})],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(run.error,undefined);
const response=JSON.parse(run.stdout.replace(/^\uFEFF/,''));await writeFile(join(report,'result.json'),JSON.stringify({exitCode:run.status,response},null,2)+'\n');console.log(JSON.stringify({report,...response}));assert.equal(run.status,0);
