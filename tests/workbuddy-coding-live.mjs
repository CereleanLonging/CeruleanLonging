// Default: read-only UI preflight plus an empty isolated output directory.
// A single file-writing task requires explicit approval of the exact plan hash.
// Native coding evidence only; product management/workflow is not exercised here.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile,open,mkdir,readdir,lstat,realpath} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const hash=value=>createHash('sha256').update(value).digest('hex');
const planBytes=await readFile('tests/fixtures/workbuddy-coding-plan.json');
const plan=JSON.parse(planBytes),planHash=hash(planBytes);
assert.equal(plan.maxMessages,1);assert.equal(plan.messages.length,1);
assert.equal(plan.version,'5.7.7.0');assert.equal(plan.chat,'工作台接入验证');assert.equal(plan.modelLabel,'Hy3');
assert.ok(plan.maxWaitSeconds>=1&&plan.maxWaitSeconds<=50);
for(const prompt of plan.messages){const markers=prompt.match(/CL_TASK_[0-9a-f]+/g)??[];assert.equal(markers.length,1);assert.match(markers[0],/^CL_TASK_[0-9a-f]{32}$/,'The complete marker must be exactly 32 hex digits; never truncate');assert.ok(prompt.length<1500&&!/[\r\n]/.test(prompt));}
const execute=process.env.CERULEAN_WORKBUDDY_CODING_APPROVAL===planHash;
assert.ok(!process.env.CERULEAN_WORKBUDDY_CODING_APPROVAL||execute,'Approval must match the exact plan file SHA256');
const report=await mkdtemp(resolve('reports/workbuddy-coding-send-'));
await writeFile(join(report,'approved-scope.json'),JSON.stringify({plan,planHash,execute},null,2)+'\n');
const expectedProject=resolve('reports/workbuddy-coding-live-20261011-01/project');
assert.equal(plan.projectDirectory.toLowerCase(),expectedProject.toLowerCase());assert.equal(plan.artifact,'sum.mjs');
assert.equal((await realpath(resolve('reports'))).toLowerCase(),resolve('reports').toLowerCase(),'report root must not redirect');
const caseRoot=resolve('reports/workbuddy-coding-live-20261011-01');
try{await mkdir(caseRoot);}catch(e){if(e.code!=='EEXIST')throw e;}
assert.equal((await realpath(caseRoot)).toLowerCase(),caseRoot.toLowerCase());
try{await mkdir(expectedProject);}catch(e){if(e.code!=='EEXIST')throw e;}
assert.equal((await realpath(expectedProject)).toLowerCase(),expectedProject.toLowerCase());
assert.deepEqual(await readdir(expectedProject),[],'coding output directory must be empty before any send');
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
async function verifyArtifact(){
 const target=join(expectedProject,plan.artifact);
 let stat;try{stat=await lstat(target);}catch(e){if(e.code==='ENOENT')return {status:'missing'};throw e;}
 assert.ok(stat.isFile()&&!stat.isSymbolicLink()&&stat.size<=65536,'expected a regular artifact of at most 64 KiB');
 assert.equal((await realpath(target)).toLowerCase(),target.toLowerCase(),'artifact must not redirect');
 const bytes=await readFile(target);assert.ok(bytes.length<=65536);
 const folder=join(report,'artifact');await mkdir(folder);
 const snapshot=join(folder,'sum.mjs');await writeFile(snapshot,bytes,{flag:'wx'});
 const verifier=resolve('tests/fixtures/workbuddy-coding-verifier.mjs');
 const env=Object.fromEntries(['SystemRoot','WINDIR','TEMP','TMP'].filter(k=>process.env[k]).map(k=>[k,process.env[k]]));
 const result=spawnSync(process.execPath,['--permission','--allow-fs-read='+snapshot,'--allow-fs-read='+verifier,'--experimental-vm-modules',verifier,snapshot],{encoding:'utf8',windowsHide:true,timeout:5000,env});
 await writeFile(join(report,'artifact-verification.log'),(result.stdout??'')+'\n'+(result.stderr??''));
 const checked=result.status===0&&!result.error?JSON.parse(result.stdout):null;
 const unchanged=hash(await readFile(target))===hash(bytes);
 return {status:checked?.status==='passed'&&checked.checks===15&&unchanged?'passed':'failed',checks:checked?.checks??0,sha256:hash(bytes),verifierSha256:hash(await readFile(verifier)),unchanged,exitCode:result.status,error:result.error?.code??null};
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
   const input={id:'a'.repeat(64),bindingId:hash(JSON.stringify(window)),marker,prompt,promptSha256:hash(prompt),bridgeSha256:hash(await readFile(bridge)),observation:prepared.observation,baseline:prepared.baseline,maxWaitSeconds:plan.maxWaitSeconds,attempt:{id:'probe-'+i,taskId:'probe-'+i,workerId:'workbuddy-probe',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'native-probe',sessionId:'ui-binding-'+hash(JSON.stringify(window)),identityStrength:'ui-observation',workspace:plan.projectDirectory}},projectId:'live-probe',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null};
   const stage=join(report,'task-'+i);await import('node:fs/promises').then(fs=>fs.mkdir(stage));
   await writeFile(join(stage,'task-review-input.json'),JSON.stringify(input,null,2)+'\n');
   const canonical=spawnSync(rust,['--exact','desktop_execution::cancellation::workbuddy_tests::native_workbuddy_stop_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_STOP_REPORT:stage,CERULEAN_WORKBUDDY_STOP_STAGE:'prepare'}});assert.equal(canonical.status,0,canonical.stdout+canonical.stderr);assert.match(canonical.stdout,/1 passed/);
   const review=JSON.parse(await readFile(join(stage,'task-review.json'),'utf8'));
   // Record the attempt before a possibly uncertain side effect. Never retry.
   const receipt=await open(resolve('reports','workbuddy-coding-send-'+planHash+'.json'),'wx');try{await receipt.writeFile(JSON.stringify({planHash,report,at:new Date().toISOString()})+'\n');await receipt.sync();}finally{await receipt.close();}
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
finally{
 if(sends>0){
  try{outcome.artifact=await verifyArtifact();}catch(e){outcome.artifact={status:'failed',error:e.message};}
  if(outcome.artifact.status!=='passed'&&outcome.status==='passed')outcome.status='artifact-failed';
 }
 outcome.managementTested=false;
 await writeFile(join(report,'result.json'),JSON.stringify(outcome,null,2)+'\n');console.log(JSON.stringify({report,...outcome}));
 if(!['passed','ready-read-only'].includes(outcome.status))process.exitCode=1;
}
