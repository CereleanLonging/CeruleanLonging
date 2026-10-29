// Synthetic reply evidence through production C#, then deserialize/validate in Rust.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-reply-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
await writeFile(join(report,'ReplyHarness.cs'),`using System;using System.Linq;
public partial class DesktopObserver {
 static T Copy<T>(T value){return json.Deserialize<T>(json.Serialize(value));}
 public static int TestReply(string mode) {
  try {
   var observation=new Observation{window=new Window{app="workbuddy",hwnd="123",pid=42,startTicks="123456",exe=@"D:\\apps\\WorkBuddy.exe",title="WorkBuddy",version="5.7.7.0"},capturedAt="2026-10-10T12:00:00Z",documentRuntimeId="42.1",sessionId=null,contentDigest=new string('a',64),layoutDigest=new string('b',64),modelLabel="Hy3",nodeCount=100,draftEmpty=true,busy=false};
   var history=new[]{new Round{user=new[]{"prior one"},reply="one",completed=true},new Round{user=new[]{"prior two"},reply="two",completed=true},new Round{user=new[]{"prior three"},reply="three",completed=true}};
   var review=new TaskReview{id=new string('1',64),bindingId=new string('2',64),marker="CL_TASK_"+new string('3',32),prompt="synthetic approved task CL_TASK_"+new string('3',32),bridgeSha256=new string('5',64),observation=Copy(observation),baseline=Baseline(history),maxWaitSeconds=10};review.promptSha256=Hash(review.prompt);
   if(mode=="review"){Console.WriteLine(json.Serialize(review));return 0;}
   var last=new Round{user=new[]{review.prompt},reply="Synthetic completed reply\\n"+review.marker,completed=true};
   var rounds=history.Concat(new[]{last}).ToArray();
   switch(mode) {
    case "clipped":rounds=rounds.Skip(1).ToArray();break;
    case "wrong-prompt":last.user=new[]{"other prompt"};break;
    case "quoted-marker":last.reply=review.marker+" quoted, not terminal";break;
    case "cancelled":last.cancelled=true;break;
    case "unfinished":last.completed=false;break;
    case "changed-history":rounds[1].reply="modified";break;
    case "too-short":rounds=rounds.Skip(2).ToArray();break;
    case "duplicate-prompt":rounds[0].user=new[]{review.prompt};review.baseline=Baseline(rounds.Take(3).ToArray());break;
    case "extra-user-text":last.user=new[]{review.prompt,"other text"};break;
    case "new-process":observation.window.pid++;break;
    case "new-document":observation.documentRuntimeId="42.2";break;
    case "new-model":observation.modelLabel="other";break;
    case "busy":observation.busy=true;break;
    case "draft":observation.draftEmpty=false;break;
    case "fake-session":review.observation.sessionId=observation.sessionId="fake";break;
    case "missing-binding":review.bindingId=null;break;
    case "missing-review":review.id=null;break;
    case "version-change":review.observation.window.version=observation.window.version="5.7.8.0";break;
    case "send-expired":SendMessage(review,true);throw new Exception("Unexpected expired send");
    case "stop-requires-observation":StopTask(review,null);throw new Exception("Unexpected stop without observation");
   }
   ReviewTask(review,true);
   Console.WriteLine(json.Serialize(TaskResult(review,new View{observation=observation},rounds)));return 0;
  }catch(InvalidOperationException e){Console.WriteLine(json.Serialize(new{error=e.Message}));return 1;}
 }
}
class ReplyHarness {static int Main(string[] args){return DesktopObserver.TestReply(args[0]);}}
`);
const exe=join(report,'ReplyHarness.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/main:ReplyHarness','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...['DesktopObserver','DesktopTrial','DesktopStop'].map(n=>resolve('crates/core/native/'+n+'.cs')),join(report,'ReplyHarness.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(build.status,0,build.stdout+build.stderr);
function run(mode){const r=spawnSync(exe,[mode],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(r.error,undefined);return {status:r.status,value:JSON.parse(r.stdout)};}
const review=run('review').value;
Object.assign(review,{attempt:{id:'attempt',taskId:'task',workerId:'worker',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'fixture',sessionId:'ui-binding-'+review.bindingId,identityStrength:'ui-observation',workspace:'D:\\test'}},projectId:'project',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null});
await writeFile(join(report,'review.json'),JSON.stringify(review,null,2)+'\n');
const cases=[];
for(const mode of ['success','clipped','wrong-prompt','quoted-marker','cancelled','unfinished','changed-history','too-short','duplicate-prompt','extra-user-text','new-process','new-document','new-model','busy','draft','fake-session','missing-binding','missing-review','version-change','send-expired','stop-requires-observation']){
 const r=run(mode);const accepted=['success','clipped'].includes(mode);
 await writeFile(join(report,mode+'.json'),JSON.stringify(r.value,null,2)+'\n');
 assert.equal(r.status,accepted?0:1,mode+JSON.stringify(r.value));
 if(mode==='send-expired')assert.equal(r.value.error,'trial_expired');
 if(mode==='stop-requires-observation')assert.equal(r.value.error,'stop_review_invalid');
 if(accepted){assert.equal(r.value.taskProof.sessionId,null);assert.equal(r.value.taskProof.localBinding.bindingId,review.bindingId);assert.equal(r.value.taskProof.localBinding.taskReviewDigest,review.id);}
 cases.push({mode,accepted,passed:true,error:r.value.error??null});
}
const rustBuild=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{encoding:'utf8',windowsHide:true,timeout:120000});assert.equal(rustBuild.status,0,rustBuild.stdout+rustBuild.stderr);
const rustExe=rustBuild.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(rustExe);
const interop=spawnSync(rustExe,['--exact','desktop_execution::local_proof_tests::native_workbuddy_reply_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_REPLY_REPORT:report}});
await writeFile(join(report,'rust-contract.log'),interop.stdout+'\n'+interop.stderr);assert.equal(interop.status,0,interop.stdout+interop.stderr);
await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',scope:'synthetic reply evidence through production C# and Rust; no actual application reply',cases,rustContractPassed:true,externalMessages:0,modelCalls:0},null,2)+'\n');
console.log(JSON.stringify({report,status:'passed',cases:cases.length,rustContractPassed:true,externalMessages:0}));
