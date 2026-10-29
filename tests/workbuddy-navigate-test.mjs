// A single guarded navigation into the already-existing named verification chat.
// No message input, model request, account URL read, or arbitrary button command.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/workbuddy-navigate-test-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
let source=await readFile('crates/core/native/DesktopObserver.cs','utf8');
source=source.replace('else if(request.op=="trialprepare")','else if(request.op=="workbuddyverificationchat")result=NavigateVerificationChat(request.window);\n            else if(request.op=="trialprepare")');
await writeFile(join(report,'DesktopObserver.cs'),source);
await writeFile(join(report,'Navigate.cs'),`using System;using System.Linq;using System.Threading;using System.Windows.Automation;using System.Runtime.InteropServices;
public partial class DesktopObserver {
 [StructLayout(LayoutKind.Sequential)] struct NavPoint {public int x,y;}
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(NavPoint p);
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h,uint flags);
 static AutomationElement VerificationLink(View v) {
  var texts=v.nodes.Where(n=>n.Cached.ControlType==ControlType.Text&&n.Cached.Name=="工作台接入验证"&&!n.Cached.IsOffscreen).ToArray();
  var links=v.nodes.Where(n=>n.Cached.ControlType==ControlType.Button&&!n.Cached.IsOffscreen&&n.Cached.IsEnabled&&(n.Cached.Name??"").StartsWith("工作台接入验证",StringComparison.Ordinal)&&texts.Any(t=>String.Join(".",t.GetRuntimeId()).StartsWith(String.Join(".",n.GetRuntimeId())+".",StringComparison.Ordinal))).ToArray();
  if(links.Length!=1)throw new InvalidOperationException("trial_history_unavailable");return links[0];
 }
 static object NavigateVerificationChat(Window w) {
  if(Environment.GetEnvironmentVariable("CERULEAN_NATIVE_OWNED_JOB")!="1"||w==null||w.app!="workbuddy"||w.version!="5.7.7.0")throw new InvalidOperationException("trial_app_not_enabled");
  var before=ReadView(w,true);var link=VerificationLink(before);string id=String.Join(".",link.GetRuntimeId());
  SetForegroundWindow(new IntPtr(Int64.Parse(w.hwnd)));Thread.Sleep(150);
  Foreground(new IntPtr(Int64.Parse(w.hwnd)),w);
  var current=ReadView(w,true);if(!SameView(before.observation,current.observation))throw new InvalidOperationException("trial_view_changed");
  link=VerificationLink(current);if(id!=String.Join(".",link.GetRuntimeId()))throw new InvalidOperationException("trial_view_changed");
  // This app's UIA Invoke focused the row but left the home page unchanged.
  // One physical click on the freshly resolved row, guarded against overlays.
  var rect=link.Current.BoundingRectangle;
  if(rect.IsEmpty||rect.Width<20||rect.Height<10)throw new InvalidOperationException("trial_navigation_unconfirmed");
  var point=new NavPoint{x=(int)(rect.Left+rect.Width/3),y=(int)(rect.Top+rect.Height/2)};
  var hwnd=new IntPtr(Int64.Parse(w.hwnd));
  if(GetAncestor(WindowFromPoint(point),2)!=hwnd||!SetCursorPos(point.x,point.y))throw new InvalidOperationException("trial_focus_changed");
  Foreground(hwnd,w);
  var clicks=new[]{new Input{type=0,data=new InputData{mouse=new MouseInput{flags=2}}},new Input{type=0,data=new InputData{mouse=new MouseInput{flags=4}}}};
  if(SendInput(2,clicks,Marshal.SizeOf(typeof(Input)))!=2)throw new InvalidOperationException("trial_navigation_unconfirmed");
  View after=null;bool loaded=false;
  for(int attempt=0;attempt<3;attempt++) {
   Thread.Sleep(500);after=ReadView(w,true);
   string header=after.observation.documentRuntimeId+".4.0.1.0.0.0.1.0.0.0";
   loaded=after.nodes.Any(n=>String.Join(".",n.GetRuntimeId())==header&&n.Cached.ControlType==ControlType.Text&&n.Cached.Name=="工作台接入验证"&&!n.Cached.IsOffscreen);
   if(loaded)break;
  }
  if(!loaded)throw new InvalidOperationException("trial_navigation_unconfirmed");
  return new{navigations=1,messagesSent=0,window=after.observation.window,documentRuntimeId=after.observation.documentRuntimeId,testLabels=after.nodes.Count(n=>n.Cached.ControlType==ControlType.Text&&n.Cached.Name=="工作台接入验证"&&!n.Cached.IsOffscreen),observation=after.observation};
 }
}`);
const exe=join(report,'Navigate.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'DesktopObserver.cs'),join(report,'Navigate.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs')],{windowsHide:true,encoding:'utf8',timeout:30000});assert.equal(build.status,0,build.stdout+build.stderr);
function run(request){const r=spawnSync(exe,[JSON.stringify({version:1,...request})],{windowsHide:true,encoding:'utf8',timeout:30000});assert.equal(r.error,undefined,r.error?.message);return {exitCode:r.status,response:JSON.parse(r.stdout.replace(/^\uFEFF/,''))};}
const discovery=run({op:'discover'});assert.equal(discovery.exitCode,0);const windows=discovery.response.result.windows.filter(w=>w.app==='workbuddy');assert.equal(windows.length,1);
const result=run({op:'workbuddyverificationchat',window:windows[0]});await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({report,...result}));assert.equal(result.exitCode,0,JSON.stringify(result.response));
