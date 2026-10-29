import {spawn} from 'node:child_process';
import {mkdtemp,mkdir,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
const binary=resolve('target/debug/cerulean-core.exe');
const root=await mkdtemp(resolve('reports/workbench-cli-startup-'));
const data=join(root,'用户 空格','核心目录');await mkdir(data,{recursive:true});
const calls=[];
function invoke(args){return new Promise((resolve,reject)=>{
  const start=Date.now(),child=spawn(binary,['--workbench','--data-dir',data,...args],{windowsHide:true,stdio:['ignore','pipe','pipe']});
  let out='',err='';child.stdout.on('data',b=>out+=b);child.stderr.on('data',b=>err+=b);
  const timeout=setTimeout(()=>{child.kill();reject(Error('Owned CLI output did not close within 10 seconds'));},10000);
  child.once('error',e=>{clearTimeout(timeout);reject(e);});
  child.once('close',code=>{clearTimeout(timeout);try{assert.equal(code,0,err);const reply=JSON.parse(out);calls.push({command:args.join(' '),milliseconds:Date.now()-start,serverPid:reply.serverPid});resolve(reply);}catch(e){reject(e);}});
});}
let started=false;
try{
  // The same caller captures stdout/stderr from both starters; neither CLI may
  // keep these pipes alive through a resident child, including the losing race.
  started=true;
  const settled=await Promise.allSettled([invoke(['service','start']),invoke(['service','start'])]);
  for(const r of settled)assert.equal(r.status,'fulfilled',String(r.reason));
  const [a,b]=settled.map(r=>r.value);assert.equal(a.serverPid,b.serverPid);
  const alive=await invoke(['status']);assert.equal(alive.serverPid,a.serverPid);
  await invoke(['service','stop']);started=false;
  await writeFile(join(root,'result.json'),JSON.stringify({status:'passed',unicodeDataDirectory:true,concurrentStarters:2,samePid:true,cliPipesClosedWhileResidentAlive:true,calls,realModelCalls:0},null,2));
  console.log(JSON.stringify({status:'passed',report:root,calls}));
}finally{if(started)await invoke(['service','stop']).catch(e=>console.error('Owned startup cleanup:',e.message));}
