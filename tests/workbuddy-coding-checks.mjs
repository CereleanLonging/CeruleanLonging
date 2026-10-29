// Verify the independent checker against good, wrong, and nonterminating artifacts.
import {spawnSync} from 'node:child_process';
import {mkdtemp,writeFile,readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const report=await mkdtemp(resolve('reports/workbuddy-coding-checks-'));
const verifier=resolve('tests/fixtures/workbuddy-coding-verifier.mjs');
const env=Object.fromEntries(['SystemRoot','WINDIR','TEMP','TMP'].filter(k=>process.env[k]).map(k=>[k,process.env[k]]));
const cases=[];
for(const [name,source,accepted] of [
 ['valid',"export function sum(a,b){if(typeof a!=='number'||typeof b!=='number'||!Number.isFinite(a)||!Number.isFinite(b))throw new TypeError('finite numbers required');return a+b;}",true],
 ['wrong-math','export function sum(){return 0;}',false],
 ['unchecked-input','export function sum(a,b){return a+b;}',false],
 ['loop','export function sum(){while(true){}}',false],
 ['external-import',"import fs from 'node:fs';export function sum(a,b){return a+b;}",false],
]){
 const file=join(report,name+'.mjs');await writeFile(file,source);
 const run=spawnSync(process.execPath,['--permission','--allow-fs-read='+file,'--allow-fs-read='+verifier,'--experimental-vm-modules',verifier,file],{encoding:'utf8',windowsHide:true,timeout:5000,env});
 assert.equal(run.error,undefined,run.error?.message);assert.equal(run.status===0,accepted,name);
 if(accepted)assert.deepEqual(JSON.parse(run.stdout),{status:'passed',checks:15});
 if(name==='loop')assert.match(run.stderr,/Script execution timed out/);
 if(name==='external-import')assert.match(run.stderr,/imports are outside this task/);
 await writeFile(join(report,name+'.log'),run.stdout+'\n'+run.stderr);
 cases.push({name,passed:true,accepted,exitCode:run.status});
}
const plan=await readFile('tests/fixtures/workbuddy-coding-plan.json');
const result={status:'passed',cases,planHash:createHash('sha256').update(plan).digest('hex'),verifierSha256:createHash('sha256').update(await readFile(verifier)).digest('hex'),externalMessages:0};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,...result}));
