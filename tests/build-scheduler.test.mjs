import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';

assert(process.env.CERULEAN_CACHE_SESSION,'Use the managed cache runner');
const run=await mkdtemp(resolve('reports/build-scheduler-'));
const fixture=join(process.env.CERULEAN_CACHE_SESSION,'scheduler-probe.cjs');
await writeFile(fixture,`const {spawnSync}=require('child_process');const os=require('os');
console.log('PROBE '+JSON.stringify({args:process.argv.slice(2),jobs:process.env.CARGO_BUILD_JOBS,priority:os.getPriority(),active:process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE}));
const child=spawnSync(process.execPath,['-e',"console.log('CHILD '+JSON.stringify({jobs:process.env.CARGO_BUILD_JOBS,priority:require('os').getPriority()}))"],{stdio:'inherit'});if(child.status)process.exit(child.status);
console.error('diagnostic-stderr');setTimeout(()=>process.exit(7),3500);`);
const args=['with spaces','literal"quote','C:\\trailing\\','$(not-a-command)','literal`text','中文'];
const launcher=resolve('scripts/build-scheduler.mjs');
const first=spawn(process.execPath,[launcher,process.execPath,fixture,...args],{windowsHide:true,stdio:['ignore','pipe','pipe']});
let stdout='',stderr='';first.stdout.on('data',data=>stdout+=data);first.stderr.on('data',data=>stderr+=data);
const completed=once(first,'close');
for(let n=0;n<200&&!stdout.includes('PROBE ')&&first.exitCode===null;n++)await new Promise(resolve=>setTimeout(resolve,50));
if(!stdout.includes('PROBE ')){
  await completed;await writeFile(join(run,'deferred.json'),JSON.stringify({stdout,stderr,exitCode:first.exitCode}));
  throw Error('Scheduler did not admit the lightweight probe; inspect receipt before continuing');
}
const second=spawnSync(process.execPath,[launcher,process.execPath,'-e','console.log("UNEXPECTED_SECOND_BUILD")'],{encoding:'utf8',windowsHide:true});
assert.equal(second.status,75,second.stdout+second.stderr);assert(!second.stdout.includes('UNEXPECTED_SECOND_BUILD'));
const [code]=await completed;assert.equal(code,7,stderr);assert(stderr.includes('diagnostic-stderr'));
const probe=JSON.parse(stdout.split('\n').find(line=>line.startsWith('PROBE ')).slice(6));
assert.deepEqual(probe.args,args);assert.equal(probe.jobs,'1');assert.equal(probe.active,'1');assert.equal(probe.priority,10);
const child=JSON.parse(stdout.split('\n').find(line=>line.startsWith('CHILD ')).slice(6));assert.equal(child.jobs,'1');assert.equal(child.priority,10);
const path=stdout.split('\n').find(line=>line.startsWith('构建调度回执：')).slice('构建调度回执：'.length).trim();
const receipt=JSON.parse(await readFile(path,'utf8'));
assert.equal(receipt.policy.CpuPercent,20);assert.equal(receipt.policy.PriorityClass,16384);
assert(BigInt(receipt.policy.AffinityMask).toString(2).replaceAll('0','').length<=2);
assert.equal(receipt.policy.MemoryLimitBytes,1024**3);
assert.equal(receipt.policy.ReserveBytes,1024**3);
assert.equal(receipt.requiredAvailableBytes,1024**3);
assert.equal(receipt.admissionReferenceBytes,2*1024**3);
assert.equal(receipt.admissionToleranceBytes,1024**3);
assert(receipt.peakJobMemoryBytes>0&&receipt.peakJobMemoryBytes<=receipt.policy.MemoryLimitBytes);
assert(receipt.observedMinimumPhysicalBytes>0&&receipt.observedMinimumCommitBytes>0);
assert.equal(receipt.exitCode,7);assert.equal(receipt.state,'failed');
const cases=['Windows Job limits set and read back; child and grandchild inherit BelowNormal and one Cargo job','Native arguments remain literal, stdout/stderr are forwarded, and failure exit code is preserved','A concurrent build is deferred before its program starts'];
const boundaryScript=join(process.env.CERULEAN_CACHE_SESSION,'memory-boundaries.ps1');
await writeFile(boundaryScript,`Add-Type -Path '${resolve('scripts/build-resource-job.cs').replaceAll("'","''")}'
$cases=@(@(2GB,2GB,1GB,1GB),@(8GB,8GB,1GB,1GB),@(1.5GB,1.5GB,1GB,1GB),@(1GB,1GB,1GB,1GB),@((1GB-1),2GB,1GB,0),@(2GB,(1GB-1),1GB,0),@(4GB,4GB,3GB,3GB),@(8GB,8GB,3GB,3GB),@(3.5GB,3.5GB,3GB,3GB),@(3GB,3GB,3GB,3GB),@((3GB-1),4GB,3GB,0),@(4GB,(3GB-1),3GB,0))
foreach($case in $cases){if([CeruleanBuildJob]::Budget($case[0],$case[1],$case[2]) -ne $case[3]){throw 'Memory boundary mismatch'}}
foreach($minimum in @((1GB-1),(3GB+1))){$rejected=$false;try{[CeruleanBuildJob]::Budget(8GB,8GB,$minimum)|Out-Null}catch{$rejected=$true};if(-not $rejected){throw 'Invalid memory budget accepted'}}
Write-Output '14 memory boundaries passed'
`);
const shell=join(process.env.SystemRoot,'System32/WindowsPowerShell/v1.0/powershell.exe');
const boundaries=spawnSync(shell,['-NoProfile','-ExecutionPolicy','RemoteSigned','-File',boundaryScript],{encoding:'utf8',windowsHide:true});
assert.equal(boundaries.status,0,boundaries.stdout+boundaries.stderr);
cases.push('Physical and commit admission accept a shortfall up to 1 GiB, reject larger shortfalls, and never raise the selected limit');
cases.push('Kernel Job peak includes exited descendants and is reported alongside the observed physical/commit minima');
await writeFile(join(run,'result.json'),JSON.stringify({passed:true,cases,receipt,stdout,stderr},null,2));
console.log(JSON.stringify({passed:true,cases:cases.length,run}));
