// Fixed incident readback: never invokes tasksend, focuses, or clears a draft.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve,relative} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const source=resolve(process.env.CERULEAN_WORKBUDDY_RECONCILE_REPORT??'reports/workbuddy-send-live-AnlJ8l');
const sourceRelative=relative(resolve('reports'),source);
assert.match(sourceRelative,/^workbuddy-(?:send-live|coding-send)-[A-Za-z0-9]+$/,'Expected an existing local live probe report');
const review=JSON.parse(await readFile(join(source,'task-0/task-review.json'),'utf8'));
const report=await mkdtemp(resolve('reports/workbuddy-send-reconcile-'));
const results={};
for(const [name,request] of [['reply',{op:'taskread',taskReview:review}],['history',{op:'taskprepare',window:review.observation.window}]]){
 const run=spawnSync(join(source,'Observer.exe'),[JSON.stringify({version:1,...request})],{encoding:'utf8',windowsHide:true,timeout:30000});
 assert.equal(run.error,undefined,run.error?.message);
 const response=JSON.parse(run.stdout.replace(/^\uFEFF/,''));
 results[name]={exitCode:run.status,response};
 await writeFile(join(report,name+'.json'),JSON.stringify(results[name],null,2)+'\n');
}
const history=results.history.response;
const result={status:'observed',readOnly:true,sendAttempts:0,originalAttempt:source,replyProven:results.reply.response.ok,historyReadable:history.ok,originalBaselineUnchanged:history.ok&&JSON.stringify(history.result.baseline)===JSON.stringify(review.baseline),draftEmpty:history.result?.observation.draftEmpty??null,busy:history.result?.observation.busy??null,replyError:results.reply.response.error??null};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,...result}));
