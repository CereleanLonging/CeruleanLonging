// Real user CLI and resident IPC. The reused DAG service replaces model replies only.
import assert from 'node:assert/strict';
import {spawn,spawnSync} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp,mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';

const binary=resolve('target/debug/cerulean-core.exe');
const fixtureExe=process.env.CERULEAN_CLI_TEST_EXE;
assert.ok(fixtureExe);
const root=await mkdtemp(resolve('reports/workbench-cli-'));
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const results=[];
function cli(data,args,input='',expected=0){
  const r=spawnSync(binary,['--workbench','--data-dir',data,...args],{input,windowsHide:true,encoding:'utf8',timeout:25000,maxBuffer:5*1024*1024});
  if(r.error&&args[0]==='service'&&args[1]==='start') {
    try {productionPid=JSON.parse(r.stdout).serverPid;productionData=data;} catch {}
  }
  assert.ok(!r.error,`${args[0]} ${args[1]??''}: ${r.error?.code}`);
  // Never include input in assertions or reports; credential probes use a synthetic sentinel.
  assert.equal(r.status,expected,`${args[0]} ${args[1]??''}: ${r.stderr}`);
  const output=JSON.parse(expected===0?r.stdout:r.stderr);
  assert.equal(output.ok,expected===0);
  return {output,raw:r.stdout+r.stderr};
}
async function until(fn,label){const end=Date.now()+45000;while(Date.now()<end){const result=await fn();if(result)return result;await sleep(80);}throw Error(`Timed out: ${label}`);}
async function history(data){
  const folder=join(data,'workflows/project/events');
  const entries=(await readdir(folder)).sort();
  return Promise.all(entries.map(async name=>({name,sha256:createHash('sha256').update(await readFile(join(folder,name))).digest('hex')})));
}
let productionData,productionPid,fixtureChild,fixtureLog='';
try {
  const help=spawnSync(binary,['--workbench','--help'],{windowsHide:true,encoding:'utf8'});
  assert.equal(help.status,0);assert.match(help.stdout,/run start\|resume/);
  const basic=join(root,'basic');await mkdir(basic);
  productionData=join(basic,'data');
  cli(productionData,['status'],'',4); // No implicit service startup.
  let response=cli(productionData,['service','start']).output;
  productionPid=response.serverPid;
  assert.equal(cli(productionData,['service','start']).output.serverPid,productionPid);
  const project=join(basic,'项目 空格');await mkdir(project);
  cli(productionData,['project','add','--id','project','--root',project,'--timezone','Asia/Shanghai']);
  const before=await history(productionData);
  cli(productionData,['plan','approve','--project','project'],'',2);
  cli(productionData,['plan','approve','--project','project','--digest','0'.repeat(64)],'',3);
  cli(productionData,['status','--principal','core'],'',2);
  cli(productionData,['status','--data-dir','other'],'',2);
  assert.deepEqual(await history(productionData),before);
  const sentinel='synthetic-cli-key-not-a-real-credential';
  for(const provider of ['openai','anthropic','deepseek','qwen','kimi','grok']){
    const set=cli(productionData,['credentials','set','--provider',provider],sentinel+'\r\n');
    assert.ok(!set.raw.includes(sentinel));
    const s=cli(productionData,['providers','list']);assert.ok(!s.raw.includes(sentinel));
    if(provider==='openai')assert.equal(s.output.result.openai.keyConfigured,true);
    else assert.ok(s.output.result.credentials.some(c=>c.provider===provider&&c.keyConfigured));
    cli(productionData,['credentials','remove','--provider',provider]);
  }
  cli(productionData,['credentials','set','--provider','openai','--key',sentinel],'',2);
  cli(productionData,['credentials','set','--provider','openai'],sentinel+'\n\n',2);
  const unknown=join(basic,'unknown.json');await writeFile(unknown,JSON.stringify({projectId:'project',principal:'core',key:sentinel}));
  const rejected=cli(productionData,['setup','save','--file',unknown],'',2);assert.ok(!rejected.raw.includes(sentinel));
  assert.equal(cli(productionData,['project','show','--project','project']).output.result.workflow.project.id,'project');
  cli(productionData,['project','show','--project','missing'],'',3);
  cli(productionData,['service','stop']);productionPid=null;
  await sleep(250);
  cli(productionData,['status'],'',4);
  response=cli(productionData,['service','start']).output;productionPid=response.serverPid;
  assert.equal(response.result.workflows[0].project.id,'project');
  assert.deepEqual(await history(productionData),before);
  cli(productionData,['service','stop']);productionPid=null;
  results.push({name:'production-cli',status:'passed',credentialProviders:6,sameResident:true,unicodePath:true,restart:true,rejectedRequestsAddedEvents:0,networkCalls:0});

  const dag=join(root,'dag');await mkdir(dag);
  fixtureChild=spawn(fixtureExe,['--exact','host::api_run_tests::native_api_dag_server','--ignored','--nocapture'],{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,CERULEAN_API_DAG_NATIVE_DIR:dag}});
  fixtureChild.stdout.on('data',b=>fixtureLog+=b);fixtureChild.stderr.on('data',b=>fixtureLog+=b);
  const {data}=await until(async()=>{assert.equal(fixtureChild.exitCode,null,fixtureLog);try{return JSON.parse(await readFile(join(dag,'fixture.json'),'utf8'));}catch{return false;}},'existing fixture service');
  await sleep(350);
  let snapshot=cli(data,['status']).output.result;
  const original=snapshot.projectSetups[0].setup;
  const draft={projectId:'project',requirement:original.requirement,codexExecutable:'',workerModel:original.apiWorker.model,workerAdapter:'deepseek',apiWorker:original.apiWorker,chiefModel:original.chiefModel,deputyModel:original.deputyModel,validatorId:original.validatorId,checkArgs:original.check.args,artifactPaths:original.artifactPaths,workerMaxRequests:original.policy.maxRequests,managementMaxRequests:original.managementMaxRequests,maxSeconds:original.policy.maxSeconds};
  const setupFile=join(dag,'setup.json');
  // Reuse the original eight API route configurations; check only the new CLI save boundary.
  const routes=[['openai',{kind:'openAi'}],['anthropic',{kind:'provider',endpoint:{kind:'anthropic'}}],['deepseek',{kind:'provider',endpoint:{kind:'deepSeek'}}],['qwen',{kind:'provider',endpoint:{kind:'qwenBeijing',workspaceId:'approved-space'}}],['qwen',{kind:'provider',endpoint:{kind:'qwenSingapore'}}],['qwen',{kind:'provider',endpoint:{kind:'qwenHongKong'}}],['kimi',{kind:'provider',endpoint:{kind:'kimiChina'}}],['grok',{kind:'provider',endpoint:{kind:'grok'}}]];
  for(const [workerAdapter,target] of routes){
    const config={...draft,workerAdapter,apiWorker:{...draft.apiWorker,target}};
    await writeFile(setupFile,JSON.stringify(config));
    const saved=cli(data,['setup','save','--file',setupFile]).output.result;
    assert.equal(saved.projectSetups[0].setup.workerAdapter,workerAdapter);
    assert.deepEqual(saved.projectSetups[0].setup.apiWorker.target,target);
    assert.equal(saved.apiJobs.length,0);assert.equal(saved.projectRuns.length,0);
  }
  await writeFile(setupFile,'\ufeff'+JSON.stringify(draft));
  snapshot=cli(data,['setup','save','--file',setupFile]).output.result;
  let setupDigest=snapshot.projectSetups[0].digest;
  cli(data,['plan','request','--project','project','--setup-digest','0'.repeat(64)],'',3);
  cli(data,['plan','request','--project','project','--setup-digest',setupDigest]);
  snapshot=await until(()=>{const s=cli(data,['status']).output.result;return s.workflows[0].revision?s:false;},'chief plan');
  const revision=snapshot.workflows[0].revisionDigest;
  assert.equal(snapshot.apiJobs.length,0);assert.equal(snapshot.projectRuns.length,0);
  const originalHistory=await history(data);
  cli(data,['run','start','--project','project','--revision-digest',revision,'--setup-digest',setupDigest],'',3);
  cli(data,['plan','approve','--project','project','--digest','0'.repeat(64)],'',3);
  assert.deepEqual(await history(data),originalHistory);
  const plan=cli(data,['plan','show','--project','project']).output.result;
  assert.equal(plan.workflow.revisionDigest,revision);assert.equal(plan.workflow.revision.workflow.tasks.length,2);
  cli(data,['plan','approve','--project','project','--digest',revision]);
  cli(data,['run','start','--project','project','--revision-digest',revision,'--setup-digest','0'.repeat(64)],'',3);
  cli(data,['run','start','--project','project','--revision-digest',revision,'--setup-digest',setupDigest]);
  snapshot=await until(()=>{const s=cli(data,['status']).output.result;assert.ok(!s.projectRuns.some(r=>r.state==='attention'),JSON.stringify(s.projectRuns));return s.projectRuns[0]?.state==='completed'?s:false;},'independent acceptance');
  assert.equal(snapshot.apiJobs.length,2);assert.equal(snapshot.verificationJobs.length,2);
  assert.ok(Object.values(snapshot.workflows[0].state.tasks).every(t=>t.state==='accepted'));
  assert.equal(await readFile(join(dag,'project/result.txt'),'utf8'),'alpha beta');
  const management=cli(data,['management','list','--project','project']).output.result;
  const op=management.managementDecisions[0].operationId;
  cli(data,['management','show','--operation',op]);
  const verification=snapshot.verificationJobs[0];
  cli(data,['verification','show','--project','project','--operation',verification.operationId]);
  const completedHistory=await history(data);
  cli(data,['service','stop']);
  await until(()=>fixtureChild.exitCode!==null,'fixture stop');assert.equal(fixtureChild.exitCode,0,fixtureLog);
  productionData=data;productionPid=cli(data,['service','start']).output.serverPid;
  const reopened=cli(data,['status']).output.result;
  assert.equal(reopened.projectRuns[0].state,'completed');assert.deepEqual(await history(data),completedHistory);
  cli(data,['service','stop']);productionPid=null;
  results.push({name:'cli-original-dag',status:'passed',configurationRoutes:8,tasksAccepted:2,independentChecks:2,originalApprovalRequired:true,staleDigestsRejected:true,reopenNewEvents:0,modelReplies:'controlled',realModelCalls:0});
  await writeFile(join(root,'result.json'),JSON.stringify({status:'passed',results,realModelCalls:0,userDesktopActions:0},null,2));
  console.log(JSON.stringify({status:'passed',report:root,results}));
}catch(error){await writeFile(join(root,'failure.json'),JSON.stringify({error:String(error),results},null,2));throw error;}
finally{
  await writeFile(join(root,'fixture.log'),fixtureLog);
  if(fixtureChild&&fixtureChild.exitCode===null){const exit=once(fixtureChild,'exit');fixtureChild.kill();await exit;}
  if(productionPid){try{cli(productionData,['service','stop']);}catch(e){console.error('Owned core requires cleanup through its authenticated data-directory endpoint:',String(e));}}
}
