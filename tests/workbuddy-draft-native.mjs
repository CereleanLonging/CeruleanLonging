// Genuine cross-process UIA over an owned WPF HWND and raw fragment provider.
// WorkBuddy itself is never discovered, operated, or sent a message by this test.
import {spawn,spawnSync} from 'node:child_process';
import {mkdtemp,readFile,rename,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-draft-native-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
function compile(name,sources,refs){
 const exe=join(report,name+'.exe');
 const run=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll','/r:System.Xaml.dll',...refs.map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...sources.map(s=>resolve(s))],{encoding:'utf8',windowsHide:true,timeout:30000});
 assert.equal(run.status,0,run.stdout+run.stderr);return exe;
}
let observer=await readFile('crates/core/native/DesktopObserver.cs','utf8');
observer=observer.replace('public StopObservation stopObservation;','public StopObservation stopObservation;public Observation draftObservation;').replace('else if(request.op=="trialprepare")','else if(request.op=="incidentprepare")result=PrepareIncidentDraft(request.taskReview); else if(request.op=="incidentsubmit")result=SubmitIncidentDraft(request.taskReview,request.draftObservation); else if(request.op=="trialprepare")');
await writeFile(join(report,'Observer.cs'),observer);
const bridge=compile('Observer',[join(report,'Observer.cs'),'crates/core/native/DesktopTrial.cs','crates/core/native/DesktopStop.cs','tests/fixtures/WorkBuddyDraftProbe.cs'],['UIAutomationClient','UIAutomationTypes','WindowsBase']);
const fixture=compile('WorkBuddy',['tests/fixtures/WorkBuddyStopWindow.cs'],['UIAutomationTypes','UIAutomationProvider','WindowsBase','PresentationCore','PresentationFramework']);
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
 const input={id:'a'.repeat(64),bindingId:'b'.repeat(64),marker,prompt,promptSha256:createHash('sha256').update(prompt).digest('hex'),bridgeSha256:createHash('sha256').update(await readFile(bridge)).digest('hex'),observation:prepared.observation,baseline:prepared.baseline,maxWaitSeconds:2,attempt:{id:'attempt',taskId:'task',workerId:'worker',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'fixture',sessionId:'ui-binding-'+'b'.repeat(64),identityStrength:'ui-observation',workspace:'D:\\test'}},projectId:'project',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null};
 await writeFile(join(report,'task-review-input.json'),JSON.stringify(input,null,2)+'\n');await rustContract('prepare');
 const taskReview=JSON.parse(await readFile(join(report,'task-review.json'),'utf8'));
 await change('send-existing');
 const ready=await request('draft-prepare',{op:'incidentprepare',taskReview});
 await request('missing-observation',{op:'incidentsubmit',taskReview},'trial_review_invalid');
 const stale=structuredClone(ready.draftObservation);stale.capturedAt=new Date(Date.now()-60000).toISOString();
 await request('expired',{op:'incidentsubmit',taskReview,draftObservation:stale},'trial_expired');
 const changed=structuredClone(ready.draftObservation);changed.contentDigest='f'.repeat(64);
 await request('changed-observation',{op:'incidentsubmit',taskReview,draftObservation:changed},'trial_view_changed');
 for(const mode of ['send-existing-altered','send-existing-model']) {
  await change(mode);await request(mode,{op:'incidentsubmit',taskReview,draftObservation:ready.draftObservation},'trial_draft_changed');
 }
 assert.equal((await waitJson('send-clicks.json')).sendClicks,0);
 await change('send-existing');const fresh=await request('fresh',{op:'incidentprepare',taskReview});
 const sent=await request('submit-existing',{op:'incidentsubmit',taskReview,draftObservation:fresh.draftObservation});
 assert.equal((await waitJson('send-clicks.json')).sendClicks,1);
 assert.deepEqual(await waitJson('mutations.json'),{valueChanges:0,invokes:0});assert.equal((await waitJson('send-input.json')).characters,0);
 await request('repeat-refused',{op:'incidentsubmit',taskReview,draftObservation:fresh.draftObservation},'trial_draft_changed');
 await request('read-only',{op:'taskread',taskReview});assert.equal((await waitJson('send-clicks.json')).sendClicks,1);
 await writeFile(join(report,'review.json'),JSON.stringify(taskReview,null,2)+'\n');await writeFile(join(report,'success.json'),JSON.stringify(sent,null,2)+'\n');
 const checked=spawnSync(rustExe,['--exact','desktop_execution::local_proof_tests::native_workbuddy_reply_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_REPLY_REPORT:report}});
 await writeFile(join(report,'rust-reply-contract.log'),checked.stdout+'\n'+checked.stderr);assert.equal(checked.status,0,checked.stdout+checked.stderr);assert.match(checked.stdout,/1 passed/);
 result={status:'passed',realUia:true,testOnlyRoute:true,productionClickHelper:true,externalApplicationsUsed:false,externalMessages:0,keyboardCharacters:0,sendClicks:1,invokes:0,cases,rustContractPassed:true,durableProductWorkflowTested:false};
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
