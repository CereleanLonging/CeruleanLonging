import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

assert.equal(process.platform, 'win32');
assert(process.env.CERULEAN_CACHE_SESSION);
assert.equal(process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE, '1');
const temporary = mkdtempSync(join(process.env.CERULEAN_CACHE_SESSION, 'desktop-draft-wait-'));
const source = readFileSync('crates/core/native/DesktopTrial.cs', 'utf8');
assert(source.includes('},QoderDraftWaitMilliseconds,100);'));
const harness = join(temporary, 'DraftWaitProbe.cs');
writeFileSync(harness, `using System;
using System.Diagnostics;
public partial class DesktopObserver {
    static void Check(bool passed, string message) {
        if (!passed) throw new Exception(message);
    }
    public static void CheckDraftWait() {
        Check(QoderDraftWaitMilliseconds == 30000, "Bounded production render wait");
        int reads = 0;
        string ready = WaitForDraft(() => { reads++; return "approved"; }, value => value == "approved", 100, 10);
        Check(ready == "approved" && reads == 1, "Immediate exact draft");

        var elapsed = Stopwatch.StartNew();
        ready = WaitForDraft(() => elapsed.ElapsedMilliseconds >= 8500 ? "approved" : "partial",
            value => value == "approved", QoderDraftWaitMilliseconds, 100);
        Check(ready == "approved" && elapsed.ElapsedMilliseconds >= 8500 && elapsed.ElapsedMilliseconds < 14000,
            "Long draft must be allowed to settle beyond the old eight-second window");

        elapsed.Restart();
        ready = WaitForDraft(() => "changed", value => value == "approved", 150, 10);
        Check(ready == "changed" && elapsed.ElapsedMilliseconds >= 150 && elapsed.ElapsedMilliseconds < 2000,
            "Mismatched draft remains rejected after a bounded wait");

        ready = WaitForDraft<string>(() => null, value => value == "approved", 50, 10);
        Check(ready == null, "Unreadable draft cannot become ready");

        reads = 0;
        bool failed = false;
        try {
            WaitForDraft<string>(() => { reads++; throw new InvalidOperationException("session_changed"); },
                value => value == "approved", 1000, 10);
        } catch (InvalidOperationException error) { failed = error.Message == "session_changed"; }
        Check(failed && reads == 1, "Identity failure must propagate without retry");

        reads = 0;
        ready = WaitForDraft(() => {
            reads++;
            if(reads==1)throw new System.Windows.Automation.ElementNotAvailableException();
            if(reads==2)throw new System.Runtime.InteropServices.COMException();
            return "approved";
        }, value => value=="approved", 1000, 10);
        Check(ready=="approved" && reads==3, "Transient UIA refresh waits without another input");
        ready=WaitForDraft<string>(()=>{throw new System.Windows.Automation.ElementNotAvailableException();},
            value=>true,50,10);
        Check(ready==null,"Unavailable controls never authorize send");
        reads=0;
        ready=WaitForDraft(()=>"approved",value=>{
            if(++reads==1)throw new InvalidOperationException("trial_history_unavailable");
            return true;
        },1000,10);
        Check(ready=="approved"&&reads==2,"Partial history can settle through read-only sampling");

        reads=0;
        object reply=WaitForReply(()=>{
            reads++;
            if(reads==1)throw new System.Windows.Automation.ElementNotAvailableException();
            if(reads==2)throw new System.Runtime.InteropServices.COMException();
            if(reads==3)throw new InvalidOperationException("trial_reply_unverified");
            if(reads==4)throw new InvalidOperationException("trial_history_unavailable");
            return "verified-original-reply";
        },1000,10);
        Check((string)reply=="verified-original-reply"&&reads==5,"Post-send reads can settle without another send");
        reads=0;
        failed=false;
        try{WaitForReply(()=>{reads++;throw new InvalidOperationException("trial_view_changed");},1000,10);}
        catch(InvalidOperationException error){failed=error.Message=="trial_view_changed";}
        Check(failed&&reads==1,"Changed review identity fails immediately");
        failed=false;elapsed.Restart();
        try{WaitForReply(()=>{throw new System.Runtime.InteropServices.COMException();},80,10);}
        catch(InvalidOperationException error){failed=error.Message=="trial_reply_unverified";}
        Check(failed&&elapsed.ElapsedMilliseconds>=80&&elapsed.ElapsedMilliseconds<2000,"Persistent read failure keeps a bounded failure");
        reads=0;failed=false;
        try{WaitForReply(()=>{reads++;throw new ArgumentException("unexpected");},1000,10);}
        catch(ArgumentException){failed=true;}
        Check(failed&&reads==1,"Unknown read errors are never accepted");
    }
}
public class DraftWaitProbe {
    public static int Main() {
        DesktopObserver.CheckDraftWait();
        Console.WriteLine("draft-wait-checks-passed:12");
        return 0;
    }
}
`);
const framework = join(process.env.SystemRoot, 'Microsoft.NET/Framework64/v4.0.30319');
const executable = join(temporary, 'DraftWaitProbe.exe');
function run(program, args, timeout) {
  const result = spawnSync(program, args, { encoding: 'utf8', windowsHide: true, timeout });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  return result.stdout;
}
run(join(framework, 'csc.exe'), ['/nologo', '/codepage:65001', '/target:exe', '/platform:x64',
  '/main:DraftWaitProbe', '/r:System.Web.Extensions.dll',
  ...['UIAutomationClient', 'UIAutomationTypes', 'WindowsBase'].map(name => '/r:' + join(framework, 'WPF', name + '.dll')),
  '/out:' + executable, ...['DesktopObserver.cs', 'DesktopTrial.cs', 'DesktopStop.cs'].map(name => resolve('crates/core/native', name)), harness], 60000);
assert.match(run(executable, [], 30000), /draft-wait-checks-passed:12/);
console.log(JSON.stringify({ passed: true, cases: 12, nativeUiActions: 0, modelCalls: 0, renderWaitMs: 30000 }));
