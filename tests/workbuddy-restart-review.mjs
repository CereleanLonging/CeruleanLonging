// Read-only comparison after a restart. An old binding is never reused as a new identity.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-restart-review-'));
const prior=JSON.parse(await readFile('reports/workbuddy-send-live-7CgjNy/task-0/task-review.json','utf8'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
const original=await readFile('crates/core/native/DesktopObserver.cs','utf8'),needle='else if(request.op=="trialprepare")';assert.equal(original.split(needle).length,2);
await writeFile(join(report,'Observer.cs'),original.replace(needle,'else if(request.op=="restartinspect")result=InspectRestart(request.window,request.taskReview);\n            '+needle));
await writeFile(join(report,'Restart.cs'),`using System;using System.Linq;using System.Windows.Automation;
public partial class DesktopObserver {
 static object InspectRestart(Window window,TaskReview prior){
  ReviewTask(prior,true);
  if(window==null||window.app!="workbuddy"||window.version!="5.7.7.0")throw new InvalidOperationException("trial_app_not_enabled");
  var view=ReadView(window,false);string header=view.observation.documentRuntimeId+".4.0.1.0.0.0.1.0.0.0";
  if(view.observation.modelLabel!="Hy3"||view.nodes.Count(n=>Runtime(n)==header&&n.Cached.ControlType==ControlType.Text&&n.Cached.Name=="工作台接入验证"&&!n.Cached.IsOffscreen)!=1)throw new InvalidOperationException("trial_view_changed");
  var rounds=TaskRounds(view);var baseline=Baseline(rounds);
  return new{observation=view.observation,baseline=baseline,historyMatchesOriginal=baseline.SequenceEqual(prior.baseline),draftMatchesOriginal=view.draft==prior.prompt,draftEmpty=view.observation.draftEmpty,promptInHistory=rounds.Any(r=>r.user.Contains(prior.prompt)),sameProcess=Same(window,prior.observation.window),readOnly=true};
 }
}`);
const exe=join(report,'Restart.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'Observer.cs'),join(report,'Restart.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(build.status,0,build.stdout+build.stderr);
async function run(name,body){const r=spawnSync(exe,[JSON.stringify({version:1,...body})],{encoding:'utf8',windowsHide:true,timeout:30000});if(r.error)throw Error(name+': '+r.error.code);const response=JSON.parse(r.stdout.replace(/^\uFEFF/,''));await writeFile(join(report,name+'.json'),JSON.stringify(response,null,2)+'\n');if(r.status!==0||!response.ok)throw Error(name+': '+response.error);return response.result;}
let result;
try {
 const discovered=await run('discover',{op:'discover'});const windows=discovered.windows.filter(w=>w.app==='workbuddy'&&w.exe.toLowerCase()==='d:\\workbuddy\\workbuddy.exe');assert.equal(windows.length,1);
 const first=await run('first',{op:'restartinspect',window:windows[0],taskReview:prior});
 const second=await run('second',{op:'restartinspect',window:windows[0],taskReview:prior});
 const a=structuredClone(first),b=structuredClone(second);delete a.observation.capturedAt;delete b.observation.capturedAt;assert.deepEqual(a,b);
 result={status:'observed',readOnly:true,historyMatchesOriginal:second.historyMatchesOriginal,draftMatchesOriginal:second.draftMatchesOriginal,draftEmpty:second.draftEmpty,promptInHistory:second.promptInHistory,sameProcess:second.sameProcess,historyRounds:second.baseline.length,busy:second.observation.busy,repeatedMatches:true,keyboardInput:0,clicks:0};
}catch(e){result={status:'failed',readOnly:true,error:e.message,keyboardInput:0,clicks:0};throw e;}
finally{await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,...result}));}
