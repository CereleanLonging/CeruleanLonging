import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

assert.equal(process.platform, 'win32');
assert(process.env.CERULEAN_CACHE_SESSION);
assert.equal(process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE, '1');
const temporary = mkdtempSync(join(process.env.CERULEAN_CACHE_SESSION, 'qoder-reply-boundary-'));
const harness = join(temporary, 'ReplyBoundaryProbe.cs');
writeFileSync(harness, `using System;
using System.Linq;
public partial class DesktopObserver {
    static void AssertReply(bool passed, string message) {
        if (!passed) throw new Exception(message);
    }
    static void RejectReply(Action action) {
        bool rejected = false;
        try { action(); }
        catch (InvalidOperationException error) { rejected = error.Message == "trial_history_unavailable"; }
        AssertReply(rejected, "Malformed summary boundary was accepted");
    }
    public static void CheckReplyBoundary() {
        string[] nodes = { "user", "agent", "reply", "summary", "file", "lines", "actions" };
        string[] descendants = { "file", "lines" };
        AssertReply(QoderReplyEnd(nodes, 1, 6, new int[0], null) == 6, "Ordinary reply");
        AssertReply(QoderReplyEnd(nodes, 1, 6, new[] { 3 }, descendants) == 3, "Product card boundary");
        RejectReply(() => QoderReplyEnd(nodes, 1, 6, new[] { 3, 4 }, descendants));
        RejectReply(() => QoderReplyEnd(nodes, 1, 6, new[] { 0 }, descendants));
        RejectReply(() => QoderReplyEnd(nodes, 1, 6, new[] { 6 }, descendants));
        RejectReply(() => QoderReplyEnd(nodes, 1, 6, new[] { 3 }, new[] { "file" }));
        RejectReply(() => QoderReplyEnd(nodes, 1, 6, new[] { 3 }, new[] { "file", "changed" }));
        RejectReply(() => QoderReplyEnd(nodes, 1, 6, new[] { 3 }, new[] { "lines", "file" }));
        RejectReply(() => QoderReplyEnd(nodes, 1, 6, new[] { 3 }, null));
        const string marker = "CL_TASK_0123456789abcdef0123456789abcdef";
        var old = new Round { user = new[] { "approved" }, reply = "done " + marker + "edited one file", completed = true };
        string before = Baseline(new[] { old })[0];
        old.messageText = "done " + marker;
        AssertReply(ReplyText(old).EndsWith(marker, StringComparison.Ordinal), "Task marker in message");
        AssertReply(Baseline(new[] { old })[0] == before, "Historical full-text digest compatibility");
        old.reply = "unfinished edited " + marker;
        old.messageText = "unfinished";
        AssertReply(!ReplyText(old).EndsWith(marker, StringComparison.Ordinal), "Card cannot provide task marker");
        old.messageText = null;
        AssertReply(ReplyText(old) == old.reply, "Other desktop adapter keeps its reply");
    }
}
public class ReplyBoundaryProbe {
    public static int Main() {
        DesktopObserver.CheckReplyBoundary();
        Console.WriteLine("reply-boundary-checks-passed:13");
        return 0;
    }
}
`);
const framework = join(process.env.SystemRoot, 'Microsoft.NET/Framework64/v4.0.30319');
const executable = join(temporary, 'ReplyBoundaryProbe.exe');
function run(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8', windowsHide: true, timeout: 60000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  return result.stdout;
}
run(join(framework, 'csc.exe'), ['/nologo', '/codepage:65001', '/target:exe', '/platform:x64',
  '/main:ReplyBoundaryProbe', '/r:System.Web.Extensions.dll',
  ...['UIAutomationClient', 'UIAutomationTypes', 'WindowsBase'].map(name => '/r:' + join(framework, 'WPF', name + '.dll')),
  '/out:' + executable, ...['DesktopObserver.cs', 'DesktopTrial.cs', 'DesktopStop.cs'].map(name => resolve('crates/core/native', name)), harness]);
assert.match(run(executable, []), /reply-boundary-checks-passed:13/);
console.log(JSON.stringify({ passed: true, cases: 13, nativeUiActions: 0, modelCalls: 0 }));
