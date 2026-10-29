// Production parser with sanitized captured outer topology and synthetic rich bodies.
// No real application, keyboard, mouse, model, or network is used.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-rich-reply-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
const marker='CL_TASK_0123456789abcdef0123456789abcdef';
const prompt='Complete synthetic coding task '+marker;
const harness=join(report,'RichHarness.cs');
await writeFile(harness,`using System;using System.IO;using System.Linq;
public partial class DesktopObserver {
 class RichInput {public string document,prompt,marker;public WorkBuddyNode[] nodes;}
 public static int TestRich(string path) {try {
  var input=json.Deserialize<RichInput>(File.ReadAllText(path));var rounds=WorkBuddyRounds(input.document,input.nodes);
  var o=new Observation{window=new Window{app="workbuddy",hwnd="123",pid=42,startTicks="123456",exe=@"D:\\apps\\WorkBuddy.exe",title="WorkBuddy",version="5.7.7.0"},capturedAt="2026-10-11T13:00:00Z",documentRuntimeId=input.document,sessionId=null,contentDigest=new string('a',64),layoutDigest=new string('b',64),modelLabel="Hy3",nodeCount=input.nodes.Length,draftEmpty=true,busy=false};
  var review=new TaskReview{id=new string('1',64),bindingId=new string('2',64),marker=input.marker,prompt=input.prompt,promptSha256=Hash(input.prompt),bridgeSha256=new string('5',64),observation=o,baseline=Baseline(rounds.Take(rounds.Length-1).ToArray()),maxWaitSeconds=10};
  Console.WriteLine(json.Serialize(new{ok=true,baseline=Baseline(rounds),review=review,result=TaskResult(review,new View{observation=o},rounds)}));return 0;
 }catch(InvalidOperationException e){Console.WriteLine(json.Serialize(new{ok=false,error=e.Message}));return 1;}}
}
class RichHarness {static int Main(string[] args){return DesktopObserver.TestRich(args[0]);}}
`);
function compile(name,sourceRoot){
 const exe=join(report,name+'.exe');
 const r=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/main:RichHarness','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...['DesktopObserver','DesktopTrial','DesktopStop'].map(n=>resolve(sourceRoot,n+'.cs')),harness],{encoding:'utf8',windowsHide:true,timeout:30000});
 assert.equal(r.status,0,r.stdout+r.stderr);return exe;
}
const exe=compile('Current','crates/core/native');
const legacy=compile('Legacy061','reports/p4-workbuddy-click-sources-20261011/crates/core/native');
const captured=JSON.parse(await readFile('reports/desktop-capability-diagnostic-dC6xvT/result.json','utf8'));
const diagnostic=captured.observations.find(o=>o.window.app==='workbuddy').lines.find(l=>l.diagnostic);
const document=diagnostic.nodes.find(n=>n.type==='ControlType.Document').id;
const panel=document+'.4.0.1.0.0.0.1',list=panel+'.1.0.0.0.0.0.0',body=list+'.8.0.3';
const nodes=diagnostic.nodes.map(n=>({id:n.id,type:n.type,name:n.label??(n.type==='ControlType.Text'?'synthetic text':'')}));
nodes.find(n=>n.type==='ControlType.Text'&&n.id.startsWith(list+'.7.0.0.0.')).name=prompt;
const base={document,prompt,marker,nodes:nodes.filter(n=>!n.id.startsWith(body+'.'))};
const node=(suffix,type,name='')=>({id:body+suffix,type:'ControlType.'+type,name});
const plain=()=>[node('.0','Text','Implemented sum.\n'),node('.1','Text',marker)];
const rich=()=>[
 node('.0','Text','Changed sum.mjs.\n'),node('.1','Custom','code'),
 node('.1.0','Text','export function sum(a,b){return a+b;}'),node('.1.1','Button','复制'),
 node('.2','Custom','tool result'),node('.2.0','Text','node tests: 3 passed'),
 node('.3','Text','Checks passed.\n'+marker)
];
async function invoke(binary,name,input){
 const file=join(report,name+'.json');await writeFile(file,JSON.stringify(input));
 const r=spawnSync(binary,[file],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(r.error,undefined);
 const v=JSON.parse(r.stdout.replace(/^\uFEFF/,''));assert.equal(r.status,v.ok?0:1);return v;
}
const cases=[];
async function test(name,fragments,expected=true,mutate=()=>{}){
 const input=structuredClone(base);input.nodes.push(...fragments);mutate(input);
 const v=await invoke(exe,name,input);assert.equal(v.ok,expected,name+': '+JSON.stringify(v));
 if(!expected)assert.ok(['trial_history_unavailable','trial_reply_unverified'].includes(v.error));
 cases.push({name,passed:true,accepted:expected});return {value:v,input};
}
const old=await test('plain-compatible',plain());
assert.deepEqual(old.value,await invoke(legacy,'legacy-plain',old.input),'plain-text proof and baseline must match frozen production bytes');
const good=await test('code-tool-and-terminal-text',rich());
assert.deepEqual(good.value.baseline.slice(0,-1),old.value.baseline.slice(0,-1));
for(const [name,index] of [['changed-code',2],['changed-tool-output',5],['changed-control-label',3]]){
 const fragments=rich();fragments[index].name+=' modified';const v=await test(name,fragments);
 assert.notEqual(v.value.baseline.at(-1),good.value.baseline.at(-1));
 assert.notEqual(v.value.result.taskProof.replySha256,good.value.result.taskProof.replySha256);
}
const clipped=await test('clipped-history',rich(),true,v=>{v.nodes=v.nodes.filter(n=>![0,1,2].some(i=>n.id===list+'.'+i||n.id.startsWith(list+'.'+i+'.')));});
assert.deepEqual(clipped.value.baseline,good.value.baseline.slice(-1*clipped.value.baseline.length));
await test('nested-marker-only',rich().map(n=>({...n,name:n.id===body+'.2.0'?marker:n.id===body+'.3'?'No final marker':n.name})),false);
await test('button-marker-only',rich().map(n=>({...n,name:n.id===body+'.1.1'?marker:n.id===body+'.3'?'No final marker':n.name})),false);
await test('quoted-terminal-status',rich(),false,v=>{v.nodes.find(n=>n.id===list+'.8.0.2.0.0').name='生成中';v.nodes.find(n=>n.id===body+'.2.0').name='已完成 3s\n'+marker;});
await test('trailing-tool-card',rich().concat(node('.4','Custom','tool'),node('.4.0','Text',marker)),false);
await test('missing-parent',rich().filter(n=>n.id!==body+'.1'),false);
await test('missing-sibling',rich().filter(n=>n.id!==body+'.1.0'),false);
await test('child-under-text',rich().concat(node('.3.0','Text',marker)),false);
await test('child-under-button',rich().concat(node('.1.1.0','Text',marker)),false);
await test('unsupported-editor',rich().map(n=>n.id===body+'.1'?{...n,type:'ControlType.Edit'}:n),false);
await test('duplicate-node',rich().concat(rich()[2]),false);
await test('oversized-text',rich().map(n=>n.id===body+'.1.0'?{...n,name:'a'.repeat(65537)}:n),false);
await test('cancelled-reply',rich(),false,v=>{v.nodes.find(n=>n.id===list+'.8.0.2.0.0').name='已取消';});
const nested=[node('.0','Custom')];for(let i=1;i<17;i++)nested.push(node('.0'+'.0'.repeat(i),'Custom'));
nested.push(node('.1','Text',marker));await test('excessive-depth',nested,false);
// Actual coding reply captured read-only after the single approved send. Earlier
// conversation text stays synthetic; this is regression replay, not another send.
const live=JSON.parse(await readFile('reports/workbuddy-coding-readback-1cbHap/codingcapture.json','utf8')).result;
const livePlan=JSON.parse(await readFile('tests/fixtures/workbuddy-coding-plan.json','utf8'));
async function capturedCase(name,mutate=()=>{},expected=true,verbatim=false){
 const input=structuredClone(base),root=list+'.8';
 input.prompt=livePlan.messages[0];input.marker=input.prompt.match(/CL_TASK_[0-9a-f]{32}/)[0];
 input.nodes=input.nodes.filter(n=>n.id!==root&&!n.id.startsWith(root+'.'));
 input.nodes.find(n=>n.type==='ControlType.Text'&&n.id.startsWith(list+'.7.0.0.0.')).name=input.prompt;
 input.nodes.push(...live.nodes.map(n=>({...n,id:root+n.id.slice(live.replyRoot.length)})));
 // Preserve the original malformed-plan failure as a negative fixture. All
 // positive mutation tests use a synthetic, correctly sized terminal marker.
 if(!verbatim){const tail=input.nodes.filter(n=>n.id.startsWith(root+'.0.8.')&&n.type==='ControlType.Text');tail.forEach((n,i)=>n.name=i===0?input.marker:'');}
 mutate(input,root);const value=await invoke(exe,name,input);
 assert.equal(value.ok,expected,name+': '+JSON.stringify(value));cases.push({name,passed:true,accepted:expected});return value;
}
await capturedCase('captured-original-marker-mismatch',()=>{},false,true);
const actual=await capturedCase('captured-layout-valid-marker');
for(const [name,suffix] of [['captured-code-mutated','.0.7.0.0.59'],['captured-inline-mutated','.0.4.1.0']]){
 const changed=await capturedCase(name,(v,r)=>{v.nodes.find(n=>n.id===r+suffix).name+=' changed';});
 assert.notEqual(changed.result.taskProof.replySha256,actual.result.taskProof.replySha256);
}
await capturedCase('captured-marker-only-in-code',(v,r)=>{v.nodes.find(n=>n.id===r+'.0.7.0.0.59').name=v.marker;v.nodes.find(n=>n.id===r+'.0.8.0').name='No marker';},false);
await capturedCase('captured-missing-block',(v,r)=>{v.nodes=v.nodes.filter(n=>n.id!==r+'.0.4'&&!n.id.startsWith(r+'.0.4.'));},false);
await capturedCase('captured-code-child-button',(v,r)=>{v.nodes.find(n=>n.id===r+'.0.7.0.0.59').type='ControlType.Button';},false);
await capturedCase('captured-terminal-inline-code',(v,r)=>{v.nodes.find(n=>n.id===r+'.0.8.0').type='ControlType.Edit';},false);
await capturedCase('captured-footer-changed',(v,r)=>{v.nodes.find(n=>n.id===r+'.0.10').name='Other action';},false);
await capturedCase('captured-content-after-footer',(v,r)=>{v.nodes.push({id:r+'.0.19',type:'ControlType.Custom',name:'tool'},{id:r+'.0.19.0',type:'ControlType.Text',name:v.marker});},false);
await capturedCase('captured-hidden-text-in-footer',(v,r)=>{v.nodes.find(n=>n.id===r+'.0.9.0').type='ControlType.Text';},false);
await capturedCase('captured-status-not-completed',(v,r)=>{v.nodes.find(n=>n.id===r+'.0.2.0.0').name='生成中';},false);
await capturedCase('captured-nested-quoted-marker',(v,r)=>{v.nodes.find(n=>n.id===r+'.0.8.0').type='ControlType.Custom';v.nodes.push({id:r+'.0.8.0.0',type:'ControlType.Text',name:v.marker});},false);
// Reply hashes are opaque evidence to Rust, but preserve the actual cross-language validation.
const review=good.value.review;Object.assign(review,{attempt:{id:'attempt',taskId:'task',workerId:'worker',revisionDigest:'9'.repeat(64),fence:1,binding:{adapterId:'workbuddy',adapterVersion:'fixture',sessionId:'ui-binding-'+review.bindingId,identityStrength:'ui-observation',workspace:'D:\\test'}},projectId:'project',workflowSequence:1,workflowHash:'a'.repeat(64),continuation:null});
await writeFile(join(report,'review.json'),JSON.stringify(review,null,2));
await writeFile(join(report,'success.json'),JSON.stringify(good.value.result,null,2));
const build=spawnSync('cargo',['test','-p','cerulean-core','--lib','--no-run','--locked','--message-format=json'],{encoding:'utf8',windowsHide:true,timeout:120000});assert.equal(build.status,0,build.stderr);
const rust=build.stdout.split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s)).find(v=>v.reason==='compiler-artifact'&&v.profile.test&&v.target.kind.includes('lib')&&v.executable)?.executable;assert.ok(rust);
const checked=spawnSync(rust,['--exact','desktop_execution::local_proof_tests::native_workbuddy_reply_contract','--ignored','--nocapture'],{encoding:'utf8',windowsHide:true,timeout:30000,env:{...process.env,CERULEAN_WORKBUDDY_REPLY_REPORT:report}});
await writeFile(join(report,'rust-contract.log'),checked.stdout+'\n'+checked.stderr);assert.equal(checked.status,0,checked.stdout+checked.stderr);assert.match(checked.stdout,/1 passed/);
const result={status:'passed',scope:'synthetic rich bodies plus actual captured coding reply replay; original marker mismatch retained as negative, positive marker normalized only in fixture',cases,legacyPlainProofIdentical:true,rustContractPassed:true,externalMessages:0};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,...result}));
