// Focus-only diagnostic on the named idle verification chat. No input or Invoke.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const version=process.argv[2]??'5.7.7.0';assert.ok(['5.7.7.0','5.8.3.0'].includes(version));
const observeOnly=process.argv[3]==='--observe-only';assert.ok(process.argv[3]===undefined||observeOnly);
const report=await mkdtemp(resolve('reports/workbuddy-focus-diagnostic-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
let source=await readFile('crates/core/native/DesktopObserver.cs','utf8');
const needle='else if(request.op=="trialprepare")';assert.equal(source.split(needle).length,2);
source=source.replace(needle,'else if(request.op=="focusdiagnostic")result=FocusDiagnostic(request.window);\n            '+needle);
await writeFile(join(report,'Observer.cs'),source);
await writeFile(join(report,'Focus.cs'),`using System;using System.Linq;using System.Collections.Generic;using System.Threading;using System.Windows.Automation;
public partial class DesktopObserver {
 static object FocusDiagnostic(Window w){
  var steps=new List<object>();string stage="identity";Observation before=null;
  try {
   if(w==null||w.app!="workbuddy"||w.version!="${version}"||!String.Equals(w.exe,@"D:\\workbuddy\\WorkBuddy.exe",StringComparison.OrdinalIgnoreCase))throw new InvalidOperationException("trial_app_not_enabled");
   stage="read-idle";var view=ReadView(w,true);before=view.observation;
   string header=before.documentRuntimeId+".4.0.1.0.0.0.1.0.0.0";
   if(before.modelLabel!="Hy3"||view.nodes.Count(n=>Runtime(n)==header&&n.Cached.ControlType==ControlType.Text&&n.Cached.Name=="工作台接入验证"&&!n.Cached.IsOffscreen)!=1)throw new InvalidOperationException("trial_view_changed");
   stage="history";var baseline=Baseline(TaskRounds(view));steps.Add(new{stage=stage,rounds=baseline.Length});
   stage="send-control";WorkBuddySendButton(view,false);steps.Add(new{stage=stage,ok=true});
   var hwnd=new IntPtr(Int64.Parse(w.hwnd));
   stage="foreground";SetForegroundWindow(hwnd);Thread.Sleep(100);Foreground(hwnd,w);steps.Add(new{stage=stage,ok=true});
   stage="editor-properties";steps.Add(new{stage=stage,focusable=view.editor.Current.IsKeyboardFocusable,focused=view.editor.Current.HasKeyboardFocus,enabled=view.editor.Current.IsEnabled});
   stage="composer-focus";if(!${observeOnly})FocusComposer(view,hwnd);steps.Add(new{stage=stage,ok=true,requested=!${observeOnly}});
   stage="read-after-focus";view=ReadView(w,true);steps.Add(new{stage=stage,same=SameView(view.observation,before),contentSame=view.observation.contentDigest==before.contentDigest,layoutSame=view.observation.layoutDigest==before.layoutDigest,nodesBefore=before.nodeCount,nodesAfter=view.observation.nodeCount,historySame=Baseline(TaskRounds(view)).SequenceEqual(baseline),draftEmpty=view.observation.draftEmpty,busy=view.observation.busy,modelSame=view.observation.modelLabel==before.modelLabel,documentSame=view.observation.documentRuntimeId==before.documentRuntimeId});
   stage="value-pattern";object value;if(!view.editor.TryGetCurrentPattern(ValuePattern.Pattern,out value))throw new InvalidOperationException("trial_draft_changed");steps.Add(new{stage=stage,readOnly=((ValuePattern)value).Current.IsReadOnly});
   stage="focus-identity";var focused=AutomationElement.FocusedElement;steps.Add(new{stage=stage,productionFocusAccepted=ComposerHasFocus(view),hasKeyboardFocus=view.editor.Current.HasKeyboardFocus,exact=focused!=null&&Runtime(focused)==Runtime(view.editor),editorId=Runtime(view.editor),focusedId=focused==null?null:Runtime(focused),focusedType=focused==null?null:focused.Current.ControlType.ProgrammaticName,focusedPid=focused==null?0:focused.Current.ProcessId,editorPid=view.editor.Current.ProcessId});
   var parent=focused;for(int i=0;i<8&&parent!=null;i++){steps.Add(new{stage="focus-ancestor",depth=i,id=Runtime(parent),type=parent.Current.ControlType.ProgrammaticName,pid=parent.Current.ProcessId,hwnd=parent.Current.NativeWindowHandle});parent=TreeWalker.RawViewWalker.GetParent(parent);}
   return new{status="passed",steps=steps,keyboardInputEvents=0,sendInvokes=0,focusMayClick=!${observeOnly}};
  }catch(Exception e){return new{status="failed",stage=stage,errorType=e.GetType().FullName,hresult=e.HResult,steps=steps,keyboardInputEvents=0,sendInvokes=0,focusMayClick=!${observeOnly}};}
 }
}`);
const exe=join(report,'FocusDiagnostic.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'Observer.cs'),join(report,'Focus.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(build.status,0,build.stdout+build.stderr);
function run(body){const r=spawnSync(exe,[JSON.stringify({version:1,...body})],{encoding:'utf8',windowsHide:true,timeout:30000});assert.equal(r.error,undefined);return {exitCode:r.status,response:JSON.parse(r.stdout.replace(/^\uFEFF/,''))};}
const discovery=run({op:'discover'});assert.equal(discovery.exitCode,0);const windows=discovery.response.result.windows.filter(w=>w.app==='workbuddy');assert.equal(windows.length,1);
const result=run({op:'focusdiagnostic',window:windows[0]});await writeFile(join(report,'result.json'),JSON.stringify({version,...result},null,2)+'\n');console.log(JSON.stringify({report,...result}));assert.equal(result.exitCode,0);assert.equal(result.response.result.status,'passed');
if(observeOnly)assert.equal(result.response.result.steps.find(s=>s.stage==='focus-identity').productionFocusAccepted,true);
