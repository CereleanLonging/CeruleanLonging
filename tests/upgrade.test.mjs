import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {createConnection} from 'node:net';
import {mkdtemp,mkdir,readFile,readdir,rename,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const binary=resolve('target/debug/cerulean-core.exe');
const root=await mkdtemp(resolve('reports/upgrade-native-'));
const data=join(root,'核心 数据'),project=join(root,'project'),archive=join(root,'backup');
await mkdir(data);await mkdir(project);
const calls=[];
function cli(args,expected=0){
  const r=spawnSync(binary,['--workbench','--data-dir',data,...args],{windowsHide:true,encoding:'utf8',timeout:45000,maxBuffer:5*1024*1024});
  assert.ok(!r.error,r.error?.code);assert.equal(r.status,expected,r.stderr);
  const response=JSON.parse(expected===0?r.stdout:r.stderr);assert.equal(response.ok,expected===0);
  calls.push({command:args.slice(0,2).join(' '),exit:r.status});return response;
}
async function connection(name){
  const socket=createConnection(name);await once(socket,'connect');
  const iterator=socket[Symbol.asyncIterator]();let buffer=Buffer.alloc(0);
  async function read(){
    while(buffer.length<4||buffer.length<4+buffer.readUInt32LE(0)){
      const next=await iterator.next();if(next.done)throw Error('closed frame');buffer=Buffer.concat([buffer,next.value]);
      if(buffer.length>=4)assert.ok(buffer.readUInt32LE(0)<=4*1024*1024);
    }
    const size=buffer.readUInt32LE(0),value=JSON.parse(buffer.subarray(4,size+4));buffer=buffer.subarray(size+4);return value;
  }
  function send(value){const bytes=Buffer.from(JSON.stringify(value)),header=Buffer.alloc(4);header.writeUInt32LE(bytes.length);socket.write(Buffer.concat([header,bytes]));}
  const greeting=await read();assert.equal(greeting.product,'CeruleanLonging');
  return {socket,read,send,greeting};
}
async function hashes(folder,prefix=''){
  const result={};for(const e of await readdir(join(folder,prefix),{withFileTypes:true})){
    const name=join(prefix,e.name);if(e.isDirectory())Object.assign(result,await hashes(folder,name));
    else result[name]=createHash('sha256').update(await readFile(join(folder,name))).digest('hex');
  }return result;
}
let child,late,stopper,started=false;
try{
  const endpoint=spawnSync(binary,['--endpoint','--data-dir',data],{encoding:'utf8',windowsHide:true});assert.equal(endpoint.status,0);
  child=spawn(binary,['--serve','--data-dir',data],{windowsHide:true,stdio:'ignore'});
  for(let n=0;n<100;n++){
    assert.equal(child.exitCode,null);
    try{late=await connection(endpoint.stdout.trim());break;}catch(e){if(!['ENOENT','EBUSY'].includes(e.code))throw e;await new Promise(r=>setTimeout(r,50));}
  }
  assert.ok(late);stopper=await connection(endpoint.stdout.trim());
  const exit=once(child,'exit');
  stopper.send({type:'shutdown',id:'stop',protocolVersion:1});
  const stopped=await stopper.read();assert.equal(stopped.stopping,true);assert.equal(stopped.response.error,null);
  late.send({type:'request',request:{id:'late',protocolVersion:1,command:{type:'registerProject',project:{id:'late',root:project,timezone:'Asia/Shanghai'}}}});
  const denied=await late.read();assert.equal(denied.response.error.code,'coreStopping');
  stopper.socket.destroy();late.socket.destroy();assert.equal((await exit)[0],0);child=null;
  started=true;cli(['service','start']);
  assert.equal(cli(['status']).result.workflows.length,0);
  const old=cli(['upgrade','review']).upgradeReview;assert.equal(old.ready,true);
  cli(['project','add','--id','project','--root',project,'--timezone','Asia/Shanghai']);
  cli(['upgrade','prepare','--review-digest',old.digest],3);
  const reviewed=cli(['upgrade','review']);assert.equal(reviewed.upgradeReview.ready,true);
  assert.equal(reviewed.upgradeReview.coreSha256,createHash('sha256').update(await readFile(binary)).digest('hex'));
  const prepared=cli(['upgrade','prepare','--review-digest',reviewed.upgradeReview.digest]);started=false;
  assert.equal(prepared.stopping,true);const hold=prepared.upgradeHold;assert.equal(hold.validRecord,true);
  cli(['service','start'],4);cli(['status'],4);
  assert.equal(cli(['upgrade','status']).result.holdDigest,hold.holdDigest);
  cli(['upgrade','backup','--hold-digest','0'.repeat(64),'--to',archive],3);
  const before=await hashes(data);
  const backup=cli(['upgrade','backup','--hold-digest',hold.holdDigest,'--to',archive]).result;
  assert.deepEqual(await hashes(data),before);
  // Simulate a failed maintenance write only in this fresh test-owned directory.
  await writeFile(join(data,'failed-maintenance-marker'),'keep this failed state for review');
  const preserved=join(root,'failed-data-preserved');
  assert.equal(resolve(data,'..'),root);assert.equal(resolve(preserved,'..'),root);
  await rename(data,preserved);
  cli(['backup','restore','--from',archive,'--manifest-digest',backup.manifestDigest]);
  assert.deepEqual(await hashes(data),before);cli(['service','start'],4);
  cli(['upgrade','release','--hold-digest','0'.repeat(64)],3);
  cli(['upgrade','release','--hold-digest',hold.holdDigest]);
  cli(['status'],4);started=true;cli(['service','start']);
  assert.deepEqual(cli(['project','show','--project','project']).result.workflow,reviewed.result.workflows[0]);
  cli(['service','stop']);started=false;
  const result={status:'passed',report:root,queuedRequestRejected:true,reviewChangedRejected:true,authenticatedProcessExitAwaited:true,
    startupHeld:true,holdBoundBackup:true,restoredHoldRetained:true,reviewedReleaseRequired:true,releaseDoesNotStart:true,workflowUnchanged:true,
    dataFiles:Object.keys(before).length,backupManifestDigest:backup.manifestDigest,realModelCalls:0,userDesktopActions:0,crossVersionUpgradeTested:false,calls};
  await writeFile(join(root,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
}finally{
  late?.socket.destroy();stopper?.socket.destroy();
  if(child&&child.exitCode===null){child.kill();await once(child,'exit');}
  if(started){try{cli(['service','stop']);}catch(e){console.error('Own test core cleanup:',e.message);}}
}
