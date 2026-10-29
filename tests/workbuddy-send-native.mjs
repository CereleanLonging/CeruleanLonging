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
const report=await mkdtemp(resolve('reports/workbuddy-send-native-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
function compile(name,sources,refs){
 const exe=join(report,name+'.exe');
 const run=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll','/r:System.Xaml.dll',...refs.map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...sources.map(s=>resolve(s))],{encoding:'utf8',windowsHide:true,timeout:30000});
 assert.equal(run.status,0,run.stdout+run.stderr);return exe;
}
const diagnostic=process.env.CERULEAN_OWNED_SEND_DIAGNOSTIC==='1';
let observer='crates/core/native/DesktopObserver.cs';
if(diagnostic){observer=join(report,'DiagnosticObserver.cs');await writeFile(observer,(await readFile('crates/core/native/DesktopObserver.cs','utf8')).replace('} catch(Exception e) {','} catch(Exception e) { Console.Error.WriteLine(e.ToString());'));}
const bridge=compile('Observer',[observer,'crates/core/native/DesktopTrial.cs','crates/core/native/DesktopStop.cs'],['UIAutomationClient','UIAutomationTypes','WindowsBase']);
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
const marker='CL_TASK_0123456789abcdef0123456789abcdef',prompt='受控 WorkBuddy 编程消息：引号"与 Unicode 🐇 '+marker;
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
 let sendClicks=0,successful=0;
 for(const mode of ['send-success','send-click-success','send-click-wrong-target','send-click-draft','send-button-wrong-target','send-unconfirmed','send-slow-unconfirmed','send-model-changed','send-disabled','send-duplicate']) {
  await change(mode,{status:'owned reply '+marker});
  const prepared=await request(mode+'-prepare',{op:'taskprepare',window});
  const reviewInput={id:'a'.repeat(64),bindingId:'b'.repeat(64),marker,prompt,promptSha256:createHash('sha256').update(prompt).digest('hex'),bridgeSha256:createHash('sha256').update(await readFile(bridge)).digest('hex'),observation:prepared.observation,baseline:prepared.baseline,maxWaitSeconds:1};
  Object.assign(reviewInput,{attempt:{id:'attempt',taskId:'task',workerId:'worker',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'fixture',sessionId:'ui-binding-'+reviewInput.bindingId,identityStrength:'ui-observation',workspace:'D:\\test'}},projectId:'project',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null});
  await writeFile(join(report,'task-review-input.json'),JSON.stringify(reviewInput,null,2)+'\n');
  await rustContract('prepare');const taskReview=JSON.parse(await readFile(join(report,'task-review.json'),'utf8'));
  if(mode==='send-success') {
   const stale=structuredClone(taskReview);stale.observation.capturedAt=new Date(Date.now()-180000).toISOString();
   await request('stale-before-input',{op:'tasksend',taskReview:stale},'trial_expired');
   const wrong=structuredClone(taskReview);wrong.prompt+=' altered';
   await request('prompt-digest-before-input',{op:'tasksend',taskReview:wrong},'trial_review_invalid');
   assert.equal((await waitJson('send-input.json')).characters,0);
  }
  let error=null;
  if(mode==='send-click-wrong-target'||mode==='send-button-wrong-target')error='trial_focus_changed';
  if(mode==='send-click-draft')error='draft_or_composer_unavailable';
  if(mode==='send-unconfirmed'||mode==='send-slow-unconfirmed')error='trial_reply_unverified';
  if(mode==='send-model-changed')error='trial_draft_changed';
  if(mode==='send-disabled'||mode==='send-duplicate')error='trial_send_unavailable';
  const began=Date.now();const sent=await request(mode,{op:'tasksend',taskReview},error);
  if(mode==='send-slow-unconfirmed'){assert.equal((await waitJson('slow-reads.json')).reads,1,'one slow read consumes the elapsed wait budget');await writeFile(join(report,'slow-wait-result.json'),JSON.stringify({maxWaitSeconds:1,readDelayMs:1200,observedReads:1,totalSendElapsedMs:Date.now()-began})+'\n');}
  if(mode==='send-unconfirmed')assert.ok(Date.now()-began<8000,'reply wait must include UIA observation time');
  if(mode==='send-success'||mode==='send-click-success'||mode==='send-unconfirmed'||mode==='send-slow-unconfirmed')sendClicks++;
  assert.deepEqual(await waitJson('mutations.json'),{valueChanges:0,invokes:0});assert.equal((await waitJson('send-clicks.json')).sendClicks,sendClicks);
  if(!['send-duplicate','send-click-wrong-target','send-click-draft'].includes(mode)) {
   const input=await waitJson('send-input.json');assert.equal(input.text,prompt);assert.equal(input.characters,prompt.length);
   await writeFile(join(report,mode+'-input.json'),JSON.stringify(input,null,2)+'\n');
  } else assert.equal((await waitJson('send-input.json')).characters,0);
  if(mode==='send-success') {
   successful++;
   assert.equal(sent.taskProof.localBinding.taskReviewDigest,taskReview.id);
   await writeFile(join(report,'review.json'),JSON.stringify(taskReview,null,2)+'\n');
   await writeFile(join(report,'success.json'),JSON.stringify(sent,null,2)+'\n');
   const interop=spawnSync(rustExe,['--exact','desktop_execution::local_proof_tests::native_workbuddy_reply_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_REPLY_REPORT:report}});
   await writeFile(join(report,'rust-reply-contract.log'),interop.stdout+'\n'+interop.stderr);assert.equal(interop.status,0,interop.stdout+interop.stderr);assert.match(interop.stdout,/1 passed/);
   await request('success-readonly',{op:'taskread',taskReview});
   const next=await request('continuation-prepare',{op:'taskprepare',window,taskReview});assert.equal(next.baseline.length,modern?3:4);
   await request('repeat-send-refused',{op:'tasksend',taskReview},'trial_view_changed');
   const altered=structuredClone(taskReview);altered.observation.modelLabel='Different';
   await request('continuation-wrong-model',{op:'taskprepare',window,taskReview:altered},'trial_view_changed');
   await request('continuation-invalid-review',{op:'taskprepare',window,taskReview:{}},'trial_review_invalid');
   const nextMarker='CL_TASK_fedcba9876543210fedcba9876543210',nextPrompt='连续任务 '+nextMarker;
   const nextInput={...taskReview,marker:nextMarker,prompt:nextPrompt,promptSha256:createHash('sha256').update(nextPrompt).digest('hex'),observation:next.observation,baseline:next.baseline};
   await writeFile(join(report,'task-review-input.json'),JSON.stringify(nextInput,null,2)+'\n');await rustContract('prepare');
   const secondReview=JSON.parse(await readFile(join(report,'task-review.json'),'utf8'));
   const second=await request('continuation-send',{op:'tasksend',taskReview:secondReview});sendClicks++;
   assert.equal(second.taskProof.marker,nextMarker);assert.equal(second.taskProof.observedBaseline.length,modern?2:4);
   const secondInput=await waitJson('send-input.json');assert.equal(secondInput.text,nextPrompt);assert.equal(secondInput.characters,prompt.length+nextPrompt.length);
   await writeFile(join(report,'continuation-input.json'),JSON.stringify(secondInput,null,2)+'\n');
   const third=await request('continuation-second-read',{op:'taskprepare',window,taskReview:secondReview});assert.equal(third.baseline.length,modern?3:5);
  }
  if(mode==='send-unconfirmed')await request('unknown-readonly',{op:'taskread',taskReview},'trial_reply_unverified');
  assert.deepEqual(await waitJson('mutations.json'),{valueChanges:0,invokes:0});assert.equal((await waitJson('send-clicks.json')).sendClicks,sendClicks);
 }
 result={status:'passed',version,realUia:true,unmodifiedProductionObserver:!diagnostic,externalApplicationsUsed:false,externalMessages:0,modelCalls:0,keyboardInputTested:true,valueChanges:0,invokes:0,sendClicks,successful,syntheticReview:true,durableWorkflowTested:false,cases,rustContractPassed:true};
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
