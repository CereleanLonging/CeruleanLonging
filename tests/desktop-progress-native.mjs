// Production UIA bridge against owned windows only; no external application.
import {spawn, spawnSync} from 'node:child_process';
import {mkdtemp, readFile, writeFile, rename, mkdir} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB, '1');
const report = await mkdtemp(resolve('reports/desktop-progress-native-'));
const framework = join(process.env.WINDIR, 'Microsoft.NET/Framework64/v4.0.30319');
const hash = value => createHash('sha256').update(value).digest('hex');
function compile(name, sources, refs) {
  const exe = join(report, name + '.exe');
  const r = spawnSync(join(framework, 'csc.exe'), ['/nologo', '/target:exe', '/platform:x64', '/r:System.Web.Extensions.dll', '/r:System.Xaml.dll', ...refs.map(n => '/r:' + join(framework, 'WPF', n + '.dll')), '/out:' + exe, ...sources.map(resolveSource => resolve(resolveSource))], {encoding:'utf8', windowsHide:true, timeout:30000});
  assert.equal(r.status, 0, r.stdout + r.stderr); return exe;
}
const bridge = compile('Observer', ['crates/core/native/DesktopObserver.cs','crates/core/native/DesktopTrial.cs','crates/core/native/DesktopStop.cs'], ['UIAutomationClient','UIAutomationTypes','WindowsBase']);
const refs = ['UIAutomationTypes','UIAutomationProvider','WindowsBase','PresentationCore','PresentationFramework'];
const workbuddy = compile('WorkBuddy', ['tests/fixtures/WorkBuddyStopWindow.cs'], refs);
const qoder = compile('Qoder', ['tests/fixtures/DesktopStopWindow.cs'], refs);
const bridgeSha = hash(await readFile(bridge));
const marker = 'CL_TASK_0123456789abcdef0123456789abcdef', prompt = 'Owned progress observation ' + marker;
const cases = [], cleanups = [];
function start(exe, args) {
  const child = spawn(exe, args, {windowsHide:true, stdio:['ignore','pipe','pipe']});
  const owner = {child, closed:false, logs:''};
  child.stdout.on('data', b => owner.logs += b); child.stderr.on('data', b => owner.logs += b);
  owner.done = new Promise(done => child.once('close', (code, signal) => {owner.closed=true; done({code,signal});}));
  return owner;
}
async function waitJson(dir, file, owner) {
  const until=Date.now()+10000;
  while(Date.now()<until) {
    try {return JSON.parse(await readFile(join(dir,file),'utf8'));}
    catch(e) {if(!['ENOENT','EBUSY','EACCES'].includes(e.code)&&!(e instanceof SyntaxError))throw e;}
    assert.equal(owner.closed,false,'owned fixture exited: '+owner.logs);
    await new Promise(r=>setTimeout(r,30));
  }
  assert.fail('timeout: '+file);
}
async function finish(owner, dir) {
  if(!owner.closed) owner.child.kill();
  const end = await owner.done;
  await writeFile(join(dir,'fixture.log'),owner.logs);
  cleanups.push({pid:owner.child.pid,closed:owner.closed,...end});
}
async function request(dir, name, body, error) {
  const r=spawnSync(bridge,[JSON.stringify({version:1,...body})],{encoding:'utf8',windowsHide:true,timeout:15000});
  await writeFile(join(dir,name+'.stdout'),r.stdout??''); await writeFile(join(dir,name+'.stderr'),r.stderr??'');
  assert.equal(r.error,undefined,r.error?.message);
  const response=JSON.parse(r.stdout.replace(/^\uFEFF/,''));
  assert.equal(response.ok,!error,name+': '+JSON.stringify(response));
  assert.equal(r.status,error?1:0);
  if(error) assert.equal(response.error,error,name);
  cases.push({name,passed:true,error:error??null}); return response.result;
}
function review(prepared) {
  return {id:'a'.repeat(64),bindingId:'b'.repeat(64),marker,prompt,promptSha256:hash(prompt),bridgeSha256:bridgeSha,observation:prepared.observation,baseline:prepared.baseline,maxWaitSeconds:10};
}
function check(sample, task) {
  assert.equal(sample.schemaVersion,1); assert.equal(sample.taskReviewDigest,task.id);
  assert.deepEqual(sample.observation.window,task.observation.window);
  assert.equal(sample.observation.documentRuntimeId,task.observation.documentRuntimeId);
  assert.equal(sample.observation.sessionId,task.observation.sessionId);
  assert.equal(sample.observation.modelLabel,task.observation.modelLabel);
  assert.equal(sample.observation.draftEmpty,true);
  assert.deepEqual(sample.baseline,task.baseline.slice(-sample.baseline.length));
  assert.ok(sample.baseline.length>=2); assert.match(sample.visibleDigest,/^[a-f0-9]{64}$/);
  assert.notEqual(sample.turnRuntimeId,sample.promptRuntimeId);
  assert.deepEqual(Object.keys(sample).sort(),['schemaVersion','taskReviewDigest','observation','baseline','turnRuntimeId','promptRuntimeId','visibleDigest','statusText'].sort());
}
let result;
try {
  const wbDir=join(report,'workbuddy'); await mkdir(wbDir);
  const owner=start(workbuddy,[wbDir]); let sequence=0;
  async function change(mode) {
    const n=++sequence, name='command-'+n;
    await writeFile(join(wbDir,name+'.tmp'),JSON.stringify({sequence:n,mode,prompt}));
    await rename(join(wbDir,name+'.tmp'),join(wbDir,name+'.json'));
    await waitJson(wbDir,'ack-'+n+'.json',owner);
  }
  try {
    const window=await waitJson(wbDir,'window.json',owner); assert.equal(window.pid,owner.child.pid);
    const task=review(await request(wbDir,'baseline',{op:'taskprepare',window}));
    for(const mode of ['running','clipped-running','completed','failed','missing-status']) {
      await change(mode);
      const {taskProgress:sample}=await request(wbDir,mode,{op:'taskprogress',taskReview:task}); check(sample,task);
      if(mode==='clipped-running')assert.equal(sample.baseline.length,2);
      if(mode==='completed')assert.match(sample.statusText,/^已完成/);
    }
    for(const [mode,error] of [['history-changed','stop_history_changed'],['prompt-changed','stop_turn_changed'],['wrong-role','stop_turn_changed'],['model-changed','stop_identity_changed'],['draft','stop_identity_changed'],['document-changed','trial_view_changed'],['ambiguous-status','stop_turn_changed'],['clipped-too-short','stop_history_changed']]) {
      await change(mode); await request(wbDir,mode,{op:'taskprogress',taskReview:task},error);
    }
    assert.deepEqual(await waitJson(wbDir,'mutations.json',owner),{valueChanges:0,invokes:0});
    await change('shutdown');
  } finally {await finish(owner,wbDir);}
  for(const [mode,error] of [['success',null],['clipped-history',null],['foreign-prompt','stop_turn_changed'],['quoted-prompt','stop_turn_changed'],['changed-model','stop_identity_changed'],['changed-session','trial_view_changed'],['draft','stop_identity_changed']]) {
    const dir=join(report,'qoder-'+mode); await mkdir(dir);
    const owner=start(qoder,[dir,mode,String(process.pid)]);
    try {
      const window=await waitJson(dir,'window.json',owner); assert.equal(window.pid,owner.child.pid);
      const task=review(await request(dir,'baseline',{op:'taskprepare',window}));
      await writeFile(join(dir,'start.json'),JSON.stringify(prompt)); await waitJson(dir,'started.json',owner);
      const observed=await request(dir,mode,{op:'taskprogress',taskReview:task},error);
      if(!error)check(observed.taskProgress,task);
      let stops=0;try{stops=JSON.parse(await readFile(join(dir,'stop-count.json'),'utf8'));}catch(e){if(e.code!=='ENOENT')throw e;}
      assert.equal(stops,0);
    } finally {await finish(owner,dir);}
  }
  result={status:'passed',realUia:true,externalApplicationsUsed:false,modelCalls:0,messagesSent:0,invokes:0,valueChanges:0,cases,cleanups};
} catch(e) {result={status:'failed',error:e.stack,cases,cleanups}; process.exitCode=1;}
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({report,status:result.status,cases:cases.length,error:result.error}));
