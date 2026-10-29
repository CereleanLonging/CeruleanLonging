// Default: read-only preflight. Execution requires explicit approval of the plan hash.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile,open} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const hash=value=>createHash('sha256').update(value).digest('hex');
const planBytes=await readFile('tests/fixtures/workbuddy-restart-plan.json');
const plan=JSON.parse(planBytes),planHash=hash(planBytes);
assert.equal(plan.maxMessages,1);assert.equal(plan.messages.length,1);
assert.equal(plan.version,'5.7.7.0');assert.equal(plan.chat,'工作台接入验证');assert.equal(plan.modelLabel,'Hy3');
assert.ok(plan.maxWaitSeconds>=1&&plan.maxWaitSeconds<=20);
for(const prompt of plan.messages){assert.equal((prompt.match(/CL_TASK_[0-9a-f]{32}/g)??[]).length,1);assert.ok(prompt.length<300&&!/[\r\n]/.test(prompt));}
const execute=process.env.CERULEAN_WORKBUDDY_RESTART_APPROVAL===planHash;
assert.ok(!process.env.CERULEAN_WORKBUDDY_RESTART_APPROVAL||execute,'Approval must match the exact plan file SHA256');
const report=await mkdtemp(resolve('reports/workbuddy-restart-send-'));
await writeFile(join(report,'approved-scope.json'),JSON.stringify({plan,planHash,execute},null,2)+'\n');
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
// Add only a read-only preparation operation restricted to the named test chat.
const original=await readFile('crates/core/native/DesktopObserver.cs','utf8');
const needle='else if(request.op=="trialprepare")';assert.equal(original.split(needle).length,2);
await writeFile(join(report,'Observer.cs'),original.replace(needle,'else if(request.op=="liveprepare")result=PrepareNamedLive(request.window,request.taskReview);\n            '+needle));
await writeFile(join(report,'NamedPreparation.cs'),`using System;using System.Linq;using System.Windows.Automation;
public partial class DesktopObserver {
 static object PrepareNamedLive(Window w,TaskReview previous){
  if(w==null||w.app!="workbuddy"||w.version!="5.7.7.0")throw new InvalidOperationException("trial_app_not_enabled");
  var before=ReadView(w,true);string header=before.observation.documentRuntimeId+".4.0.1.0.0.0.1.0.0.0";
  if(before.observation.modelLabel!="Hy3"||before.nodes.Count(n=>Runtime(n)==header&&n.Cached.ControlType==ControlType.Text&&n.Cached.Name=="工作台接入验证"&&!n.Cached.IsOffscreen)!=1)throw new InvalidOperationException("trial_view_changed");
  var prepared=PrepareTask(w,previous);var after=ReadView(w,true);
  if(!SameView(before.observation,after.observation))throw new InvalidOperationException("trial_view_changed");return prepared;
 }
}`);
const bridge=join(report,'Observer.exe');
const compile=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+bridge,join(report,'Observer.cs'),join(report,'NamedPreparation.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});
assert.equal(compile.status,0,compile.stdout+compile.stderr);
let sends=0;
async function run(name,body){
 const r=spawnSync(bridge,[JSON.stringify({version:1,...body})],{encoding:'utf8',windowsHide:true,timeout:90000});
 await writeFile(join(report,name+'.stderr.log'),r.stderr??'');
 if(r.error)throw Error(name+': '+(r.error.code??'process-error'));
 const response=JSON.parse(r.stdout.replace(/^\uFEFF/,''));await writeFile(join(report,name+'.json'),JSON.stringify(response,null,2)+'\n');
 assert.equal(r.status,0,name+': '+JSON.stringify(response));assert.equal(response.ok,true);return response.result;
}
let outcome;
try {
 const discovered=await run('discovery',{op:'discover'});
 const candidates=discovered.windows.filter(w=>w.app==='workbuddy');assert.equal(candidates.length,1,'Expected one WorkBuddy window');
 const window=candidates[0];const first=await run('preflight',{op:'liveprepare',window});
 assert.equal(first.observation.sessionId,null);assert.equal(first.observation.modelLabel,plan.modelLabel);assert.ok(first.baseline.length>=2);
 if(execute){
  const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{encoding:'utf8',windowsHide:true,timeout:120000});assert.equal(build.status,0,build.stderr);
  const rust=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(rust);
  let previous=null;
  for(let i=0;i<1;i++){
   const prepared=await run('prepare-'+i,{op:'liveprepare',window,taskReview:previous});
   const prompt=plan.messages[i],marker=prompt.match(/CL_TASK_[0-9a-f]{32}/)[0];
   const input={id:'a'.repeat(64),bindingId:hash(JSON.stringify(window)),marker,prompt,promptSha256:hash(prompt),bridgeSha256:hash(await readFile(bridge)),observation:prepared.observation,baseline:prepared.baseline,maxWaitSeconds:plan.maxWaitSeconds,attempt:{id:'probe-'+i,taskId:'probe-'+i,workerId:'workbuddy-probe',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'native-probe',sessionId:'ui-binding-'+hash(JSON.stringify(window)),identityStrength:'ui-observation',workspace:'D:\\test'}},projectId:'live-probe',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null};
   const stage=join(report,'task-'+i);await import('node:fs/promises').then(fs=>fs.mkdir(stage));
   await writeFile(join(stage,'task-review-input.json'),JSON.stringify(input,null,2)+'\n');
   const canonical=spawnSync(rust,['--exact','desktop_execution::cancellation::workbuddy_tests::native_workbuddy_stop_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_STOP_REPORT:stage,CERULEAN_WORKBUDDY_STOP_STAGE:'prepare'}});assert.equal(canonical.status,0,canonical.stdout+canonical.stderr);assert.match(canonical.stdout,/1 passed/);
   const review=JSON.parse(await readFile(join(stage,'task-review.json'),'utf8'));
   // Record the attempt before a possibly uncertain side effect. Never retry.
   const receipt=await open(resolve('reports','workbuddy-restart-send-'+planHash+'.json'),'wx');try{await receipt.writeFile(JSON.stringify({planHash,report,at:new Date().toISOString()})+'\n');await receipt.sync();}finally{await receipt.close();}
   sends++;await writeFile(join(report,'attempts.json'),JSON.stringify({planHash,sendAttempts:sends})+'\n');
   const sent=await run('send-'+i,{op:'tasksend',taskReview:review});
   await writeFile(join(stage,'review.json'),JSON.stringify(review,null,2)+'\n');await writeFile(join(stage,'success.json'),JSON.stringify(sent,null,2)+'\n');
   const checked=spawnSync(rust,['--exact','desktop_execution::local_proof_tests::native_workbuddy_reply_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_REPLY_REPORT:stage}});
   await writeFile(join(stage,'rust-reply-contract.log'),checked.stdout+'\n'+checked.stderr);assert.equal(checked.status,0,checked.stdout+checked.stderr);assert.match(checked.stdout,/1 passed/);
   previous=review;
  }
  await run('final-continuation',{op:'liveprepare',window,taskReview:previous});
 }
 outcome={status:execute?'passed':'ready-read-only',capturedAt:new Date().toISOString(),planHash,sendAttempts:sends,readOnly:!execute,initialHistoryRounds:first.baseline.length,modelLabel:first.observation.modelLabel,syntheticTaskReview:execute,durableWorkflowTested:false};
}catch(error){outcome={status:'failed',planHash,sendAttempts:sends,error:error.message,durableWorkflowTested:false};throw error;}
finally{await writeFile(join(report,'result.json'),JSON.stringify(outcome,null,2)+'\n');console.log(JSON.stringify({report,...outcome}));}
