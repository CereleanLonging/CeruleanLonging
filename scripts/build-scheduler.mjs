import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync,mkdtempSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const supplied=process.argv.slice(2);
const rust=supplied[0]==='--rust';if(rust)supplied.shift();
const [program,...args]=supplied;
if(!program)throw Error('Expected build program and arguments');
if(process.platform!=='win32'||process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE==='1'){
  const result=spawnSync(program,args,{stdio:'inherit',shell:false,env:{...process.env,CARGO_BUILD_JOBS:'1'}});
  if(result.error)throw result.error;process.exit(result.status??1);
}
const cache=process.env.CERULEAN_CACHE_SESSION||join(root,'.temp/build-cache/bootstrap');
mkdirSync(cache,{recursive:true});
const run=mkdtempSync(join(cache,'scheduling-'));
const receipt=join(run,'result.json'),configuration=join(run,'command.json');
const workload=rust?'rust':'frontend';
// Rust: 2.17 GiB failed and 3 GiB completed this checkout's full release build.
// Frontend: independently validate under its smaller budget; never claim that
// an observed peak proves an absolute minimum or silently increase a failed cap.
const memoryBytes=(rust?3:1)*1024**3;
const calibrationPath=join(root,'docs/build-cache/build-memory-calibration.json');
let calibration={schema:1,profiles:{}};
try{const saved=JSON.parse(readFileSync(calibrationPath,'utf8'));if(saved.schema===1&&saved.profiles&&typeof saved.profiles==='object')calibration=saved;}catch{}
const prior=calibration.profiles[workload];
writeFileSync(configuration,JSON.stringify({program,arguments:args,directory:process.cwd(),receipt,
  minimumMemoryBytes:memoryBytes,
  workspaceId:createHash('sha256').update(root.toLowerCase()).digest('hex')}));
// Use the installed Windows shell. No command or argument is interpolated as PowerShell source.
const shell=join(process.env.SystemRoot||'C:/Windows','System32/WindowsPowerShell/v1.0/powershell.exe');
const modulePath=[process.env.PSModulePath,join(process.env.SystemRoot||'C:/Windows','System32/WindowsPowerShell/v1.0/Modules')].filter(Boolean).join(';');
const result=spawnSync(shell,['-NoProfile','-ExecutionPolicy','RemoteSigned','-File',join(root,'scripts/build-resource-guard.ps1'),'-Configuration',configuration],
  {stdio:'inherit',shell:false,windowsHide:true,env:{...process.env,TEMP:run,TMP:run,PSModulePath:modulePath}});
if(result.error)throw result.error;
let record;try{record=JSON.parse(readFileSync(receipt,'utf8'));}catch{}
if(record?.state==='running')record.state='interrupted';
const logs=join(root,'docs/build-cache/scheduling-logs');mkdirSync(logs,{recursive:true});
const report=join(logs,`${Date.now()}-${process.pid}.json`);
const complete={at:new Date().toISOString(),program,workload,...record,exitCode:result.status,
  memoryBasis:rust?'Lowest successful full-release budget so far: 3 GiB; 2.17 GiB failed. Not an absolute minimum.':'A 1 GiB budget for lightweight frontend tasks; revalidated by each completed command.',
  previousSuccessfulMeasurement:prior??null};
writeFileSync(report,JSON.stringify(complete,null,2));
if(result.status===0&&record?.state==='completed'&&Number.isSafeInteger(record.peakJobMemoryBytes)&&record.peakJobMemoryBytes>0){
  const entry={at:complete.at,budgetBytes:memoryBytes,peakJobMemoryBytes:record.peakJobMemoryBytes,requiredAvailableBytes:record.requiredAvailableBytes,receipt:report};
  calibration.profiles[workload]={...entry,highestSuccessfulPeakBytes:Math.max(prior?.highestSuccessfulPeakBytes??0,entry.peakJobMemoryBytes)};
  try{writeFileSync(calibrationPath,JSON.stringify(calibration,null,2));}catch(error){console.error('内存测量汇总未保存；完整回执仍在：'+report+' ('+error.message+')');}
}
console.log(`构建调度回执：${report}`);
process.exit(result.status??1);
