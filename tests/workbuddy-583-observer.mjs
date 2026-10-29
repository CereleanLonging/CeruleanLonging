// Actual UIA over an owned provider. No WorkBuddy discovery, navigation or send.
import {spawn,spawnSync} from 'node:child_process';
import {mkdtemp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-583-observer-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
let provider=await readFile('tests/fixtures/WorkBuddyStopWindow.cs','utf8');
provider=provider.slice(0,provider.indexOf('class WorkBuddyStopWindow {')).replaceAll('5.7.7.0','5.8.3.0');
provider=provider.replace('public string Value{get{','public string Value{get{File.AppendAllText(System.IO.Path.Combine(Root.Directory,"value-reads.log"),String.Join(".",Path)+"\\n");');
const providerPath=join(report,'Provider.cs');await writeFile(providerPath,provider);
const harness=join(report,'Window.cs');
await writeFile(harness,`using System;using System.Collections.Generic;using System.Diagnostics;using System.IO;using System.Linq;using System.Windows;using System.Windows.Automation;using System.Windows.Automation.Provider;using System.Windows.Controls;using System.Windows.Interop;using System.Web.Script.Serialization;
class ObserverWindow {
 class Node {public string id,type,name;}
 class Fixture {public string document;public Node[] nodes;}
 static BuddyNode Add(BuddyRoot root,BuddyNode doc,string path,int type,string name="") {
  var parent=doc;var ids=new List<int>();foreach(var value in path.Split('.')){int part=Int32.Parse(value);ids.Add(part);var next=parent.Children.FirstOrDefault(n=>n.Path.Last()==part);if(next==null){next=new BuddyNode(root,parent,ids.ToArray(),ControlType.Custom.Id);parent.Children.Add(next);parent.Children.Sort((a,b)=>a.Path.Last().CompareTo(b.Path.Last()));}parent=next;}parent.Type=type;parent.Name=name;return parent;
 }
 [STAThread] static void Main(string[] args) {
  string dir=args[0],mode=args[1];var json=new JavaScriptSerializer();var fixture=json.Deserialize<Fixture>(File.ReadAllText(args[2]));
  var app=new Application();var window=new Window{Title="Owned WorkBuddy 5.8.3 observation fixture",Width=780,Height=400,Content=new TextBlock{Text="Read-only version-delta test. No external messages."}};
  var root=new BuddyRoot{Directory=dir,DocumentSeed=5830};
  var doc=new BuddyNode(root,root,new int[0],ControlType.Document.Id){Name="WorkBuddy"};root.Children.Add(doc);
  foreach(var n in fixture.nodes)if(n.id!=fixture.document){
   if(mode=="missing-header"&&n.id==fixture.document+".4.0.1.0.0.0.1.0.0.0")continue;
   if(mode=="one-round"&&n.id.StartsWith(fixture.document+".4.0.1.0.0.0.1.1.0.0.0.0.0.0.0.2",StringComparison.Ordinal))continue;
   var type=(ControlType)typeof(ControlType).GetField(n.type.Substring(12)).GetValue(null);
   Add(root,doc,n.id.Substring(fixture.document.Length+1),type.Id,n.name);
  }
  const string composer="4.0.1.0.0.0.1.1.0.0.1.0.1.0";
  var editor=Add(root,doc,composer+".1.0",ControlType.Edit.Id);editor.ValueText=mode=="draft"?"preserve my draft":mode=="space"?" ":"";
  if(mode=="duplicate-editor")Add(root,doc,composer+".1.1",ControlType.Edit.Id);
  Add(root,doc,composer+".2.2.1.0",ControlType.ComboBox.Id,"Select model: Hy3");
  if(mode=="duplicate-model")Add(root,doc,composer+".2.2.1.1",ControlType.ComboBox.Id,"Select model: Other");
  var send=Add(root,doc,composer+".2.2.3",ControlType.Button.Id,mode=="busy"?"停止":"发送");send.Enabled=mode=="busy";
  // A sidebar far above the old whole-tree node limit must not be traversed.
  for(int i=0;i<1100;i++)Add(root,doc,"4.0.1.0.0.0.0."+i,ControlType.Text.Id,"unrelated synthetic sidebar text");
  window.SourceInitialized+=(s,e)=>{root.Hwnd=new WindowInteropHelper(window).Handle;HwndSource.FromHwnd(root.Hwnd).AddHook((IntPtr h,int m,IntPtr w,IntPtr l,ref bool handled)=>{if(m==0x003D&&l.ToInt64()==AutomationInteropProvider.RootObjectId){handled=true;return AutomationInteropProvider.ReturnRawElementProvider(h,w,l,root);}return IntPtr.Zero;});};
  window.Loaded+=(s,e)=>{root.Bounds=new Rect(window.PointToScreen(new Point(0,0)),new Size(window.ActualWidth,window.ActualHeight));root.WriteCounts();File.WriteAllText(Path.Combine(dir,"window.json"),json.Serialize(new{app="workbuddy",hwnd=root.Hwnd.ToInt64().ToString(),pid=Process.GetCurrentProcess().Id,startTicks=Process.GetCurrentProcess().StartTime.ToUniversalTime().Ticks.ToString(),exe=Process.GetCurrentProcess().MainModule.FileName,title=window.Title,version="5.8.3.0"}));};app.Run(window);
 }
}`);
function compile(name,sources,refs){const exe=join(report,name+'.exe');const r=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/codepage:65001','/r:System.Web.Extensions.dll','/r:System.Xaml.dll',...refs.map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,...sources],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(r.status,0,r.stdout+r.stderr);return exe;}
const fixture=compile('WorkBuddy',[providerPath,harness],['UIAutomationTypes','UIAutomationProvider','WindowsBase','PresentationCore','PresentationFramework']);
const bridge=compile('Observer',['DesktopObserver','DesktopTrial','DesktopStop'].map(n=>resolve('crates/core/native/'+n+'.cs')),['UIAutomationClient','UIAutomationTypes','WindowsBase']);
const cases=[];
for(const [mode,error] of [['valid',null],['draft','draft_or_composer_unavailable'],['space','draft_or_composer_unavailable'],['duplicate-editor','draft_or_composer_unavailable'],['duplicate-model',null],['busy','session_busy_or_approval_pending'],['missing-header','trial_history_unavailable'],['one-round','trial_history_unavailable']]){
 const dir=join(report,mode);await mkdir(dir);const child=spawn(fixture,[dir,mode,resolve('tests/fixtures/workbuddy-583-history-topology.json')],{windowsHide:true,stdio:['ignore','pipe','pipe']});
 let err='',exitInfo=null;child.stderr.on('data',b=>err+=b);child.stdout.resume();
 const exited=new Promise(resolve=>{child.once('exit',(code,signal)=>{exitInfo={code,signal};resolve();});child.once('error',e=>{exitInfo={error:e.message};resolve();});});
 try{
  let identity;const deadline=Date.now()+10000;while(!identity){assert.equal(exitInfo,null,err);try{identity=JSON.parse(await readFile(join(dir,'window.json'),'utf8'));}catch{assert.ok(Date.now()<deadline,err);await new Promise(r=>setTimeout(r,40));}}
  function request(op){const r=spawnSync(bridge,[JSON.stringify({version:1,op,window:identity})],{windowsHide:true,encoding:'utf8',timeout:15000});assert.equal(r.error,undefined);const body=JSON.parse(r.stdout.replace(/^\uFEFF/,''));assert.equal(r.status,body.ok?0:1);return body;}
  const observed=request('inspect');assert.equal(observed.ok,error===null,mode+': '+JSON.stringify(observed));
  if(error)assert.equal(observed.error,error);else{assert.equal(observed.result.observation.modelLabel,mode==='duplicate-model'?null:'Hy3');assert.equal(observed.result.observation.draftEmpty,true);assert.ok(observed.result.observation.nodeCount<400);}
  if(mode==='valid'){const repeat=request('inspect');for(const k of ['contentDigest','layoutDigest','nodeCount','modelLabel'])assert.deepEqual(repeat.result.observation[k],observed.result.observation[k]);const prepared=request('taskprepare');assert.equal(prepared.ok,true);assert.equal(prepared.result.baseline.length,2);}
  let reads=[];try{reads=(await readFile(join(dir,'value-reads.log'),'utf8')).trim().split('\n').filter(Boolean);}catch(e){assert.equal(e.code,'ENOENT');}
  assert.ok(reads.every(id=>id.startsWith('4.0.1.0.0.0.1.1.0.0.1.0.1.0.1.')),'reply editor values must not be read');
  if(mode==='valid')assert.equal(reads.length,3,'one composer read per observation/preparation');
  assert.deepEqual(JSON.parse(await readFile(join(dir,'mutations.json'),'utf8')),{valueChanges:0,invokes:0});
  await writeFile(join(dir,'result.json'),JSON.stringify({observed,valueReads:reads,passed:true}));cases.push({mode,passed:true,error,replyValuesRead:0});
 }finally{if(exitInfo===null)child.kill();await exited;await writeFile(join(dir,'process-exit.json'),JSON.stringify(exitInfo));}
}
await writeFile(join(report,'result.json'),JSON.stringify({status:'passed',cases,actualCrossProcessUia:true,ownedWindowsOnly:true,externalMessages:0},null,2)+'\n');
console.log(JSON.stringify({report,status:'passed',cases:cases.length}));
