// Read-only live structure evidence: known control labels only, no chat bodies or account URLs.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/desktop-capability-diagnostic-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
let source=await readFile('crates/core/native/DesktopObserver.cs','utf8');
source=source.replace('if(documents.Length!=1)throw new InvalidOperationException("document_not_unique");', 'if(documents.Length!=1){Console.WriteLine(json.Serialize(new{documentDiagnostic=true,app=expected.app,documents=documents.Length,nodes=nodes.Length}));throw new InvalidOperationException("document_not_unique");}');
const needle='if(editors!=1||(idle&&!draftEmpty))throw new InvalidOperationException("draft_or_composer_unavailable");';
assert.equal(source.split(needle).length,2);
source=source.replace(needle,`Console.WriteLine(json.Serialize(new {
    diagnostic=true, app=expected.app, sessionId=session,
    knownTestSession=session=="76edb3bc-078f-4ce4-bc9b-b6525fde9c51", editors=editors, draftEmpty=draftEmpty,
    choices=expected.app!="workbuddy"?null:nodes.Where(n=>n.Cached.ControlType==ControlType.ComboBox&&!n.Cached.IsOffscreen).Select(n=>new {
        id=String.Join(".",n.GetRuntimeId()), name=(n.Cached.Name??"").Length<=100?n.Cached.Name:null,
        labels=nodes.Where(t=>t.Cached.ControlType==ControlType.Text&&!t.Cached.IsOffscreen&&(t.Cached.Name??"").Length<=100&&String.Join(".",t.GetRuntimeId()).StartsWith(String.Join(".",n.GetRuntimeId())+".",StringComparison.Ordinal)).Select(t=>t.Cached.Name).ToArray()
    }).ToArray(),
    nodes=nodes.Select(n=>new {
        id=String.Join(".",n.GetRuntimeId()), type=n.Cached.ControlType.ProgrammaticName, visible=!n.Cached.IsOffscreen,
        enabled=n.Cached.IsEnabled, nameLength=(n.Cached.Name??"").Length,
        label=Regex.IsMatch(n.Cached.Name??"",@"^(工作台接入验证|WorkBuddy|发送|停止|发送消息|停止生成|消息列表|消息锚点|一轮任务|Agent 回复|回复操作|已完成 (?:[0-9]+m)?[0-9.]+s|模型: [^\\r\\n]{1,80}|(?:GPT|Claude|DeepSeek|deepseek|GLM|Qwen|Kimi|Auto|自动|混元|Hunyuan)[A-Za-z0-9 ._:/+\\-]{0,60})$")?n.Cached.Name:null,
        withinWorkspace=Regex.IsMatch(n.Cached.Name??"",@"^D:\\\\test(?:\\\\[^\\r\\n]{0,180})?$",RegexOptions.IgnoreCase),
        workspaceLabel=Regex.IsMatch(n.Cached.Name??"",@"^D:\\\\test(?:\\\\[^\\r\\n]{0,180})?$",RegexOptions.IgnoreCase)?n.Cached.Name:null,
        x=n.Current.BoundingRectangle.IsEmpty?0:n.Current.BoundingRectangle.X,
        y=n.Current.BoundingRectangle.IsEmpty?0:n.Current.BoundingRectangle.Y,
        width=n.Current.BoundingRectangle.IsEmpty?0:n.Current.BoundingRectangle.Width,
        height=n.Current.BoundingRectangle.IsEmpty?0:n.Current.BoundingRectangle.Height
    }).ToArray()
}));
${needle}`);
await writeFile(join(report,'DesktopObserver.cs'),source);
const exe=join(report,'CapabilityDiagnostic.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'DesktopObserver.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});
assert.equal(build.status,0,build.stdout+build.stderr);
function run(request){const r=spawnSync(exe,[JSON.stringify({version:1,...request})],{windowsHide:true,encoding:'utf8',timeout:30000});assert.equal(r.error,undefined,r.error?.message);return {exitCode:r.status,lines:r.stdout.trim().split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s.replace(/^\uFEFF/,'')))};}
const discovery=run({op:'discover'});assert.equal(discovery.exitCode,0);
const observations=discovery.lines[0].result.windows.map(window=>({window,...run({op:'inspect',window})}));
await writeFile(join(report,'result.json'),JSON.stringify({capturedAt:new Date().toISOString(),readOnly:true,externalMessages:0,observations},null,2)+'\n');
console.log(JSON.stringify({report,readOnly:true,windows:observations.map(o=>({app:o.window.app,version:o.window.version,exitCode:o.exitCode,document:o.lines.find(v=>v.documentDiagnostic),knownTestSession:o.lines.find(v=>v.diagnostic)?.knownTestSession,error:o.lines.find(v=>!v.ok&&v.error)?.error,choices:o.lines.find(v=>v.diagnostic)?.choices,labels:o.lines.find(v=>v.diagnostic)?.nodes.filter(n=>n.label||n.withinWorkspace).map(n=>({id:n.id,type:n.type,label:n.label,workspaceLabel:n.workspaceLabel,x:n.x,y:n.y}))}))}));
