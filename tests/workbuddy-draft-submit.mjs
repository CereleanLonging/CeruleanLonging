// Default read-only preflight. Exact-plan approval allows one persisted click attempt.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile,open} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const hash=data=>createHash('sha256').update(data).digest('hex');
const planBytes=await readFile('tests/fixtures/workbuddy-draft-submit-plan.json'),plan=JSON.parse(planBytes),planHash=hash(planBytes);
assert.equal(plan.sourceReport,'reports/workbuddy-send-live-7CgjNy');assert.equal(plan.maxClicks,1);assert.equal(plan.keyboardInput,false);assert.equal(plan.secondMessage,false);
const review=JSON.parse(await readFile(join(plan.sourceReport,'task-0/task-review.json'),'utf8'));
assert.equal(review.id,plan.taskReviewDigest);assert.equal(review.promptSha256,plan.promptSha256);assert.equal(hash(review.prompt),plan.promptSha256);
const execute=process.env.CERULEAN_WORKBUDDY_DRAFT_APPROVAL===planHash;
assert.ok(!process.env.CERULEAN_WORKBUDDY_DRAFT_APPROVAL||execute,'Approval must match plan');
const report=await mkdtemp(resolve('reports/workbuddy-draft-submit-'));
await writeFile(join(report,'scope.json'),JSON.stringify({plan,planHash,execute},null,2)+'\n');
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
let source=await readFile('crates/core/native/DesktopObserver.cs','utf8');
const field='public StopObservation stopObservation;',route='else if(request.op=="trialprepare")';
assert.equal(source.split(field).length,2);assert.equal(source.split(route).length,2);
source=source.replace(field,field+'public Observation draftObservation;').replace(route,'else if(request.op=="incidentprepare")result=PrepareIncidentDraft(request.taskReview);\n            else if(request.op=="incidentsubmit")result=SubmitIncidentDraft(request.taskReview,request.draftObservation);\n            '+route);
await writeFile(join(report,'Observer.cs'),source);
const exe=join(report,'DraftSubmit.exe');
const compiled=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'Observer.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs'),resolve('tests/fixtures/WorkBuddyDraftProbe.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(compiled.status,0,compiled.stdout+compiled.stderr);
async function run(name,body){
 const r=spawnSync(exe,[JSON.stringify({version:1,...body})],{encoding:'utf8',windowsHide:true,timeout:90000});
 await writeFile(join(report,name+'.stderr.log'),r.stderr??'');await writeFile(join(report,name+'.stdout.log'),r.stdout??'');
 if(r.error)throw Error(name+': '+(r.error.code??'process-error'));
 const response=JSON.parse(r.stdout.replace(/^\uFEFF/,''));await writeFile(join(report,name+'.json'),JSON.stringify(response,null,2)+'\n');
 if(r.status!==0||!response.ok)throw Error(name+': '+response.error);return response.result;
}
let result,clickAttempts=0;
try {
 let rust;
 if(execute){
  const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{encoding:'utf8',windowsHide:true,timeout:120000});assert.equal(build.status,0,build.stderr);
  rust=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(rust);
 }
 const prepared=await run('preflight',{op:'incidentprepare',taskReview:review});
 if(execute){
  // Exclusive durable guard: this exact approved incident may never auto-click twice.
  const receipt=await open(resolve('reports','workbuddy-draft-click-'+planHash+'.json'),'wx');
  try {await receipt.writeFile(JSON.stringify({planHash,report,at:new Date().toISOString(),action:'one-click-attempt'})+'\n');await receipt.sync();}finally{await receipt.close();}
  clickAttempts=1;
  const sent=await run('submit',{op:'incidentsubmit',taskReview:review,draftObservation:prepared.draftObservation});
  await writeFile(join(report,'review.json'),JSON.stringify(review,null,2)+'\n');await writeFile(join(report,'success.json'),JSON.stringify(sent,null,2)+'\n');
  const contract=spawnSync(rust,['--exact','desktop_execution::local_proof_tests::native_workbuddy_reply_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_REPLY_REPORT:report}});
  await writeFile(join(report,'rust-reply-contract.log'),contract.stdout+'\n'+contract.stderr);assert.equal(contract.status,0,contract.stdout+contract.stderr);assert.match(contract.stdout,/1 passed/);
 }
 result={status:execute?'passed':'ready-read-only',planHash,clickAttempts,keyboardInput:false,secondMessage:false,durableProductWorkflowTested:false};
}catch(e){result={status:'failed',planHash,clickAttempts,error:e.message,keyboardInput:false,secondMessage:false,durableProductWorkflowTested:false};throw e;}
finally{await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,...result}));}
