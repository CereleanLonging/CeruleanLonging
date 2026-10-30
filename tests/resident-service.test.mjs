import test from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createConnection } from "node:net";
import { mkdtemp, mkdir, writeFile, readFile, rename } from "node:fs/promises";
import { resolve, join } from "node:path";
const binary=resolve("target/debug/cerulean-core.exe");
async function root() {await mkdir("reports",{recursive:true});return mkdtemp(resolve("reports/resident-"));}
function start(dir) {const child=spawn(binary,["--serve","--data-dir",dir],{windowsHide:true,stdio:["ignore","ignore","pipe"]});let errors="";child.stderr.on("data",b=>errors+=b);return {child,errors:()=>errors};}
function endpoint(dir) {const r=spawnSync(binary,["--endpoint","--data-dir",dir],{encoding:"utf8",windowsHide:true});assert.equal(r.status,0,r.stderr);return r.stdout.trim();}
async function connection(name) {
  const socket=createConnection(name);await once(socket,"connect");
  const iterator=socket[Symbol.asyncIterator]();let buffer=Buffer.alloc(0);
  async function read() {
    while(buffer.length<4 || buffer.length<4+buffer.readUInt32LE(0)) {
      const item=await iterator.next();if(item.done)throw Error("closed frame");buffer=Buffer.concat([buffer,item.value]);
      if(buffer.length>=4 && buffer.readUInt32LE(0)>4*1024*1024)throw Error("oversize response");
    }
    const size=buffer.readUInt32LE(0);const result=JSON.parse(buffer.subarray(4,size+4));buffer=buffer.subarray(size+4);return result;
  }
  function send(value) {const bytes=Buffer.from(JSON.stringify(value));const header=Buffer.alloc(4);header.writeUInt32LE(bytes.length);socket.write(Buffer.concat([header,bytes]));}
  const greeting=await read();assert.equal(greeting.product,"CeruleanLonging");assert.equal(greeting.protocolVersion,1);
  return {socket,greeting,read,send};
}
async function ready(server,name) {
  for(let i=0;i<80;i++) {assert.equal(server.child.exitCode,null,server.errors());try{return await connection(name);}catch(e){if(!["ENOENT","EBUSY"].includes(e.code))throw e;await new Promise(r=>setTimeout(r,50));}}
  throw Error("service startup deadline");
}
async function request(name,id,command,protocolVersion=1) {const c=await connection(name);try{c.send({type:"request",request:{id,protocolVersion,command}});const reply=await c.read();assert.equal(reply.response.id,id);return {reply,pid:c.greeting.serverPid};}finally{c.socket.destroy();}}
async function stop(server,name) {
  if(server.child.exitCode!==null)return;
  const exit=once(server.child,"exit");
  try {const c=await connection(name);c.send({type:"shutdown",id:"stop",protocolVersion:1});const reply=await c.read();c.socket.destroy();assert.equal(reply.stopping,true);await exit;}
  finally {if(server.child.exitCode===null)server.child.kill();}
}

test("resident survives disconnects, shares client state, rejects duplicate service and replays on restart",{timeout:30000},async()=>{
  const dir=await root(),name=endpoint(dir);let server=start(dir);
  try {
    const first=await ready(server,name);assert.equal(first.greeting.serverPid,server.child.pid);first.socket.destroy();
    const create=await request(name,"create",{type:"createPlan",title:"Resident fixture",tasks:["Persist and reconnect"]});
    const plan=create.reply.response.result.plans[0];
    const approved=await request(name,"approve",{type:"approvePlan",id:plan.id,digest:plan.digest});assert.equal(approved.reply.response.result.plans[0].approved,true);
    assert.equal(create.pid,approved.pid);
    const duplicate=start(dir);const [exitCode]=await once(duplicate.child,"exit");assert.notEqual(exitCode,0);assert.match(duplicate.errors(),/active|lock|writer|另一个/i);
    const cli=spawn(binary,["--request","--data-dir",dir],{windowsHide:true,stdio:["pipe","pipe","pipe"]});
    let output="",cliError="";cli.stdout.on("data",b=>output+=b);cli.stderr.on("data",b=>cliError+=b);const cliExit=once(cli,"close");cli.stdin.end(JSON.stringify({id:"cli",protocolVersion:1,command:{type:"snapshot"}}));assert.equal((await cliExit)[0],0,cliError);
    assert.equal(JSON.parse(output).response.result.plans[0].approved,true);
    await stop(server,name);assert.equal(server.child.exitCode,0);
    server=start(dir);const reopened=await ready(server,name);assert.notEqual(reopened.greeting.serverPid,create.pid);reopened.socket.destroy();
    const snapshot=await request(name,"reopened",{type:"snapshot"});assert.equal(snapshot.reply.response.result.events.length,2);
    await writeFile(join(dir,"review.json"),JSON.stringify({status:"passed",sameServiceAcrossClients:true,cliSharedState:true,duplicateRejected:true,restartReplayed:true,modelCalls:0},null,2));
  } finally {await stop(server,name);}
});

test("malformed, oversized, stale protocol and role injection requests cannot mutate; slow client does not block others",{timeout:30000},async()=>{
  const dir=await root(),name=endpoint(dir),server=start(dir);let slow;
  try {
    const initial=await ready(server,name);initial.socket.destroy();
    const wrong=await request(name,"wrong",{type:"createPlan",title:"deny",tasks:["deny"]},999);assert.equal(wrong.reply.response.error.code,"protocolMismatch");
    for(const input of ["oversize","role"]) {
      const c=await connection(name);
      if(input==="oversize"){const header=Buffer.alloc(4);header.writeUInt32LE(65537);c.socket.write(header);}
      else c.send({type:"request",principal:"chief",request:{id:"forged",protocolVersion:1,command:{type:"createPlan",title:"deny",tasks:["deny"]}}});
      await assert.rejects(c.read());c.socket.destroy();
    }
    slow=await connection(name);const header=Buffer.alloc(4);header.writeUInt32LE(1000);slow.socket.write(header);
    const snapshot=await request(name,"parallel",{type:"snapshot"});assert.equal(snapshot.reply.response.result.events.length,0);slow.socket.destroy();
    await writeFile(join(dir,"review.json"),JSON.stringify({status:"passed",staleProtocolRejected:true,oversizeRejected:true,roleInjectionRejected:true,slowClientIsolated:true,events:0,modelCalls:0},null,2));
  } finally {slow?.socket.destroy();await stop(server,name);}
});

test("legacy clients reject malformed and oversized requests and stop the original service",{timeout:30000},async()=>{
  const dir=await root(),name=endpoint(dir),server=start(dir);
  async function cli(mode,input="") {
    const child=spawn(binary,[mode,"--data-dir",dir],{windowsHide:true,stdio:["pipe","pipe","pipe"]});
    let output="",errors="";
    child.stdout.on("data",b=>output+=b);
    child.stderr.on("data",b=>errors+=b);
    const closed=once(child,"close");child.stdin.end(input);
    return {code:(await closed)[0],output,errors};
  }
  try {
    const first=await ready(server,name);first.socket.destroy();
    for(const input of ["{", "x".repeat(65537)]) {
      const result=await cli("--request",input);
      assert.equal(result.code,1,result.errors);
      assert.equal(result.output,"");
    }
    const rejected=await cli("--request",JSON.stringify({id:"stale-cli",protocolVersion:999,command:{type:"snapshot"}}));
    assert.equal(rejected.code,1,rejected.errors);
    assert.ok(JSON.parse(rejected.output).response.error);
    const state=await request(name,"unchanged",{type:"snapshot"});
    assert.equal(state.pid,server.child.pid);assert.equal(state.reply.response.result.events.length,0);
    const stopped=once(server.child,"exit"),result=await cli("--stop");
    assert.equal(result.code,0,result.errors);assert.equal(JSON.parse(result.output).stopping,true);
    assert.equal((await stopped)[0],0,server.errors());
  } finally {await stop(server,name);}
});

test("damaged startup serves locked read-only diagnostics and explicit recheck preserves evidence",{timeout:30000},async()=>{
  const dir=await root(),name=endpoint(dir);await mkdir(join(dir,'omp-startups/project'),{recursive:true});
  const damaged=join(dir,'omp-startups/project/attempt.0.pending'),bytes=Buffer.from('{"schemaVersion":');await writeFile(damaged,bytes);
  let server=start(dir);
  async function recheck(){const c=await connection(name),exit=once(server.child,'exit');c.send({type:'restartDataDiagnostics',id:'recheck',protocolVersion:1});const reply=await c.read();c.socket.destroy();assert.equal(reply.stopping,true);assert.equal((await exit)[0],0);server=start(dir);const readyClient=await ready(server,name);readyClient.socket.destroy();}
  try{
    const c=await ready(server,name);c.socket.destroy();
    let state=(await request(name,'diagnostic',{type:'snapshot'})).reply.response.result;
    assert.equal(state.coreDiagnostic.startupFiles[0].state,'truncated-json');assert.equal(state.workflows.length,0);
    for(const command of [{type:'createPlan',title:'denied',tasks:['denied']},{type:'checkOpenAi'}])assert.equal((await request(name,'deny',command)).reply.response.error.code,'diagnosticReadOnly');
    const duplicate=start(dir);assert.notEqual((await once(duplicate.child,'exit'))[0],0);
    const wrong=await connection(name);wrong.send({type:'restartReviewedStops',id:'wrong',protocolVersion:1});assert.equal((await wrong.read()).response.error.code,'restartRejected');wrong.socket.destroy();
    await recheck();assert.deepEqual(await readFile(damaged),bytes);assert.ok((await request(name,'still-diagnostic',{type:'snapshot'})).reply.response.result.coreDiagnostic);
    // Test-only restore to the known empty baseline. Keep the injected bytes
    // outside the journal; the product does not implement this repair action.
    const preserved=join(dir,'injected-startup-original.bin');await rename(damaged,preserved);
    await recheck();state=(await request(name,'healthy',{type:'snapshot'})).reply.response.result;assert.equal(state.coreDiagnostic,null);assert.deepEqual(await readFile(preserved),bytes);
    const reject=await connection(name);reject.send({type:'restartDataDiagnostics',id:'healthy-reject',protocolVersion:1});const response=await reject.read();assert.equal(response.stopping,false);assert.equal(response.response.error.code,'restartRejected');reject.socket.destroy();
    await writeFile(join(dir,'review.json'),JSON.stringify({status:'passed',diagnosticLockHeld:true,mutationsDenied:true,explicitRechecks:2,originalBytesPreserved:true,restoration:'test-only known empty baseline',modelCalls:0},null,2));
  } finally {await stop(server,name);}
});
