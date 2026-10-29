// Execute only the installed steer handler and buffer with synthetic services.
// No application launch, socket, runtime import, credential access or model call.
import {readFile, mkdir, writeFile} from 'node:fs/promises';
import {resolve, relative, join, isAbsolute} from 'node:path';
import {createHash} from 'node:crypto';
import vm from 'node:vm';
import assert from 'node:assert/strict';

const source = resolve(process.argv[2] ?? '');
const report = resolve(process.argv[3] ?? '');
assert.equal(source, resolve('D:/workbuddy/resources/app.asar.unpacked/cli/dist/codebuddy-lite-wb.mjs'));
const reportRelative = relative(resolve('reports'), report);
assert.ok(reportRelative && !reportRelative.startsWith('..') && !isAbsolute(reportRelative));
await mkdir(report, {recursive: false});
const bytes = await readFile(source), text = bytes.toString('utf8');
const hash = value => createHash('sha256').update(value).digest('hex');
function extract(start, end, prefix='') {
  const at = text.indexOf(start);
  assert.ok(at >= 0 && text.indexOf(start, at + 1) < 0, `Unique start: ${start}`);
  const until = text.indexOf(end, at + start.length);
  assert.ok(until > at && until - at < 10000, `Bounded end: ${end}`);
  const value = text.slice(at + prefix.length, until);
  return {value, characterStart: at + prefix.length, characterEnd: until, sha256: hash(value)};
}
const handler = extract('async handleSteer(t,i){', 'async handleInjectHistory(');
const buffer = extract('let ab=class{constructor(){this.store=new Map}', ';ab=steer_input_buffer_decorate', 'let ab=');
const cases = [];
let state;
const tokens = {machine: Symbol('machine'), buffer: Symbol('buffer')};
const sandbox = {
  structuredClone,
  SK: {GI: {invalidParams: message => new Error(message)}},
  g9: {zys: blocks => blocks.every(b => b.type === 'text'), MU8: tokens.machine,
    wX5: tokens.buffer, 'n$D': status => status === 'running'},
  g7: {ContainerUtil: {get: token => token === tokens.machine
    ? {getSnapshot: () => ({status: state.status})} : state.buffer}},
  acpPromptToUserInputItems: ({prompt}) => prompt,
  prepareSteerMessages: items => items,
  acp_agent_isRecord: value => value !== null && typeof value === 'object' && !Array.isArray(value),
  Ei: 'synthetic-reserved-meta',
};
const context = vm.createContext(sandbox, {codeGeneration: {strings: false, wasm: false}});
const BufferClass = new vm.Script(`(${buffer.value})`).runInContext(context, {timeout: 1000});
const method = new vm.Script(`({${handler.value}}).handleSteer`).runInContext(context, {timeout: 1000});
const request = extra => ({sessionId: 'original-session', expectedRequestId: 'original-request',
  clientUserMessageId: 'advice-1', contentBlocks: [{type:'text',text:'Keep the approved scope'}], ...extra});
function setup() {
  state = {session: {conversationRequestId:'original-request'}, status:'running', buffer:new BufferClass()};
  const host = {getRuntimeSession: async id => id === 'original-session' ? state.session : undefined,
    logger:{info(){}}, consumeModPromptReceipt: () => false};
  return host;
}
const plain = value => JSON.parse(JSON.stringify(value));
function pass(name, details={}) { cases.push({name, passed:true, ...details}); }
let result;
try {
  {
    const host=setup(), ack=await method.call(host,request());
    assert.deepEqual(plain(ack),{steered:true,ownerRequestId:'original-request'});
    assert.equal(state.buffer.has('original-session','original-request'),true);
    pass('matching-owner-is-buffered', {ack:plain(ack), injectedReceiptProven:false});
  }
  {
    const host=setup(); state.session.conversationRequestId='replacement-request';
    assert.deepEqual(plain(await method.call(host,request())),{steered:false,reason:'stale'});
    assert.equal(state.buffer.has('original-session','replacement-request'),false);
    pass('mismatched-expected-owner-is-refused');
  }
  {
    const host=setup();host.getRuntimeSession=async()=>{await Promise.resolve();state.session.conversationRequestId='replacement-request';return state.session;};
    assert.deepEqual(plain(await method.call(host,request())),{steered:false,reason:'stale'});
    pass('owner-change-during-session-resolution-is-refused');
  }
  for (const missingOwner of [false,true]) {
    const host=setup();state.status='idle';if(missingOwner)delete state.session.conversationRequestId;
    assert.deepEqual(plain(await method.call(host,request())),{steered:false,reason:'idle'});
    assert.equal(state.buffer.store.size,0);
    pass(missingOwner?'missing-owner-is-refused':'idle-session-is-refused');
  }
  {
    const host=setup();state.session.conversationRequestId='replacement-request';
    const ack=await method.call(host,request({expectedRequestId:undefined}));
    assert.equal(ack.ownerRequestId,'replacement-request');
    pass('omitting-expected-owner-targets-current-request', {safeForApprovedOriginal:false});
  }
  {
    const host=setup();await assert.rejects(method.call(host,request({clientUserMessageId:' '})),/non-empty/);
    assert.equal(state.buffer.store.size,0);pass('invalid-client-message-id-is-refused');
  }
  {
    const host=setup();await assert.rejects(method.call(host,request({sessionId:'missing'})),/No active session/);
    assert.equal(state.buffer.store.size,0);pass('unknown-session-is-refused');
  }
  {
    const host=setup();await method.call(host,request());await method.call(host,request());
    assert.equal(state.buffer.drain('original-session','original-request').length,2);
    pass('same-client-message-id-does-not-deduplicate', {automaticRetrySafe:false});
  }
  {
    const host=setup();await method.call(host,request());
    assert.equal(state.buffer.drain('original-session','replacement-request').length,0);
    assert.equal(state.buffer.has('original-session','original-request'),false);
    pass('different-owner-drain-discards-old-buffer', {bufferedDoesNotMeanConsumed:true});
  }
  {
    const host=setup();await method.call(host,request());
    const drained=state.buffer.drain('original-session','original-request');
    assert.equal(drained.length,1);assert.equal(drained[0].clientUserMessageId,'advice-1');
    assert.equal(state.buffer.drain('original-session','original-request').length,0);
    pass('matching-owner-drains-once-and-preserves-client-id');
  }
  {
    const host=setup();await method.call(host,request());state.buffer.clear('original-session');
    assert.equal(state.buffer.drain('original-session','original-request').length,0);
    pass('cleared-buffer-is-not-a-consumption-receipt');
  }
  result={status:'passed',source,sourceSha256:hash(bytes),cases,
    extraction:{handler:{...handler,value:undefined},buffer:{...buffer,value:undefined}},
    scope:'Installed handler and buffer with synthetic session, state-machine and content conversion dependencies; not transport or desktop acceptance',
    conclusion:{expectedRequestIdEnforced:true,clientMessageIdDeduplicated:false,bufferedAckProvesConsumption:false,
      nativeSteerPromising:true,existingDesktopTransportVerified:false,desktopSupervisionAccepted:false},
    realModelCalls:0,realDesktopSends:0,applicationStarted:false,credentialRead:false};
} catch(error) {
  result={status:'failed',source,sourceSha256:hash(bytes),cases,error:error.stack};process.exitCode=1;
}
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({status:result.status,caseCount:cases.length,report}));
