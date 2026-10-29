// Captured 203 structural differences; synthetic text only, no desktop actions.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-583-coding-history-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
const harness=join(report,'Harness.cs');
await writeFile(harness,`using System;using System.IO;using System.Linq;
public partial class DesktopObserver {
 class CodingHistoryInput {public string root,document,version;public bool modern;public WorkBuddyNode[] nodes;}
 public static int TestCoding(string path){try{var v=json.Deserialize<CodingHistoryInput>(File.ReadAllText(path));
 if(v.document!=null){var r=WorkBuddyRounds(v.document,v.nodes,v.version);Console.WriteLine(json.Serialize(new{ok=true,baseline=Baseline(r),reply=r.Last().reply}));}
 else{string digest;var text=WorkBuddyRenderedReply(v.root,v.nodes,out digest,v.modern);Console.WriteLine(json.Serialize(new{ok=true,reply=text,digest=digest}));}return 0;
 }catch(InvalidOperationException e){Console.WriteLine(json.Serialize(new{ok=false,error=e.Message}));return 1;}}
}
class Harness {static int Main(string[] args){return DesktopObserver.TestCoding(args[0]);}}
`);
function compile(name,root){
 const exe=join(report,name+'.exe');
 const r=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/main:Harness','/codepage:65001','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...['DesktopObserver','DesktopTrial','DesktopStop'].map(n=>resolve(root,n+'.cs')),harness],{encoding:'utf8',windowsHide:true,timeout:30000});
 assert.equal(r.status,0,r.stdout+r.stderr);return exe;
}
const exe=compile('Current','crates/core/native');
const old=compile('Before204','reports/workbuddy-actions-201/source-archive/crates/core/native');
const fixture=JSON.parse(await readFile('tests/fixtures/workbuddy-583-coding-topology.json','utf8'));
const [historical,latest]=fixture.profiles;
const base=JSON.parse(await readFile('tests/fixtures/workbuddy-583-history-topology.json','utf8'));
const marker='CL_TASK_0123456789abcdef0123456789abcdef';
const cases=[];
function at(v,suffix){const n=v.nodes.find(n=>n.id===v.root+suffix);assert.ok(n,suffix);return n;}
async function run(binary,name,v){
 const path=join(report,name+'.json');await writeFile(path,JSON.stringify(v));
 const r=spawnSync(binary,[path],{encoding:'utf8',windowsHide:true,timeout:10000});assert.equal(r.error,undefined);
 const result=JSON.parse(r.stdout.replace(/^\uFEFF/,''));assert.equal(r.status,result.ok?0:1,r.stderr);return result;
}
async function check(name,source,change=()=>{},accepted=true){
 const v=structuredClone(source);change(v);
 const r=await run(exe,name,v);assert.equal(r.ok,accepted,name+': '+JSON.stringify(r));
 if(!accepted)assert.equal(r.error,'trial_history_unavailable');
 cases.push({name,passed:true,accepted});return r;
}
const prior=await run(old,'old-expanded',base);
assert.equal(prior.ok,true);
const same=await check('expanded-round-digests-preserved',base);assert.deepEqual(same,prior);
// Real 203 evidence: historical replies lose only the regenerate action. Its
// removal must not change the hash of the unchanged message and footer values.
const historyRoot=base.document+'.4.0.1.0.0.0.1.1.0.0.0.0.0.0.0.3.0';
function historicAction(v){
 const removed=historyRoot+'.13';
 v.nodes=v.nodes.filter(n=>n.id!==removed&&!n.id.startsWith(removed+'.')).map(n=>{
  if(!n.id.startsWith(historyRoot+'.'))return n;
  const suffix=n.id.slice(historyRoot.length+1).split('.'),index=Number(suffix[0]);
  return index>13?{...n,id:historyRoot+'.'+[index-1,...suffix.slice(1)].join('.')}:n;
 });
}
const shifted=await check('historical-action-removal-preserves-message-digests',base,historicAction);
assert.deepEqual(shifted,prior,'UI-only action removal must not invalidate unchanged historical messages');
const changedHistoric=await check('historical-footer-content-remains-bound',base,v=>{
 historicAction(v);v.nodes.find(n=>n.id===historyRoot+'.15').name+=' modified';
});
assert.equal(changedHistoric.baseline[0],prior.baseline[0]);assert.notEqual(changedHistoric.baseline[1],prior.baseline[1]);
for(const [name,v] of [['latest',latest],['historical',historical]]){
 assert.equal((await run(old,'old-rejects-'+name,v)).ok,false);
 await check('captured-'+name+'-profile',v);
}
const terminal=await check('latest-final-paragraph-marker',latest,v=>at(v,'.6.6').name=marker);assert.ok(terminal.reply.endsWith(marker));
const listMarker=await check('list-marker-is-not-terminal',latest,v=>at(v,'.5.0.0').name=marker);assert.ok(!listMarker.reply.includes(marker));
const footerMarker=await check('footer-marker-is-not-terminal',latest,v=>at(v,'.15').name=marker);assert.ok(!footerMarker.reply.includes(marker));
const latestGood=await check('latest-baseline',latest);
for(const [name,suffix] of [['list-content','.5.0.0'],['list-code','.5.0.2.0'],['footer-content','.15']]){
 const r=await check(name+'-bound-to-digest',latest,v=>at(v,suffix).name+=' changed');assert.notEqual(r.digest,latestGood.digest);
}
await check('list-disabled-for-legacy',latest,v=>v.modern=false,false);
await check('list-direct-text-rejected',latest,v=>at(v,'.5.0').type='ControlType.Text',false);
await check('list-item-parent-rejected',latest,v=>at(v,'.5').type='ControlType.Custom',false);
await check('list-interactive-button-rejected',latest,v=>at(v,'.5.0.2').type='ControlType.Button',false);
await check('list-nested-editor-rejected',latest,v=>at(v,'.5.0.2.0').type='ControlType.Edit',false);
await check('list-gap-rejected',latest,v=>v.nodes=v.nodes.filter(n=>n.id!==v.root+'.5.0.1'),false);
await check('empty-list-rejected',latest,v=>v.nodes=v.nodes.filter(n=>!n.id.startsWith(v.root+'.5.')),false);
await check('empty-list-item-rejected',latest,v=>v.nodes=v.nodes.filter(n=>!n.id.startsWith(v.root+'.5.0.')),false);
await check('terminal-list-refused',latest,v=>{v.nodes=v.nodes.filter(n=>!n.id.startsWith(v.root+'.6.'));at(v,'.6').type='ControlType.List';},false);
await check('compact-footer-date-refused',latest,v=>at(v,'.16').name='2026-10-19',false);
await check('compact-footer-extra-refused',latest,v=>v.nodes.push({id:v.root+'.17',type:'ControlType.Text',name:'extra'}),false);
await check('compact-footer-nested-refused',latest,v=>v.nodes.push({id:v.root+'.15.0',type:'ControlType.Text',name:'hidden'}),false);
await check('compact-footer-wrong-type-refused',latest,v=>at(v,'.14').type='ControlType.Custom',false);
await check('action-label-refused',latest,v=>at(v,'.11').name='unknown regenerate',false);
await check('historical-unknown-action-refused',historical,v=>at(v,'.13').name='unknown share',false);
await check('historical-footer-bound',historical,v=>at(v,'.15').name+=' changed');
// Parsing a body never authorizes a clipped history. Keep the two-round floor.
const list=base.document+'.4.0.1.0.0.0.1.1.0.0.0.0.0.0.0';
const full=structuredClone(base),fullReply=list+'.3.0';
full.nodes=full.nodes.filter(n=>!n.id.startsWith(fullReply+'.'));
full.nodes.push(...latest.nodes.map(n=>({...n,id:fullReply+n.id.slice(latest.root.length)})));
full.nodes.find(n=>n.id===fullReply+'.6.6').name=marker;
const fullGood=await check('complete-history-with-captured-coding-layout',full);
assert.equal(fullGood.baseline.length,2);assert.equal(fullGood.baseline[0],prior.baseline[0]);assert.ok(fullGood.reply.endsWith(marker));
await check('complete-history-forged-status-refused',full,v=>v.nodes.find(n=>n.id===fullReply+'.2.0.0').name='生成中',false);
const fullMutated=await check('complete-history-list-hash-bound',full,v=>v.nodes.find(n=>n.id===fullReply+'.5.0.0').name+=' changed');
assert.equal(fullMutated.baseline[0],fullGood.baseline[0]);assert.notEqual(fullMutated.baseline[1],fullGood.baseline[1]);
await check('one-complete-round-still-rejected',base,v=>{v.nodes=v.nodes.filter(n=>!n.id.startsWith(list+'.0')&&!n.id.startsWith(list+'.1'));},false);
await check('leading-assistant-and-one-round-still-rejected',base,v=>{v.nodes=v.nodes.filter(n=>!n.id.startsWith(list+'.0'));},false);
const result={status:'passed',cases,scope:fixture.scope,realMessages:0,realReconciliationAccepted:false};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({report,status:result.status,cases:cases.length}));
