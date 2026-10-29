// Inspect installed Qoder code, executing only isolated methods with fake services.
import {readFile, mkdir, writeFile} from 'node:fs/promises';
import {resolve, relative, isAbsolute, join} from 'node:path';
import {createHash} from 'node:crypto';
import vm from 'node:vm';
import assert from 'node:assert/strict';

const source=resolve(process.argv[2]??''), report=resolve(process.argv[3]??'');
assert.equal(source,resolve('E:/千问/Qoder/.qoder-versions/0.4.3/resources/app.asar'));
const rel=relative(resolve('reports'),report);
assert.ok(rel && !rel.startsWith('..') && !isAbsolute(rel));
await mkdir(report,{recursive:false});
const asar=await readFile(source), hash=b=>createHash('sha256').update(b).digest('hex');
const headerSize=asar.readUInt32LE(12), header=JSON.parse(asar.subarray(16,16+headerSize).toString('utf8'));
const entry=header.files.out.files.main.files['index.js'];
assert.ok(!entry.unpacked && Number.isSafeInteger(entry.size));
const offset=8+asar.readUInt32LE(4)+Number(entry.offset);
assert.ok(offset>16+headerSize && offset+entry.size<=asar.length);
const bytes=asar.subarray(offset,offset+entry.size), text=bytes.toString('utf8');
const extracts=[];
function extract(start,end) {
  const a=text.indexOf(start),b=text.indexOf(end,a+start.length);
  assert.ok(a>=0 && b>a && b-a<10000 && text.indexOf(start,a+1)<0,start);
  const code=text.slice(a,b);
  extracts.push({start,end,characterStart:a,characterEnd:b,sha256:hash(code)});
  return code;
}
const steer=extract('async steerActiveTurn(e,r){','resolveSteerFenceFallback(e){');
const fence=extract('resolveSteerFenceFallback(e){','toAdmissionFallbackReason(e){');
const normalize=extract('function BJr(t){','function wJr(t){');
const valid=extract('function ZJ(t){','function tse(t){');
const id=extract('function ok(t){','function ZJ(t){');
const route=extract('function mDt(t){','const fDt=');
const sandbox={De:()=> 'synthetic-id',d1:id=>'synthetic-request-set-'+id};
const context=vm.createContext(sandbox,{codeGeneration:{strings:false,wasm:false}});
const functions=new vm.Script(`(()=>{${normalize}${valid}${id}${route}return {normalize:BJr,route:mDt,steer:({${steer}}).steerActiveTurn,fence:({${fence}}).resolveSteerFenceFallback};})()`)
  .runInContext(context,{timeout:1000});
const plain=x=>JSON.parse(JSON.stringify(x));
const cases=[];
const opts={inputId:'advice',turnId:'queued-advice',dispatchMode:'steer'};
function setup() {
  const effects={starts:[],queues:[],commits:[]};
  const host={activeTurn:{turnId:'original-turn',state:'running'},steerReservations:0,
    options:{sessionId:'original-session'},
    hasPendingInteractionForActiveTurn:()=>false, clearIdleTimer(){},
    startTurn:async(prompt,options)=>effects.starts.push({prompt,options}),
    queueTurn:(prompt,options,reason)=>effects.queues.push({prompt,options,reason}),
    enqueueDelivery:fn=>fn(),prepareSteerDelivery:async()=> 'ready',
    releaseSteerReservation:async()=>{host.steerReservations--;},
    adoptTurnPermissionControls(){},createTurnLifecycle:()=>({}),emitSteerUserMessage(){},
    commitSteerDelivery:async(record,emit,payload)=>{effects.commits.push({owner:host.activeTurn?.turnId,record,payload});emit();},
  };
  return {host,effects};
}
function pass(name,x,result,claim) {cases.push({name,passed:true,result:plain(result),effects:x?.effects,claim});}
let result;
try {
  {
    const x=setup();x.host.activeTurn=undefined;
    const r=await functions.steer.call(x.host,'advice',opts);
    assert.equal(r.disposition,'started');assert.equal(x.effects.starts.length,1);
    pass('idle-steer-starts-new-turn',x,r,'Steer method is not an original-turn-only endpoint.');
  }
  {
    const x=setup();x.host.activeTurn.state='waiting-user';
    const r=await functions.steer.call(x.host,'advice',opts);
    assert.equal(r.disposition,'queued');assert.equal(x.effects.queues.length,1);
    pass('waiting-user-queues-advice',x,r,'Queued is not consumed by the original turn.');
  }
  for (const status of ['waiting-user','active-turn-ended','control-command']) {
    const x=setup();x.host.prepareSteerDelivery=async()=>status;
    const r=await functions.steer.call(x.host,'advice',opts);
    assert.equal(r.disposition,'queued');assert.equal(x.effects.commits.length,0);assert.equal(x.effects.queues.length,1);
    pass('preparation-'+status+'-queues-advice',x,r,'Read-only preflight does not remove this queue fallback.');
  }
  {
    const x=setup(),r=await functions.steer.call(x.host,'advice',opts);
    assert.deepEqual(plain(r),{inputId:'advice',disposition:'steered',activeTurnId:'original-turn'});
    assert.equal(x.effects.commits.length,1);assert.equal(x.effects.starts.length,0);assert.equal(x.effects.queues.length,0);
    pass('ready-original-turn-selects-steer',x,r,'Commit dependency is synthetic; this does not prove actual runtime consumption.');
  }
  for (const [name,generation,owner,expected] of [
    ['matching-fence',3,'original-turn',undefined],
    ['runtime-generation-changed',4,'original-turn','stale-active-turn'],
    ['active-turn-changed',3,'replacement-turn','stale-active-turn'],
  ]) {
    const r=functions.fence.call({activeTurn:{turnId:owner}},
      {dispatchMode:'steer',steerTarget:{turnId:'original-turn',runtimeGeneration:3},runtimeGeneration:generation});
    assert.equal(r,expected);pass(name,null,{fallback:r??null},'Fence diagnoses a fallback; not a public no-fallback delivery guarantee.');
  }
  {
    const r=functions.normalize({sessionId:'original-session',prompt:'advice',requestId:'advice',turnId:'original-turn',expectedTurnId:'original-turn'});
    assert.deepEqual(plain(r),{sessionId:'original-session',prompt:'advice',requestId:'advice'});
    pass('plugin-continue-does-not-forward-turn-target',null,r,'Adding an undeclared target field cannot bind plugin continuation to the original turn.');
  }
  {
    const r=functions.route({requestedMode:'steer',activeTurnState:null,supportsSteer:true});
    assert.deepEqual(plain(r),{action:'start',reason:'runtime-idle'});
    pass('steer-routing-when-idle-selects-start',null,r,'Requested steer alone does not prohibit a new task.');
  }
  const evidence=[];
  for(const term of ['continueSession(e,r){const i=BJr(r)','async continueSessionFromPlugin(t,e)','async observeTurn(e,r)','async commitSteerDelivery(e,r,i)']) {
    const at=text.indexOf(term);assert.ok(at>=0);
    evidence.push({term,characterOffset:at,excerpt:text.slice(at,at+1500)});
  }
  result={status:'passed',scope:'Installed Qoder routing, plugin argument parser and steer method with synthetic runtime dependencies',
    source,sourceSha256:hash(asar),member:'out/main/index.js',memberSha256:hash(bytes),extracts,cases,evidence,
    conclusion:{ordinarySteerCanStartOrQueue:true,pluginContinuePinsOriginalTurn:false,
      existingDesktopStrictSteerTransportVerified:false,desktopSupervisionAccepted:false},
    realModelCalls:0,realDesktopSends:0,applicationStarted:false,applicationConnected:false,credentialRead:false};
}catch(error){result={status:'failed',source,sourceSha256:hash(asar),extracts,cases,error:error.stack};process.exitCode=1;}
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({status:result.status,cases:cases.length,report}));
