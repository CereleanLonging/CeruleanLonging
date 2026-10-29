// Read-only production-bridge diagnostic. Never returns composer/chat/account text.
import {spawnSync} from 'node:child_process';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import assert from 'node:assert/strict';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1');
const report=await mkdtemp(resolve('reports/desktop-composer-diagnostic-'));
const framework=join(process.env.WINDIR,'Microsoft.NET/Framework64/v4.0.30319');
let source=await readFile('crates/core/native/DesktopObserver.cs','utf8');
const needle='if(editors!=1||(idle&&!draftEmpty))throw new InvalidOperationException("draft_or_composer_unavailable");';
assert.equal(source.split(needle).length,2);
source=source.replace(needle,`if(expected.app=="workbuddy") {
    foreach(var input in nodes.Where(n=>n.Cached.ControlType==ControlType.Edit)) {
        object vp; string inputValue=input.TryGetCurrentPattern(ValuePattern.Pattern,out vp)?((ValuePattern)vp).Current.Value:null;
        string inputNorm=inputValue==null?null:inputValue.Trim('\\ufeff','\\r','\\n');
        string inputName=input.Cached.Name??"";
        var rect=input.Current.BoundingRectangle;
        Console.WriteLine(json.Serialize(new { editorDiagnostic=true, visible=!input.Cached.IsOffscreen, enabled=input.Cached.IsEnabled,
            nameLength=inputName.Length, knownName=Regex.IsMatch(inputName,@"^(搜索|搜索任务|搜索历史任务|搜索历史记录|搜索聊天|搜索会话|输入消息|发送消息|今天帮你做些什么？|)$")?inputName:null,
            width=rect.IsEmpty?0:rect.Width, height=rect.IsEmpty?0:rect.Height,
            valueAvailable=inputValue!=null, valueLength=inputValue==null?-1:inputValue.Length, normalizedEmpty=inputNorm=="",
            normalizedKnownPlaceholder=inputNorm=="今天帮你做些什么？@ 添加上下文，/调用技能与指令" }));
    }
    string normalized=draftText==null?null:draftText.Trim('\\ufeff','\\r','\\n');
    Console.WriteLine(json.Serialize(new { diagnostic=true, editors=editors, nodeCount=nodes.Length, valueAvailable=draftText!=null,
        valueLength=draftText==null?-1:draftText.Length, draftEmpty=draftEmpty,
        normalizedEmpty=normalized=="", normalizedKnownPlaceholder=normalized=="今天帮你做些什么？@ 添加上下文，/调用技能与指令",
        formatCharacters=draftText==null?new int[0]:draftText.Where(c=>Char.IsControl(c)||c=='\\ufeff').Select(c=>(int)c).ToArray() }));
}
${needle}`);
await writeFile(join(report,'DesktopObserver.cs'),source);
const exe=join(report,'ComposerDiagnostic.exe');
const build=spawnSync(join(framework,'csc.exe'),['/nologo','/target:exe','/platform:x64','/r:System.Web.Extensions.dll',...['UIAutomationClient','UIAutomationTypes','WindowsBase'].map(n=>'/r:'+join(framework,'WPF',n+'.dll')),'/out:'+exe,join(report,'DesktopObserver.cs'),resolve('crates/core/native/DesktopTrial.cs'),resolve('crates/core/native/DesktopStop.cs')],{encoding:'utf8',windowsHide:true,timeout:30000});
assert.equal(build.status,0,build.stdout+build.stderr);
function run(request) {
 const result=spawnSync(exe,[JSON.stringify({version:1,...request})],{encoding:'utf8',windowsHide:true,timeout:25000});
 assert.equal(result.error,undefined,result.error?.message);
 return {exitCode:result.status,lines:result.stdout.trim().split(/\r?\n/).filter(Boolean).map(s=>JSON.parse(s.replace(/^\uFEFF/,'')))};
}
const discovery=run({op:'discover'});assert.equal(discovery.exitCode,0);
const windows=discovery.lines[0].result.windows.filter(w=>w.app==='workbuddy');
const observations=windows.map(window=>({window,...run({op:'inspect',window})}));
const result={capturedAt:new Date().toISOString(),readOnly:true,externalMessages:0,observations};
await writeFile(join(report,'result.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({report,...result}));
