// Real UIA with the existing owned provider, adapted only to observed 5.8.3 paths.
// Exercises read contracts and confirms unsupported versions remain gated.
import {spawn,spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile,rename} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-583-tasks-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
let source=await readFile('tests/fixtures/WorkBuddyStopWindow.cs','utf8');
source=source.replaceAll('5.7.7.0','5.8.3.0').replace('List=Panel+".1.0.0.0.0.0.0",Composer=Panel+".1.0.1.0.1.0"','List=Panel+".1.0.0.0.0.0.0.0",Composer=Panel+".1.0.0.1.0.1.0"');
assert.ok(source.includes('List=Panel+".1.0.0.0.0.0.0.0"'));
source=source.replace('"":"owned partial reply",status);','"":"owned partial reply "+System.Text.RegularExpressions.Regex.Match(command.prompt??"",@"CL_TASK_[0-9a-f]{32}").Value,status);');
assert.ok(source.includes('"owned partial reply "+'));
const provider=join(report,'Provider.cs');await writeFile(provider,source);
function compile(name,sources,refs){const exe=join(report,name+'.exe');const r=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/codepage:65001','/r:System.Web.Extensions.dll','/r:System.Xaml.dll',...refs.map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...sources],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(r.status,0,r.stdout+r.stderr);return exe;}
const fixture=compile('WorkBuddy',[provider],['UIAutomationTypes','UIAutomationProvider','WindowsBase','PresentationCore','PresentationFramework']);
const bridge=compile('Observer',['DesktopObserver','DesktopTrial','DesktopStop'].map(n=>resolve('crates/core/native/'+n+'.cs')),['UIAutomationClient','UIAutomationTypes','WindowsBase']);
const rustBuild=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{windowsHide:true,encoding:'utf8',timeout:120000});
await writeFile(join(report,'rust-build.stdout.log'),rustBuild.stdout??'');await writeFile(join(report,'rust-build.stderr.log'),rustBuild.stderr??'');assert.equal(rustBuild.status,0,rustBuild.stderr);
const rustExe=rustBuild.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(x=>x.reason==='compiler-artifact'&&x.profile.test&&x.target.kind.includes('lib')&&x.executable)?.executable;assert.ok(rustExe);
async function rustCheck(stage){const name=stage==='reply'?'desktop_execution::local_proof_tests::native_workbuddy_reply_contract':'desktop_execution::cancellation::workbuddy_tests::native_workbuddy_stop_contract';const r=spawnSync(rustExe,['--exact',name,'--ignored','--nocapture'],{windowsHide:true,encoding:'utf8',timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_STOP_REPORT:report,CERULEAN_WORKBUDDY_STOP_STAGE:stage,CERULEAN_WORKBUDDY_REPLY_REPORT:report}});await writeFile(join(report,'rust-'+stage+'.log'),(r.stdout??'')+'\n'+(r.stderr??''));assert.equal(r.status,0,r.stdout+r.stderr);assert.match(r.stdout,/1 passed/);}
const child=spawn(fixture,[report],{windowsHide:true,stdio:['ignore','pipe','pipe']});let stdout='',stderr='',exitInfo=null;
child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);
const exited=new Promise(resolve=>{child.once('exit',(code,signal)=>{exitInfo={code,signal};resolve();});child.once('error',e=>{exitInfo={error:e.message};resolve();});});
async function waitJson(name){const until=Date.now()+10000;while(Date.now()<until){assert.equal(exitInfo,null,stderr);try{return JSON.parse(await readFile(join(report,name),'utf8'));}catch(e){if(!['ENOENT','EBUSY','EACCES'].includes(e.code)&&!(e instanceof SyntaxError))throw e;}await new Promise(r=>setTimeout(r,30));}assert.fail('fixture timeout '+name);}
const marker='CL_TASK_0123456789abcdef0123456789abcdef',prompt='Synthetic approved task '+marker;
const cases=[];let sequence=0;
async function change(mode,extra={}){const n=++sequence,file=join(report,`command-${n}.json`);await writeFile(file+'.tmp',JSON.stringify({sequence:n,mode,prompt,...extra}),{flag:'wx'});await rename(file+'.tmp',file);if(mode!=='shutdown')await waitJson(`ack-${n}.json`);}
async function request(name,body,expected=null){const r=spawnSync(bridge,[JSON.stringify({version:1,...body})],{windowsHide:true,encoding:'utf8',timeout:15000});assert.equal(r.error,undefined);const result=JSON.parse(r.stdout.replace(/^\uFEFF/,''));await writeFile(join(report,name+'.json'),JSON.stringify(result,null,2));assert.equal(result.ok,expected===null,name+': '+r.stdout);assert.equal(r.status,expected===null?0:1);if(expected)assert.ok((Array.isArray(expected)?expected:[expected]).includes(result.error),name+': '+result.error);cases.push({name,passed:true,error:result.error??null});return result.result;}
let result;
try {
 const window=await waitJson('window.json');assert.equal(window.pid,child.pid);assert.equal(window.version,'5.8.3.0');
 const first=await request('prepare-initial',{op:'taskprepare',window});assert.equal(first.baseline.length,3);
 const inputReview={id:'1'.repeat(64),bindingId:'2'.repeat(64),bridgeSha256:'3'.repeat(64),marker,prompt,promptSha256:createHash('sha256').update(prompt).digest('hex'),observation:first.observation,baseline:first.baseline,maxWaitSeconds:10,
  attempt:{id:'attempt',taskId:'task',workerId:'worker',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'fixture',sessionId:'ui-binding-'+'2'.repeat(64),identityStrength:'ui-observation',workspace:'D:\\test'}},projectId:'project',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null};
 await writeFile(join(report,'task-review-input.json'),JSON.stringify(inputReview,null,2));await rustCheck('prepare');const taskReview=JSON.parse(await readFile(join(report,'task-review.json'),'utf8'));
 const unsupported=structuredClone(taskReview);unsupported.observation.window.version='5.8.4.0';
 await request('send-unknown-version-gate',{op:'tasksend',taskReview:unsupported},'trial_app_not_enabled');
 await request('stop-unknown-version-gate',{op:'taskstop',taskReview:unsupported,stopObservation:null},'trial_app_not_enabled');
 await change('completed');
 const success=await request('read-original-completed',{op:'taskread',taskReview});assert.equal(success.taskProof.completed,true);assert.equal(success.taskProof.sessionId,null);assert.deepEqual(success.taskProof.observedBaseline,first.baseline.slice(-2));
 const second=await request('prepare-after-original',{op:'taskprepare',window,taskReview});assert.equal(second.baseline.length,3);
 const recovered=await request('reopen-read-same-result',{op:'taskread',taskReview});assert.equal(success.taskProof.replySha256,recovered.taskProof.replySha256);assert.equal(success.taskProof.roundSha256,recovered.taskProof.roundSha256);
 for(const [field,value] of [['modelLabel','Different'],['documentRuntimeId','different-document']]){
  const wrong=structuredClone(taskReview);wrong.observation[field]=value;await request('reject-'+field,{op:'taskread',taskReview:wrong},'trial_view_changed');
 }
 const wrongBaseline=structuredClone(taskReview);wrongBaseline.baseline[2]='4'.repeat(64);await request('reject-history-change',{op:'taskread',taskReview:wrongBaseline},'trial_reply_unverified');
 const wrongVersion=structuredClone(taskReview);wrongVersion.observation.window.version='5.8.4.0';await request('reject-unknown-version',{op:'taskread',taskReview:wrongVersion},'trial_app_not_enabled');
 await change('draft');await request('preserve-draft',{op:'taskread',taskReview},['trial_reply_unverified','trial_history_unavailable']);
 await change('running');
 const progress=await request('observe-running',{op:'taskprogress',taskReview});assert.equal(progress.taskProgress.statusText,'生成中');assert.equal(progress.taskProgress.taskReviewDigest,taskReview.id);
 const stopped=await request('stop-prepare-read-only',{op:'taskstopprepare',taskReview});assert.equal(stopped.stopObservation.observation.busy,true);assert.deepEqual(stopped.stopObservation.baseline,first.baseline.slice(-2));
 await request('stop-unknown-version-with-valid-observation',{op:'taskstop',taskReview:unsupported,stopObservation:stopped.stopObservation},'trial_app_not_enabled');
 await change('duplicate-stop');await request('duplicate-stop-refused',{op:'taskprogress',taskReview},'stop_control_ambiguous');
 await change('body-stop-task');await request('body-stop-not-composer',{op:'taskprogress',taskReview},'stop_control_ambiguous');
 await change('prompt-changed');await request('different-original-task-refused',{op:'taskprogress',taskReview},'stop_turn_changed');
 await change('cancelled');
 const cancelled=await request('read-original-cancelled',{op:'taskstopread',taskReview,stopObservation:stopped.stopObservation});assert.equal(cancelled.stopProof.cancelled,true);
 await request('cancelled-not-completed',{op:'taskread',taskReview},'trial_reply_unverified');
 await change('completed');await request('completed-not-cancelled',{op:'taskstopread',taskReview,stopObservation:stopped.stopObservation},'stop_terminal_unverified');
 const mutation=await waitJson('mutations.json');assert.deepEqual(mutation,{valueChanges:0,invokes:0});assert.equal((await waitJson('send-clicks.json')).sendClicks,0);
 await writeFile(join(report,'review.json'),JSON.stringify(taskReview,null,2));await writeFile(join(report,'success.json'),JSON.stringify(success,null,2));
 await writeFile(join(report,'running-prepare.json'),await readFile(join(report,'stop-prepare-read-only.json')));await writeFile(join(report,'cancelled-proof.json'),await readFile(join(report,'read-original-cancelled.json')));
 await rustCheck('validate');await rustCheck('reply');
 result={status:'passed',cases,realUia:true,ownedProvider:true,sourceVersion:'5.8.3.0',realModelCalls:0,externalMessages:0,valueChanges:0,invokes:0,sendClicks:0,unsupportedVersionActionsGated:true,rustCanonicalReview:true,rustReplyAndStopContractsPassed:true};
}catch(error){result={status:'failed',cases,error:error.stack};throw error;}
finally{
 if(exitInfo===null){await change('shutdown');let timer;await Promise.race([exited,new Promise(r=>{timer=setTimeout(r,3000);})]);clearTimeout(timer);if(exitInfo===null){child.kill();await exited;}}
 await writeFile(join(report,'fixture.log'),stdout+'\n'+stderr);await writeFile(join(report,'fixture-cleanup.json'),JSON.stringify(exitInfo));await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,status:result?.status,cases:cases.length}));
}
