// Real packaged CLI and journal, synthetic Codex handoff only. No model/network calls.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdirSync,readFileSync,writeFileSync,readdirSync,statSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';

const [packagePath,reportPath,mode='fixed']=process.argv.slice(2);
assert(process.env.CERULEAN_CACHE_SESSION && packagePath && reportPath);
const root=join(resolve(process.env.CERULEAN_CACHE_SESSION.replace(/^\\\\\?\\/,'')),'scratch','planning-capacity');
mkdirSync(root,{recursive:true});
const data=join(root,'data'),project=join(root,'project'),chief=join(root,'chief'),backup=join(root,'backup');
mkdirSync(project);
const exe=join(resolve(packagePath),'cerulean-runtime.exe'),cases=[],calls=[],metrics={};
const requirement='# 合成项目\n## 任务\n'+'- 保留中文、所有数据以及原始约束。\n'.repeat(900);
const draft={projectId:'capacity-project',requirement,workerAdapter:'deepseek',codexExecutable:'',workerModel:'deepseek-flash',
  chiefModel:'external-codex-session',deputyModel:'deepseek-flash',chiefDirectory:chief,
  managementSources:{chief:{kind:'codexSession',sessionId:'synthetic-chief-session'},deputy:{kind:'provider',endpoint:{kind:'deepSeek'}}},
  apiWorker:{target:{kind:'provider',endpoint:{kind:'deepSeek'}},model:'deepseek-flash',fileTools:['read-files','write-files'],maxRounds:3,maxToolCalls:16,maxOutputTokens:4096,maxTotalTokens:30000,requestTimeoutSeconds:120},
  validatorId:'cerulean-objective-checker',checkArgs:[],artifactPaths:[],workerMaxRequests:3,managementMaxRequests:10,maxSeconds:180,
  acceptance:{schemaVersion:1,stages:[{id:'project-check',name:'独立客观检查',validatorId:'cerulean-objective-checker',args:[],expectedAssertions:[],timeoutSeconds:60}]}};
function invoke(args){
  const result=spawnSync(exe,args,{cwd:root,windowsHide:true,encoding:'utf8',input:'',timeout:45000,maxBuffer:4*1024*1024});
  assert(!result.error,result.error?.message);
  return result;
}
function cli(args,expected=true){
  const result=invoke(['--workbench','--data-dir',data,...args]);
  const value=JSON.parse(result.status===0?result.stdout:result.stderr);
  calls.push({command:args.slice(0,2).join(' '),code:result.status});
  assert.equal(value.ok,expected,JSON.stringify(value));
  return value;
}
let started=false;
try{
  assert.equal(invoke(['--stdio','--data-dir',data]).status,0);
  const manifest=cli(['backup','create','--to',backup]).result.manifestDigest;
  const review=cli(['data-format','review-relay','--from',backup,'--manifest-digest',manifest]).result;
  cli(['data-format','promote-relay','--from',backup,'--manifest-digest',manifest,'--review-digest',review.digest]);
  cli(['service','start']);started=true;
  cli(['project','add','--id',draft.projectId,'--root',project]);
  const file=join(root,'setup.json');writeFileSync(file,JSON.stringify(draft));
  cli(['setup','save','--file',file]);
  const saved=cli(['setup','show','--project',draft.projectId]).result.setup;
  assert.equal(saved.setup.requirement,requirement);
  const requested=cli(['plan','request','--project',draft.projectId,'--setup-digest',saved.digest],mode!=='baseline');
  if(mode==='baseline'){
    assert(JSON.stringify(requested).includes('单次审阅容量'));
    assert.equal(readdirSync(join(data,'management-decisions')).filter(p=>p.endsWith('.json')).length,0);
    cases.push('Original runtime rejects the synthetic 45925-byte requirement at the 64KiB composed context guard before journal publication');
  }else{
    let snapshot=cli(['status']).result;
    const operation=snapshot.managementDecisions.find(d=>d.projectId===draft.projectId);
    assert.equal(operation.state,'awaitingExternal');
    const original=cli(['external-chief','read','--operation',operation.operationId]).result;
    const contextBytes=Buffer.byteLength(JSON.stringify(original.handoff.context));
    metrics.contextBytes=contextBytes;
    assert(contextBytes>64*1024&&contextBytes<256*1024);
    assert.equal(original.handoff.context.requirement,requirement);
    assert.equal(original.handoff.context.executionSetup.setup.requirement,requirement);
    assert.equal(original.handoff.context.roleWorkspace.material.requirement,requirement);
    const record=join(data,'management-decisions',operation.operationId+'.0.json');
    assert(statSync(record).size>256*1024);
    metrics.journalBytes=statSync(record).size;
    const recordHash=createHash('sha256').update(readFileSync(record)).digest('hex');
    cli(['service','stop']);started=false;
    cli(['service','start']);started=true;
    const reopened=cli(['external-chief','read','--operation',operation.operationId]).result;
    assert.deepEqual(reopened.handoff,original.handoff);
    assert.equal(createHash('sha256').update(readFileSync(record)).digest('hex'),recordHash);
    cli(['plan','request','--project',draft.projectId,'--setup-digest',saved.digest],false);
    cases.push('Large planning context and >256KiB journal persist, reopen byte-identically, and reject a duplicate request');
    const response={requestDigest:original.requestDigest,sessionId:'synthetic-chief-session',envelope:{stateDigest:original.handoff.stateDigest,decision:{action:'wait',reason:'合成测试已核对完整需求，请用户补充。'}}};
    const replyFile=join(root,'reply.json');writeFileSync(replyFile,JSON.stringify(response));
    cli(['external-chief','submit','--operation',operation.operationId,'--session','synthetic-chief-session','--file',replyFile]);
    snapshot=cli(['status']).result;
    assert.equal(snapshot.managementDecisions.find(d=>d.operationId===operation.operationId).state,'applied');
    assert.equal(snapshot.workflows[0].approval,null);
    cases.push('Bound synthetic reply validates the complete original context and records Wait without approving execution');
    const firstWait=snapshot.managementDecisions.find(d=>d.operationId===operation.operationId);
    const answer='合成补充：保留所有中文内容，输入格式为 UTF-8。';
    cli(['management','answer','--operation',operation.operationId,'--evidence-digest',firstWait.evidenceDigest,'--answer',answer]);
    cli(['plan','request','--project',draft.projectId,'--setup-digest',saved.digest]);
    snapshot=cli(['status']).result;
    const followup=snapshot.managementDecisions.find(d=>d.projectId===draft.projectId&&d.state==='awaitingExternal');
    assert(followup && followup.operationId!==operation.operationId);
    const followupRequest=cli(['external-chief','read','--operation',followup.operationId]).result;
    assert.equal(followupRequest.handoff.context.planningContextVersion,1);
    assert(followupRequest.handoff.context.userResponses.some(r=>r.answer===answer));
    assert.equal(followupRequest.handoff.context.requirement,requirement);
    cli(['service','stop']);started=false;
    cli(['service','start']);started=true;
    const followupReopened=cli(['external-chief','read','--operation',followup.operationId]).result;
    assert.deepEqual(followupReopened.handoff,followupRequest.handoff);
    response.requestDigest=followupRequest.requestDigest;
    response.envelope.stateDigest=followupRequest.handoff.stateDigest;
    writeFileSync(replyFile,JSON.stringify(response));
    cli(['external-chief','submit','--operation',followup.operationId,'--session','synthetic-chief-session','--file',replyFile]);
    cases.push('Long-plan clarification is accepted, included in the next handoff and preserved across reopen and reply validation');
    draft.requirement='完整需求不可截断。'.repeat(12000);writeFileSync(file,JSON.stringify(draft));
    const rejected=cli(['setup','save','--file',file],false);
    assert(JSON.stringify(rejected).includes('60 KiB'));
    assert.equal(cli(['setup','show','--project',draft.projectId]).result.setup.digest,saved.digest);
    cases.push('CLI JSON input over its separate 60KiB limit is rejected without overwriting the saved setup');
  }
}catch(error){writeFileSync(resolve(reportPath),JSON.stringify({passed:false,error:String(error.stack),cases,calls},null,2));throw error;}
finally{
  if(started){
    cli(['service','stop']);
  }
}
writeFileSync(resolve(reportPath),JSON.stringify({passed:true,mode,cases,calls,metrics,requirementBytes:Buffer.byteLength(requirement),modelRequests:0},null,2));
console.log(JSON.stringify({passed:true,cases:cases.length,report:resolve(reportPath)}));
