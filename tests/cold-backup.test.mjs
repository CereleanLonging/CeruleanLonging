// Backup-specific integration: real CLI/core/DPAPI/files, no model or desktop calls.
import {spawnSync} from 'node:child_process';
import {mkdtemp,mkdir,readdir,readFile,rename,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const binary=resolve('target/debug/cerulean-core.exe');
const root=await mkdtemp(resolve('reports/cold-backup-native-'));
const data=join(root,'核心 数据'),archive=join(root,'冷备份'),project=join(root,'项目');
await mkdir(project);
const calls=[];
function cli(args,input='',expected=0){
  const r=spawnSync(binary,['--workbench','--data-dir',data,...args],{input,windowsHide:true,encoding:'utf8',timeout:25000,maxBuffer:5*1024*1024});
  assert.ok(!r.error,`${args[0]}: ${r.error?.code}`);
  assert.equal(r.status,expected,`${args[0]} ${args[1]}: ${r.stderr}`);
  const reply=JSON.parse(expected===0?r.stdout:r.stderr);
  assert.equal(reply.ok,expected===0);
  calls.push({command:args[0]+' '+(args[1]??''),exit:r.status});
  return reply;
}
async function hashes(folder,prefix=''){
  const result={};
  for(const e of await readdir(join(folder,prefix),{withFileTypes:true})){
    const name=join(prefix,e.name);
    if(e.isDirectory())Object.assign(result,await hashes(folder,name));
    else result[name]=createHash('sha256').update(await readFile(join(folder,name))).digest('hex');
  }
  return result;
}
let started=false;
try{
  started=true;cli(['service','start']);
  cli(['project','add','--id','backup-project','--root',project,'--timezone','Asia/Shanghai']);
  cli(['credentials','set','--provider','openai'],'synthetic-cold-backup-only-not-a-real-key\n');
  const workflow=cli(['project','show','--project','backup-project']).result.workflow;
  cli(['backup','create','--to',archive],'',3);
  cli(['service','stop']);started=false;
  // Wait only for our acknowledged shutdown to release its original store lock.
  await new Promise(r=>setTimeout(r,300));
  const original=await hashes(data);
  const created=cli(['backup','create','--to',archive]).result;
  const verified=cli(['backup','verify','--from',archive]).result;
  assert.equal(created.manifestDigest,verified.manifestDigest);
  assert.deepEqual(await hashes(data),original);
  cli(['backup','restore','--from',archive,'--manifest-digest',created.manifestDigest],'',3);
  // Both resolved paths are siblings inside this newly created, test-owned report.
  const preserved=join(root,'preserved-original');
  assert.equal(resolve(data,'..'),root);assert.equal(resolve(preserved,'..'),root);
  await rename(data,preserved);
  cli(['backup','restore','--from',archive,'--manifest-digest','0'.repeat(64)],'',3);
  cli(['backup','restore','--from',archive,'--manifest-digest',created.manifestDigest]);
  assert.deepEqual(await hashes(data),original);
  cli(['status'],'',4); // Restore must not start the service.
  started=true;cli(['service','start']);
  assert.equal(cli(['providers','list']).result.openai.keyConfigured,true);
  assert.deepEqual(cli(['project','show','--project','backup-project']).result.workflow,workflow);
  cli(['service','stop']);started=false;
  const result={status:'passed',report:root,manifestDigest:created.manifestDigest,fileCount:Object.keys(original).length,byteExactRestore:true,workflowUnchanged:true,restoreDidNotStartService:true,liveCoreRejected:true,existingDestinationRejected:true,staleDigestRejected:true,realModelCalls:0,userDesktopActions:0,calls};
  await writeFile(join(root,'result.json'),JSON.stringify(result,null,2));
  console.log(JSON.stringify(result));
}finally{if(started){try{cli(['service','stop']);}catch(e){console.error('Owned test core cleanup:',e.message);}}}
