import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtemp,mkdir,readFile,readdir,rename,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const oldBinary=resolve('reports/p6-candidate-baseline-20261012/cerulean-runtime-0.1.0.exe');
const newBinary=resolve('target/release/cerulean-core.exe');
const sha=b=>createHash('sha256').update(b).digest('hex');
const provenance=JSON.parse(await readFile(resolve('reports/p6-candidate-baseline-20261012/provenance.json'),'utf8'));
assert.equal(sha(await readFile(oldBinary)),'dcfadecc725ddf87c627d763b1a112de6bc6278aa3311f5f61a41dfcfd12585d');
const run=await mkdtemp(resolve('reports/backup-version-native-'));
const data=join(run,'core data'),project=join(run,'project'),backup=join(run,'old-backup'),newBackup=join(run,'new-backup');
await mkdir(project);
let activeBinary;
const calls=[];
function cli(binary,args,expected=0){
  const r=spawnSync(binary,['--workbench','--data-dir',data,...args],{windowsHide:true,encoding:'utf8',timeout:45000,maxBuffer:5*1024*1024});
  assert.ok(!r.error,r.error?.code);assert.equal(r.status,expected,r.stderr);
  const response=JSON.parse(expected===0?r.stdout:r.stderr);assert.equal(response.ok,expected===0);
  calls.push({version:binary===oldBinary?'0.1.0':'0.1.1',command:args.slice(0,2).join(' '),exit:r.status});return response;
}
async function hashes(folder,prefix=''){
  const out={};for(const e of await readdir(join(folder,prefix),{withFileTypes:true})){
    const p=join(prefix,e.name);if(e.isDirectory())Object.assign(out,await hashes(folder,p));else out[p]=sha(await readFile(join(folder,p)));
  }return out;
}
try{
  const empty=join(run,'empty');await mkdir(empty);
  const infoResult=spawnSync(newBinary,['--build-info'],{cwd:empty,windowsHide:true,encoding:'utf8',timeout:10000});
  assert.equal(infoResult.status,0,infoResult.stderr);const info=JSON.parse(infoResult.stdout);
  assert.equal(info.version,'0.1.1');assert.deepEqual(info.backupWriterVersions,['0.1.0','0.1.1']);assert.deepEqual(await readdir(empty),[]);
  activeBinary=oldBinary;cli(oldBinary,['service','start']);
  cli(oldBinary,['project','add','--id','version-project','--root',project,'--timezone','Asia/Shanghai']);
  const reviewed=cli(oldBinary,['upgrade','review']);assert.equal(reviewed.upgradeReview.coreVersion,'0.1.0');
  const prepared=cli(oldBinary,['upgrade','prepare','--review-digest',reviewed.upgradeReview.digest]);activeBinary=null;
  const hold=prepared.upgradeHold;
  const created=cli(oldBinary,['upgrade','backup','--hold-digest',hold.holdDigest,'--to',backup]).result;
  const original=await hashes(data);
  assert.equal(JSON.parse(await readFile(join(backup,'manifest.json'),'utf8')).version,'0.1.0');
  assert.equal(cli(newBinary,['backup','verify','--from',backup]).result.manifestDigest,created.manifestDigest);
  const saved=join(run,'original-preserved');assert.equal(resolve(saved,'..'),run);assert.equal(resolve(data,'..'),run);
  await rename(data,saved);
  cli(newBinary,['backup','restore','--from',backup,'--manifest-digest',created.manifestDigest]);
  assert.deepEqual(await hashes(data),original);cli(newBinary,['service','start'],4);
  const newer=cli(newBinary,['upgrade','backup','--hold-digest',hold.holdDigest,'--to',newBackup]).result;
  assert.equal(JSON.parse(await readFile(join(newBackup,'manifest.json'),'utf8')).version,'0.1.1');
  cli(oldBinary,['backup','verify','--from',newBackup],3);
  cli(oldBinary,['backup','restore','--from',newBackup,'--manifest-digest',newer.manifestDigest],3);
  assert.deepEqual(await hashes(data),original);
  // Only synthetic copies of the manifest are changed; preserve both real backups.
  const {cp}=await import('node:fs/promises');const invalid=join(run,'future-backup');await cp(newBackup,invalid,{recursive:true,errorOnExist:true});
  const manifest=JSON.parse(await readFile(join(invalid,'manifest.json'),'utf8'));
  manifest.version='0.2.0';await writeFile(join(invalid,'manifest.json'),JSON.stringify(manifest));
  cli(newBinary,['backup','verify','--from',invalid],3);
  manifest.version='0.1.1';manifest.schema=2;await writeFile(join(invalid,'manifest.json'),JSON.stringify(manifest));
  cli(newBinary,['backup','verify','--from',invalid],3);
  cli(newBinary,['upgrade','release','--hold-digest',hold.holdDigest]);
  activeBinary=newBinary;cli(newBinary,['service','start']);
  assert.deepEqual(cli(newBinary,['project','show','--project','version-project']).result.workflow,reviewed.result.workflows[0]);
  cli(newBinary,['service','stop']);activeBinary=null;
  const result={status:'passed',run,oldRuntimeSha256:sha(await readFile(oldBinary)),newRuntimeSha256:sha(await readFile(newBinary)),oldProvenance:provenance,
    oldBackupReadAndRestored:true,restoredFiles:Object.keys(original).length,holdPreserved:true,workflowUnchanged:true,
    oldBackupReaderRejectsNewWriter:true,unknownVersionRejected:true,unknownSchemaRejected:true,buildInfoCreatesNoFiles:true,
    capabilities:info,crossVersionBinaryInstallationTested:false,ledgerSchemaMigrationTested:false,realModelCalls:0,userDesktopActions:0,calls};
  await writeFile(join(run,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
}finally{if(activeBinary){try{cli(activeBinary,['service','stop']);}catch(e){console.error('Own test core cleanup:',e.message);}}}
