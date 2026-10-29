// Genuine cross-process UIA over an owned WPF HWND and raw fragment provider.
// WorkBuddy itself is never discovered, operated, or sent a message by this test.
import {spawn,spawnSync} from 'node:child_process';
import {mkdtemp,readFile,rename,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {workbuddyFixture} from './workbuddy-owned-version-fixture.mjs';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const version=process.argv[2]??'5.7.7.0';assert.ok(['5.7.7.0','5.8.3.0'].includes(version));const modern=version==='5.8.3.0';
const report=await mkdtemp(resolve('reports/workbuddy-stop-native-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
function compile(name,sources,refs){
 const exe=join(report,name+'.exe');
 const run=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll','/r:System.Xaml.dll',...refs.map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...sources.map(s=>resolve(s))],{encoding:'utf8',windowsHide:true,timeout:30000});
 assert.equal(run.status,0,run.stdout+run.stderr);return exe;
}
const bridge=compile('Observer',['crates/core/native/DesktopObserver.cs','crates/core/native/DesktopTrial.cs','crates/core/native/DesktopStop.cs'],['UIAutomationClient','UIAutomationTypes','WindowsBase']);
const fixture=compile('WorkBuddy',[await workbuddyFixture(report,version)],['UIAutomationTypes','UIAutomationProvider','WindowsBase','PresentationCore','PresentationFramework']);
// Build before opening the UI fixture, so compiler time is not part of any
// observed UI lease and no fixture stays open while Cargo waits for a lock.
const rustBuild=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{encoding:'utf8',windowsHide:true,timeout:120000});
await writeFile(join(report,'rust-build.stdout.log'),rustBuild.stdout??'');await writeFile(join(report,'rust-build.stderr.log'),rustBuild.stderr??'');
assert.equal(rustBuild.status,0,rustBuild.stdout+rustBuild.stderr);
const rustExe=rustBuild.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(rustExe);
async function rustContract(stage){
 const run=spawnSync(rustExe,['--exact','desktop_execution::cancellation::workbuddy_tests::native_workbuddy_stop_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_STOP_REPORT:report,CERULEAN_WORKBUDDY_STOP_STAGE:stage}});
 await writeFile(join(report,'rust-contract-'+stage+'.log'),(run.stdout??'')+'\n'+(run.stderr??''));
 assert.equal(run.error,undefined,run.error?.message);assert.equal(run.status,0,run.stdout+run.stderr);
 assert.match(run.stdout,/1 passed/,'the named Rust contract test must actually execute');
}
const child=spawn(fixture,[report],{windowsHide:true,stdio:['ignore','pipe','pipe']});
let stdout='',stderr='';child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);
let exitInfo=null;
const exited=new Promise(resolve=>{child.once('exit',(code,signal)=>{exitInfo={code,signal};resolve(exitInfo);});child.once('error',error=>{exitInfo={error:error.message};resolve(exitInfo);});});
const cases=[];
async function waitJson(file,predicate=()=>true){const deadline=Date.now()+10000;while(Date.now()<deadline){try{const value=JSON.parse(await readFile(join(report,file),'utf8'));if(predicate(value))return value;}catch(error){if(!['ENOENT','EBUSY','EACCES'].includes(error.code)&&!(error instanceof SyntaxError))throw error;}assert.equal(exitInfo,null,'fixture exited: '+stderr);await new Promise(r=>setTimeout(r,40));}assert.fail('fixture timed out: '+file+' '+stderr);}
async function request(name,body,expected=null){
 const run=spawnSync(bridge,[JSON.stringify({version:1,...body})],{encoding:'utf8',windowsHide:true,timeout:15000});
 await writeFile(join(report,name+'.stdout.log'),run.stdout??'');await writeFile(join(report,name+'.stderr.log'),run.stderr??'');
 assert.equal(run.error,undefined,run.error?.message);let response;try{response=JSON.parse(run.stdout.replace(/^\uFEFF/,''));}catch{assert.fail(name+': '+run.stdout+' '+run.stderr);}
 await writeFile(join(report,name+'.json'),JSON.stringify(response,null,2)+'\n');
 assert.equal(response.ok,expected===null,name+': '+JSON.stringify(response));assert.equal(run.status,expected===null?0:1,name);
 if(expected!==null)assert.ok((Array.isArray(expected)?expected:[expected]).includes(response.error),name+': '+response.error);
 cases.push({name,passed:true,error:response.error??null});return response.result;
}
let sequence=0;
const marker='CL_TASK_0123456789abcdef0123456789abcdef',prompt='Owned WorkBuddy cancellation test '+marker;
async function publish(mode,extra={}){
 const next=++sequence,name='command-'+next+'.json';
 await writeFile(join(report,name+'.tmp'),JSON.stringify({sequence:next,mode,prompt,...extra}),{flag:'wx'});
 await rename(join(report,name+'.tmp'),join(report,name));return next;
}
async function change(mode,extra={}){const next=await publish(mode,extra);await waitJson('ack-'+next+'.json',value=>value.sequence===next&&value.mode===mode);}
async function boundedExit(milliseconds){let timer;try{return await Promise.race([exited,new Promise(resolve=>{timer=setTimeout(()=>resolve(null),milliseconds);})]);}finally{clearTimeout(timer);}}
let result;
try {
 const window=await waitJson('window.json');assert.equal(window.pid,child.pid);
 const prepared=await request('baseline',{op:'taskprepare',window});
 assert.equal(prepared.baseline.length,3);assert.equal(prepared.observation.modelLabel,'Hy3');assert.equal(prepared.observation.sessionId,null);
 const reviewInput={id:'a'.repeat(64),bindingId:'b'.repeat(64),marker,prompt,promptSha256:createHash('sha256').update(prompt).digest('hex'),bridgeSha256:createHash('sha256').update(await readFile(bridge)).digest('hex'),observation:prepared.observation,baseline:prepared.baseline,maxWaitSeconds:10};
 Object.assign(reviewInput,{attempt:{id:'attempt',taskId:'task',workerId:'worker',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'fixture',sessionId:'ui-binding-'+reviewInput.bindingId,identityStrength:'ui-observation',workspace:'D:\\test'}},projectId:'project',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null});
 await writeFile(join(report,'task-review-input.json'),JSON.stringify(reviewInput,null,2)+'\n');
 await rustContract('prepare');const taskReview=JSON.parse(await readFile(join(report,'task-review.json'),'utf8'));
 await change('running');
 const running=await request('running-prepare',{op:'taskstopprepare',taskReview});const stopObservation=running.stopObservation;
 assert.equal(stopObservation.taskReviewDigest,taskReview.id);assert.equal(stopObservation.observation.busy,true);assert.deepEqual(stopObservation.baseline,modern?taskReview.baseline.slice(-2):taskReview.baseline);
 assert.ok(stopObservation.turnRuntimeId.startsWith(prepared.observation.documentRuntimeId+'.4.0.1.0.0.0.1.1.0.0.0.0.0.0.'));
 assert.ok(stopObservation.stopRuntimeId.startsWith(prepared.observation.documentRuntimeId+(modern?'.4.0.1.0.0.0.1.1.0.0.1.0.1.0.2.2.':'.4.0.1.0.0.0.1.1.0.1.0.1.0.2.2.')));
 await request('running-read-refused',{op:'taskstopread',taskReview,stopObservation},'stop_terminal_unverified');
 await request('action-needs-observation',{op:'taskstop',taskReview,stopObservation:null},'stop_review_invalid');
 await request('send-while-busy-refused',{op:'tasksend',taskReview},'session_busy_or_approval_pending');
 for(const status of ['已取消','已停止','已中断','任务已停止','任务已取消']){
  await change('cancelled',{status});const result=await request('cancelled-'+cases.length,{op:'taskstopread',taskReview,stopObservation});
  assert.equal(result.stopProof.cancelled,true);assert.equal(result.stopProof.taskReviewDigest,taskReview.id);assert.equal(result.stopProof.observation.busy,false);assert.deepEqual(result.stopProof.observedBaseline,modern?taskReview.baseline.slice(-2):taskReview.baseline);
  if(status==='已取消')await writeFile(join(report,'cancelled-proof.json'),JSON.stringify({ok:true,result},null,2)+'\n');
 }
 for(const [mode,expected] of [
  ['completed','stop_terminal_unverified'],['quoted-terminal','stop_terminal_unverified'],
  ['history-changed','stop_history_changed'],['prompt-changed','stop_turn_changed'],['wrong-role','stop_turn_changed'],
  ['model-changed','stop_identity_changed'],['draft','stop_identity_changed'],['document-changed','trial_view_changed'],
  ['ambiguous-status','stop_turn_changed'],['duplicate-stop','stop_control_ambiguous'],
 ]){await change(mode);await request(mode,{op:'taskstopread',taskReview,stopObservation},expected);}
 await change('cancelled-empty-reply');const emptyReply=await request('cancelled-empty-reply',{op:'taskstopread',taskReview,stopObservation});assert.equal(emptyReply.stopProof.cancelled,true);
 await change('body-stop');await request('body-stop-is-not-composer',{op:'taskstopprepare',taskReview},'stop_control_ambiguous');
 await change('body-stop-task');await request('body-stop-task-is-not-composer',{op:'taskstopprepare',taskReview},'stop_control_ambiguous');
 await change('missing-status');await request('missing-status-cannot-prove-stop',{op:'taskstopread',taskReview,stopObservation},'stop_terminal_unverified');
 await change('terminal-busy');await request('terminal-while-busy',{op:'taskstopread',taskReview,stopObservation},'stop_terminal_unverified');
 for(const status of ['已取消（引用文字）','已完成 3s ','已失败（引用文字）']){
  await change('malformed-terminal',{status});await request('malformed-terminal-'+cases.length,{op:'taskstopprepare',taskReview},'stop_turn_changed');
 }
 await change('clipped-running');const clipped=await request('clipped-history-prepare',{op:'taskstopprepare',taskReview});assert.deepEqual(clipped.stopObservation.baseline,taskReview.baseline.slice(1));
 await change('clipped-cancelled');const clippedProof=await request('clipped-history-read',{op:'taskstopread',taskReview,stopObservation});assert.deepEqual(clippedProof.stopProof.observedBaseline,taskReview.baseline.slice(1));
 await change('clipped-too-short');await request('one-retained-round-refused',{op:'taskstopread',taskReview,stopObservation},'stop_history_changed');
 await rustContract('validate');
 const readOnlyMutations=await waitJson('mutations.json');assert.deepEqual(readOnlyMutations,{valueChanges:0,invokes:0});
 // Exercise the real Invoke on this owned UIA provider. Each independent
 // scenario starts with a fresh stop observation; uncertainty is only read.
 let expectedInvokes=0;
 for(const mode of ['action-success','action-unconfirmed','action-completed','action-failed']){
  await change(mode);
  const preparedStop=(await request(mode+'-prepare',{op:'taskstopprepare',taskReview})).stopObservation;
  if(mode==='action-success')await writeFile(join(report,'running-prepare.json'),JSON.stringify({ok:true,result:{stopObservation:preparedStop}},null,2)+'\n');
  const expected=mode==='action-success'?null:'stop_terminal_unverified';
  const stopped=await request(mode,{op:'taskstop',taskReview,stopObservation:preparedStop},expected);
  expectedInvokes++;
  assert.deepEqual(await waitJson('mutations.json'),{valueChanges:0,invokes:expectedInvokes});
  if(mode==='action-success'){
   assert.equal(stopped.stopProof.cancelled,true);
   await writeFile(join(report,'cancelled-proof.json'),JSON.stringify({ok:true,result:stopped},null,2)+'\n');
   await rustContract('validate');
   await request(mode+'-repeat-refused',{op:'taskstop',taskReview,stopObservation:preparedStop},'stop_not_running');
  }
  await request(mode+'-readonly',{op:'taskstopread',taskReview,stopObservation:preparedStop},expected);
  assert.deepEqual(await waitJson('mutations.json'),{valueChanges:0,invokes:expectedInvokes});
  if(mode==='action-unconfirmed'){
   await change('cancelled');
   await request(mode+'-late-readonly',{op:'taskstopread',taskReview,stopObservation:preparedStop});
   assert.deepEqual(await waitJson('mutations.json'),{valueChanges:0,invokes:expectedInvokes});
  }
 }
 // Identity and stale-intent failures must reject before any Invoke.
 for(const [mode,error] of [['model-changed','stop_identity_changed'],['prompt-changed','stop_turn_changed'],['duplicate-stop','stop_control_ambiguous'],['draft','stop_identity_changed']]){
  await change('action-success');const guard=(await request('guard-'+mode+'-prepare',{op:'taskstopprepare',taskReview})).stopObservation;
  await change(mode);await request('guard-'+mode,{op:'taskstop',taskReview,stopObservation:guard},error);
 }
 await change('action-success');const stale=(await request('expired-prepare',{op:'taskstopprepare',taskReview})).stopObservation;
 stale.observation.capturedAt=new Date(Date.now()-60000).toISOString();
 await request('expired-action-refused',{op:'taskstop',taskReview,stopObservation:stale},'stop_review_expired');
 const mutations=await waitJson('mutations.json');assert.deepEqual(mutations,{valueChanges:0,invokes:expectedInvokes});
 result={status:'passed',version,realUia:true,provider:'owned WPF HWND with raw UIA fragment tree and positional runtime IDs',unmodifiedProductionObserver:true,externalApplicationsUsed:false,messagesSent:0,modelCalls:0,...mutations,readOnlyMutations,actionScenarios:4,cases,rustContractPassed:true};
} catch(error) {
 result={status:'failed',realUia:true,cases,error:{name:error.name,message:error.message,stack:error.stack}};throw error;
} finally {
 // Ask only this owned fixture to close; force only its child if it fails to
 // respond. The outer Windows Job still enforces descendant cleanup on timeout.
 let forced=false,cleanupError=null;
 try{if(exitInfo===null){await publish('shutdown');if(await boundedExit(3000)===null){forced=true;child.kill();await boundedExit(3000);}}}catch(error){cleanupError=error.message;forced=true;child.kill();await boundedExit(3000);}
 await writeFile(join(report,'fixture.log'),stdout+'\n'+stderr);
 await writeFile(join(report,'fixture-cleanup.json'),JSON.stringify({exitInfo,forced,cleanupError},null,2)+'\n');
 if(exitInfo===null||cleanupError!==null){result.status='failed';result.cleanupError=cleanupError??'fixture did not exit';}
 await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');
 console.log(JSON.stringify({report,status:result.status,cases:cases.length,realUia:true,rustContractPassed:result.rustContractPassed??false}));
 if(result.status!=='passed')process.exitCode=1;
}
