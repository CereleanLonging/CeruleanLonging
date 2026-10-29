// Production parser tested with sanitized real topology and live read-only preparation.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-history-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
const harness=join(report,'HistoryHarness.cs');
await writeFile(harness,`using System;using System.IO;using System.Web.Script.Serialization;
public partial class DesktopObserver {
 class HistoryInput {public string document;public WorkBuddyNode[] nodes;}
 public static int TestHistory(string path){try{var input=json.Deserialize<HistoryInput>(File.ReadAllText(path));var rounds=WorkBuddyRounds(input.document,input.nodes);Console.WriteLine(json.Serialize(new{ok=true,baseline=Baseline(rounds)}));return 0;}catch(InvalidOperationException e){Console.WriteLine(json.Serialize(new{ok=false,error=e.Message}));return 1;}}
}
class HistoryHarness {static int Main(string[] args){return DesktopObserver.TestHistory(args[0]);}}
`);
function compile(name,extra=[],main=null){const exe=join(report,name+'.exe');const b=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),...(main?['/main:'+main]:[]),'/out:'+exe,...['DesktopObserver','DesktopTrial','DesktopStop'].map(n=>resolve('crates/core/native/'+n+'.cs')),...extra],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(b.status,0,b.stdout+b.stderr);return exe;}
const testExe=compile('HistoryTests',[harness],'HistoryHarness');const bridge=compile('Observer');
const captured=JSON.parse(await readFile('reports/desktop-capability-diagnostic-dC6xvT/result.json','utf8'));
const diagnostic=captured.observations.find(o=>o.window.app==='workbuddy').lines.find(l=>l.diagnostic);
const document=diagnostic.nodes.find(n=>n.type==='ControlType.Document').id;
const panel=document+'.4.0.1.0.0.0.1',list=panel+'.1.0.0.0.0.0.0';
// Preserve observed control topology only. Replace every non-control text with generated test content.
const nodes=diagnostic.nodes.map(n=>({id:n.id,type:n.type,name:n.label??(n.type==='ControlType.Text'?'synthetic text': '')}));
for(const n of nodes){if(n.id.endsWith('.0.0.0.0.0.0.0')&&n.id.startsWith(list+'.'))n.name='synthetic user '+n.id.split('.').at(-8);}
const base={document,nodes};
async function test(name,mutate,expected,compare){const input=structuredClone(base);mutate(input);const path=join(report,name+'.json');await writeFile(path,JSON.stringify(input));const run=spawnSync(testExe,[path],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(run.error,undefined);const response=JSON.parse(run.stdout);assert.equal(response.ok,expected,name+': '+run.stdout);assert.equal(run.status,expected?0:1);if(expected){assert.ok(response.baseline.length>=2);if(compare)compare(response);}else assert.equal(response.error,'trial_history_unavailable');cases.push({name,passed:true,accepted:expected,rounds:response.baseline?.length??0});return response;}
const cases=[];const good=await test('orphan-prefix',()=>{},true,r=>assert.equal(r.baseline.length,4));
const drop=(v,p)=>{v.nodes=v.nodes.filter(n=>n.id!==p&&!n.id.startsWith(p+'.'));};
await test('two-round-suffix',v=>{for(const i of [0,1,2,3,4])drop(v,list+'.'+i);},true,r=>assert.deepEqual(r.baseline,good.baseline.slice(-2)));
await test('changed-user',v=>{v.nodes.find(n=>n.type==='ControlType.Text'&&n.id.startsWith(list+'.7.0.0.0.')).name+=' modified';},true,r=>assert.notEqual(r.baseline.at(-1),good.baseline.at(-1)));
await test('changed-reply',v=>{v.nodes.find(n=>n.id===list+'.8.0.3.0').name+=' modified';},true,r=>assert.notEqual(r.baseline.at(-1),good.baseline.at(-1)));
await test('cancelled-round',v=>{v.nodes.find(n=>n.id===list+'.8.0.2.0.0').name='已取消';},true,r=>assert.notEqual(r.baseline.at(-1),good.baseline.at(-1)));
await test('one-round',v=>{for(const i of [0,1,2,3,4,5,6])drop(v,list+'.'+i);},false);
await test('missing-user',v=>drop(v,list+'.7'),false);
await test('wrong-role',v=>{v.nodes.find(n=>n.id===list+'.8.0.1').name='Other';},false);
await test('quoted-completion-only',v=>{v.nodes.find(n=>n.id===list+'.8.0.2.0.0').name='生成中';v.nodes.find(n=>n.id===list+'.8.0.3.0').name='已完成 3s';},false);
await test('multiple-status',v=>v.nodes.push({id:list+'.8.0.2.1',type:'ControlType.Text',name:'已完成 2s'}),false);
await test('multiple-user-texts',v=>v.nodes.push({id:list+'.7.0.0.0.99',type:'ControlType.Text',name:'second prompt'}),false);
await test('fragment-gap',v=>drop(v,list+'.8.0.3.1'),false);
await test('nested-reply-control',v=>v.nodes.push({id:list+'.8.0.3.0.0',type:'ControlType.Text',name:'quote'}),false);
await test('duplicate-runtime-id',v=>v.nodes.push({...v.nodes[0]}),false);
await test('missing-header',v=>drop(v,panel+'.0.0.0'),false);
await test('wrong-list-type',v=>{v.nodes.find(n=>n.id===list).type='ControlType.Text';},false);
await test('trailing-user',v=>drop(v,list+'.8'),false);
await test('empty-reply',v=>{for(const n of v.nodes)if(n.id.startsWith(list+'.8.0.3.'))n.name='';},false);
await test('wrong-status-container',v=>{v.nodes.find(n=>n.id===list+'.8.0.2').type='ControlType.Button';},false);
await writeFile(join(report,'parser-result.json'),JSON.stringify({status:'passed',scope:'production parser with sanitized captured topology; not a live UIA observation',cases},null,2)+'\n');
if(process.env.CERULEAN_WORKBUDDY_HISTORY_LIVE!=='1') {
 console.log(JSON.stringify({report,status:'passed',scope:'parser-only',cases:cases.length,liveTested:false,messagesSent:0}));
} else {
function live(request){const r=spawnSync(bridge,[JSON.stringify({version:1,...request})],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(r.error,undefined);return {exitCode:r.status,response:JSON.parse(r.stdout.replace(/^\uFEFF/,''))};}
const windows=live({op:'discover'});assert.equal(windows.exitCode,0);
const candidates=windows.response.result.windows.filter(w=>w.app==='workbuddy');assert.equal(candidates.length,1);
const window=candidates[0];assert.equal(window.version,'5.7.7.0');
const first=live({op:'taskprepare',window});await writeFile(join(report,'live-first.json'),JSON.stringify(first,null,2));assert.equal(first.exitCode,0,JSON.stringify(first));
const second=live({op:'taskprepare',window});await writeFile(join(report,'live-second.json'),JSON.stringify(second,null,2));assert.equal(second.exitCode,0,JSON.stringify(second));
assert.deepEqual(second.response.result.baseline,first.response.result.baseline);assert.ok(first.response.result.baseline.length>=2);assert.equal(first.response.result.observation.sessionId,null);assert.equal(first.response.result.observation.modelLabel,'Hy3');
for(const field of ['window','documentRuntimeId','sessionId','contentDigest','layoutDigest','modelLabel','nodeCount','draftEmpty','busy'])assert.deepEqual(second.response.result.observation[field],first.response.result.observation[field],field+' changed between reads');
const continued=live({op:'taskprepare',window,taskReview:{}});assert.equal(continued.exitCode,1);assert.equal(continued.response.error,'trial_review_invalid');
await writeFile(join(report,'continuation-rejected.json'),JSON.stringify(continued,null,2)+'\n');
const result={status:'passed',cases,liveWindow:window,liveRounds:first.response.result.baseline.length,repeatedReadMatches:true,repeatedObservationMatches:true,liveObservation:first.response.result.observation,baseline:first.response.result.baseline,invalidContinuationRejected:true,readOnly:true,messagesSent:0,realModelCalls:0};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,status:'passed',cases:cases.length,liveRounds:result.liveRounds,messagesSent:0}));
}
