// Read shipped application code; execute only its dependency-free queue class
// with synthetic hosts in an isolated VM. Never start or connect to WorkBuddy.
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {resolve, join} from 'node:path';
import {createHash} from 'node:crypto';
import vm from 'node:vm';
import assert from 'node:assert/strict';

const source = resolve(process.argv[2] ?? '');
const report = resolve(process.argv[3] ?? '');
assert.equal(source, resolve('D:/workbuddy/resources/app.asar.unpacked/renderer/assets/contexts-BMeWSUHk.js'));
assert.ok(report.startsWith(resolve('reports') + '\\'));
await mkdir(report, {recursive:false});
const bytes = await readFile(source), text = bytes.toString('utf8');
const hash = value => createHash('sha256').update(value).digest('hex');
const start = text.indexOf('QueueManagerImpl=class{');
const end = text.indexOf('}}));function remoteUrlFields(', start);
assert.ok(start >= 0 && end > start);
const expression = text.slice(start + 'QueueManagerImpl='.length, end + 1);
assert.ok(expression.length < 16000);
const context = vm.createContext(Object.create(null), {codeGeneration:{strings:false,wasm:false}});
const Queue = new vm.Script('(' + expression + ')').runInContext(context, {timeout:1000});
const cases = [];
const item = (id='advice', value='Keep original approved task scope') => ({id,content:[{type:'text',text:value}]});
function setup(overrides={}) {
  const state={busy:true,owner:'original',starts:[],steers:[]};
  const host={
    isBusy:()=>state.busy,
    dispatchQueued:(value,meta)=>state.starts.push({id:value.id,meta}),
    steer:async(value,expected)=>{state.steers.push({id:value.id,expected:expected??null,owner:state.owner});return {steered:true,ownerRequestId:state.owner};},
    ...overrides,
  };
  const queue = new Queue(); queue.attachHost(host); assert.equal(queue.enqueue(item()),true);
  return {state,host,queue};
}
function record(name, x, outcome, claim) {
  cases.push({name,passed:true,outcome,starts:x.state.starts,steers:x.state.steers,remaining:x.queue.items().map(v=>v.id),claim});
}
let result;
try {
  {
    const x=setup(), outcome=await x.queue.sendNow('advice');
    assert.equal(outcome,'steered');assert.equal(x.state.starts.length,0);
    assert.deepEqual(x.state.steers,[{id:'advice',expected:null,owner:'original'}]);
    record('busy-steer-first-call-has-no-expected-request',x,outcome,'Ordinary first queue send does not pass an expected original request ID.');
  }
  {
    const x=setup();x.state.busy=false;const outcome=await x.queue.sendNow('advice');
    assert.equal(outcome,'started');assert.equal(x.state.starts.length,1);assert.equal(x.state.steers.length,0);
    record('idle-send-now-starts-new-prompt',x,outcome,'A queue send is not an original-task-only steer operation.');
  }
  {
    const x=setup();x.host.waitForQueuedPromptReady=async()=>{x.state.owner='replacement';};
    const outcome=await x.queue.sendNow('advice');
    assert.equal(outcome,'steered');assert.equal(x.state.steers[0].owner,'replacement');assert.equal(x.state.steers[0].expected,null);
    record('owner-changes-during-readiness-wait',x,outcome,'A UI preflight of the old owner cannot bind the later first steer.');
  }
  {
    const x=setup({steer:undefined});const outcome=await x.queue.sendNow('advice');
    assert.equal(outcome,'queued');x.state.busy=false;x.queue.drainNext();assert.equal(x.state.starts.length,1);
    record('unsupported-steer-falls-back-to-later-prompt',x,outcome,'Queue presence does not prove advice reached the original task.');
  }
  for (const reason of ['idle','stale']) {
    const x=setup({steer:async()=>({steered:false,reason})});const outcome=await x.queue.sendNow('advice');
    assert.equal(outcome,'queued');x.state.busy=false;assert.equal(x.queue.drainNext(),undefined);assert.equal(x.state.starts.length,0);
    record('explicit-'+reason+'-fallback-is-held',x,outcome,'These explicit refusal branches retain the item without automatic draining.');
  }
  {
    const x=setup({steer:async()=>{throw Error('synthetic uncertain response');}});const outcome=await x.queue.sendNow('advice');
    assert.equal(outcome,'queued');x.state.busy=false;x.queue.drainNext();assert.equal(x.state.starts.length,1);
    record('unknown-steer-response-can-drain-as-new-prompt',x,outcome,'Uncertain delivery is not an at-most-once original-task receipt.');
  }
  {
    const x=setup();await x.queue.delete('advice');x.queue.enqueue(item('command','/compact'));
    const outcome=await x.queue.sendNow('command');assert.equal(outcome,'queued');assert.equal(x.state.steers.length,0);
    x.state.busy=false;x.queue.drainNext();assert.equal(x.state.starts.length,1);
    record('control-command-is-a-queued-prompt',x,outcome,'Control text does not use the advice injection branch.');
  }
  {
    const x=setup();x.queue.enqueue(item('advice-two'));
    const outcomes=await Promise.all([x.queue.sendNow('advice'),x.queue.sendNow('advice-two')]);
    assert.equal(outcomes.join(','),'steered,steered');assert.equal(x.state.steers[0].expected,null);assert.equal(x.state.steers[1].expected,'original');
    record('concurrent-follow-up-inherits-first-returned-owner',x,outcomes,'Follow-up owner pinning exists; it does not bind the first send to our approved task.');
  }
  const evidenceTerms=['async steerQueued(e,t)','expectedRequestId:t','clientUserMessageId:e.id','status:`pending`','function readInjectedSteerItemId(e)','status===`injected`'];
  const evidence=evidenceTerms.map(term=>{const at=text.indexOf(term);assert.ok(at>=0,term);return {term,characterOffset:at,excerpt:text.slice(Math.max(0,at-100),at+term.length+320)};});
  result={status:'passed',scope:'Shipped queue-class behavior with synthetic hosts, not desktop integration acceptance',source,sourceSha256:hash(bytes),queueClass:{characterStart:start,characterEnd:end+1,sha256:hash(expression)},cases,evidence,
    conclusion:{queueSendNowSafeForStrictOriginalTaskAdvice:false,firstSendExpectedOwnerMissing:true,idleCanStartNewPrompt:true,uncertainCanStartNewPrompt:true,explicitIdleAndStaleHold:true,steeredAckIsNotInjectedReceipt:true},
    actualApplicationStarted:false,actualApplicationConnected:false,realModelCalls:0,realDesktopSends:0,credentialRead:false,productionFilesModified:false};
} catch(error) {result={status:'failed',source,sourceSha256:hash(bytes),cases,error:error.stack};process.exitCode=1;}
assert.equal(hash(await readFile(source)),hash(bytes),'Installed source changed during audit');
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({report,status:result.status,cases:cases.length,conclusion:result.conclusion,error:result.error}));
