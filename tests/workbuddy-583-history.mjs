// Parse sanitized captured topology with the production parser. No desktop input.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-583-history-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
const harness=join(report,'Harness.cs');
await writeFile(harness,`using System;using System.IO;using System.Linq;
public partial class DesktopObserver {
 class HistoryInput {public string document,version;public WorkBuddyNode[] nodes;}
 public static int RunVersionHistory(string path){try{var v=json.Deserialize<HistoryInput>(File.ReadAllText(path));var r=WorkBuddyRounds(v.document,v.nodes,v.version);Console.WriteLine(json.Serialize(new{ok=true,baseline=Baseline(r),reply=r.Last().reply}));return 0;}catch(InvalidOperationException e){Console.WriteLine(json.Serialize(new{ok=false,error=e.Message}));return 1;}}
}
class Harness {static int Main(string[] args){return DesktopObserver.RunVersionHistory(args[0]);}}
`);
const exe=join(report,'History.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/main:Harness','/codepage:65001','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...['DesktopObserver','DesktopTrial','DesktopStop'].map(n=>resolve('crates/core/native/'+n+'.cs')),harness],{windowsHide:true,encoding:'utf8',timeout:30000});
assert.equal(build.status,0,build.stdout+build.stderr);
const base=JSON.parse(await readFile('tests/fixtures/workbuddy-583-history-topology.json','utf8'));
const list=base.document+'.4.0.1.0.0.0.1.1.0.0.0.0.0.0.0',reply=list+'.3.0';
const cases=[];
const node=(v,id)=>{const n=v.nodes.find(n=>n.id===id);assert.ok(n,id);return n;};
const drop=(v,id)=>{v.nodes=v.nodes.filter(n=>n.id!==id&&!n.id.startsWith(id+'.'));};
async function check(name,mutate,expected=true){
 const v=structuredClone(base);mutate(v);const path=join(report,name+'.json');await writeFile(path,JSON.stringify(v));
 const r=spawnSync(exe,[path],{windowsHide:true,encoding:'utf8',timeout:10000});assert.equal(r.error,undefined);
 const result=JSON.parse(r.stdout.replace(/^\uFEFF/,''));assert.equal(result.ok,expected,name+': '+r.stdout);assert.equal(r.status,expected?0:1);
 if(!expected)assert.equal(result.error,'trial_history_unavailable');
 cases.push({name,passed:true,accepted:expected});return result;
}
const good=await check('captured-two-rounds',()=>{});assert.equal(good.baseline.length,2);assert.ok(good.reply.endsWith('TERMINAL_BODY'));
await check('unsupported-version',v=>{v.version='5.8.4.0';},false);
await check('legacy-version-does-not-accept-new-topology',v=>{v.version='5.7.7.0';},false);
await check('missing-wrapper',v=>{v.nodes=v.nodes.filter(n=>n.id!==list);},false);
await check('old-regenerate-label-refused',v=>{node(v,reply+'.13').name='点击将重新生成内容，此会话后的内容都将被覆盖，请谨慎操作';},false);
await check('missing-footer-field',v=>drop(v,reply+'.16'),false);
await check('extra-content-after-footer',v=>v.nodes.push({id:reply+'.21',type:'ControlType.Text',name:'unapproved extra'}),false);
await check('nested-footer-content',v=>v.nodes.push({id:reply+'.16.0',type:'ControlType.Text',name:'hidden'}),false);
await check('footer-wrong-type',v=>{node(v,reply+'.16').type='ControlType.Edit';},false);
await check('empty-footer-field',v=>{node(v,reply+'.17').name='';},false);
for(const [name,suffix] of [['new-footer-bound','.16'],['date-footer-bound','.20'],['inline-code-bound','.4.1.0'],['code-block-bound','.7.0.0.59']]){
 const changed=await check(name,v=>{node(v,reply+suffix).name+=' changed';});assert.notEqual(changed.baseline[1],good.baseline[1]);assert.equal(changed.baseline[0],good.baseline[0]);
}
const footerMarker=await check('footer-is-not-terminal-prose',v=>{node(v,reply+'.16').name='FORGED_MARKER';});assert.ok(!footerMarker.reply.includes('FORGED_MARKER'));
const codeMarker=await check('code-is-not-terminal-prose',v=>{node(v,reply+'.7.0.0.59').name='FORGED_MARKER';});assert.ok(!codeMarker.reply.includes('FORGED_MARKER'));
await check('terminal-inline-code-refused',v=>{node(v,reply+'.8.0').type='ControlType.Edit';},false);
await check('wrong-role',v=>{node(v,reply+'.1').name='Other';},false);
await check('quoted-completion-refused',v=>{node(v,reply+'.2.0.0').name='生成中';node(v,reply+'.8.0').name='已完成 3s';},false);
await check('duplicate-status',v=>v.nodes.push({id:reply+'.2.1',type:'ControlType.Text',name:'已完成 3s'}),false);
await check('duplicate-id',v=>v.nodes.push({...v.nodes[0]}),false);
await check('missing-user',v=>drop(v,list+'.2'),false);
await check('extra-user-text',v=>v.nodes.push({id:list+'.2.0.0.0.99',type:'ControlType.Text',name:'second prompt'}),false);
await check('empty-header',v=>{node(v,base.document+'.4.0.1.0.0.0.1.0.0.0').name='';},false);
await check('wrong-action-icon',v=>{node(v,reply+'.9.0').type='ControlType.Text';},false);
await check('trailing-user',v=>drop(v,list+'.3'),false);
const result={status:'passed',cases,scope:'Captured 5.8.3 topology with synthetic message text; no live sends',externalMessages:0};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,status:result.status,cases:cases.length}));
