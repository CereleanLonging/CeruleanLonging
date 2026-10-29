[CmdletBinding(DefaultParameterSetName = 'Live')]
param(
    [Parameter(Mandatory = $true)][string]$ReportDirectory,
    [Parameter(Mandatory = $true, ParameterSetName = 'Live')][string]$TestExecutable,
    [Parameter(ParameterSetName = 'Live')][string]$CodexExecutable = $env:CERULEAN_CODEX_EXE,
    [Parameter(ParameterSetName = 'Live')][string]$CodexModel = $env:CERULEAN_CODEX_MODEL,
    [Parameter(ParameterSetName = 'Live')][string]$ReadOnlyRecoverySource,
    [Parameter(Mandatory = $true, ParameterSetName = 'SelfTest')][switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$reportsRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'reports'))
$outerRoot = [IO.Path]::GetFullPath($ReportDirectory)
if (-not $outerRoot.StartsWith($reportsRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'ReportDirectory must be a fresh directory below this project reports directory.'
}
if (Test-Path -LiteralPath $outerRoot) {
    if (-not (Test-Path -LiteralPath $outerRoot -PathType Container) -or @(Get-ChildItem -LiteralPath $outerRoot -Force).Count -ne 0) {
        throw 'ReportDirectory must be empty or absent.'
    }
} else { $null = New-Item -ItemType Directory -Path $outerRoot }
$pathToInspect = Get-Item -LiteralPath $outerRoot
while ($null -ne $pathToInspect) {
    if (($pathToInspect.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Reparse paths are not supported.' }
    $pathToInspect = $pathToInspect.Parent
}

# This runner creates a suspended process, attaches it to a private kill-on-close
# job, then resumes it. Its file handles are the only inherited handles. It never
# enumerates or kills existing Codex processes and never uses a process name kill.
if (-not ('CeruleanCancelLive.OwnedRunner' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace CeruleanCancelLive {
    public sealed class Outcome {
        public bool Created, Assigned, Resumed, TimedOut, CleanupConfirmed;
        public uint ProcessId;
        public uint? ExitCode, ActiveBeforeCleanup, RemainingProcesses;
        public double ElapsedSeconds;
        public string Error, CleanupError;
    }
    public static class OwnedRunner {
        [StructLayout(LayoutKind.Sequential)] struct Security {
            public uint Length; public IntPtr Descriptor; public int Inherit;
        }
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct Startup {
            public uint cb; public string reserved, desktop, title;
            public uint x, y, xSize, ySize, xCount, yCount, fill, flags;
            public ushort show, reserved2; public IntPtr reservedBytes, stdin, stdout, stderr;
        }
        [StructLayout(LayoutKind.Sequential)] struct StartupEx { public Startup startup; public IntPtr attributes; }
        [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr process, thread; public uint pid, tid; }
        [StructLayout(LayoutKind.Sequential)] struct BasicLimit {
            public long processTime, jobTime; public uint flags;
            public UIntPtr minWorkingSet, maxWorkingSet; public uint activeLimit;
            public UIntPtr affinity; public uint priority, scheduling;
        }
        [StructLayout(LayoutKind.Sequential)] struct IoCounters { public ulong a,b,c,d,e,f; }
        [StructLayout(LayoutKind.Sequential)] struct ExtendedLimit {
            public BasicLimit basic; public IoCounters io;
            public UIntPtr processMemory, jobMemory, peakProcessMemory, peakJobMemory;
        }
        [StructLayout(LayoutKind.Sequential)] struct Accounting {
            public long user, kernel, periodUser, periodKernel;
            public uint faults, total, active, terminated;
        }
        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool SetInformationJobObject(IntPtr job, int kind, ref ExtendedLimit info, uint length);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool QueryInformationJobObject(IntPtr job, int kind, out Accounting info, uint length, IntPtr returned);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool TerminateJobObject(IntPtr job, uint exitCode);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool TerminateProcess(IntPtr process, uint exitCode);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        static extern IntPtr CreateFileW(string path, uint access, uint share, ref Security security, uint disposition, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, uint flags, ref IntPtr size);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr key, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
        [DllImport("kernel32.dll")]
        static extern void DeleteProcThreadAttributeList(IntPtr list);
        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        static extern bool CreateProcessW(string application, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes,
            bool inherit, uint flags, IntPtr environment, string directory, ref StartupEx startup, out ProcessInfo info);
        static Exception Error(string action) { return new Win32Exception(Marshal.GetLastWin32Error(), action); }
        static void Check(bool ok, string action) { if (!ok) throw Error(action); }
        static uint Active(IntPtr job) {
            Accounting info;
            Check(QueryInformationJobObject(job, 1, out info, (uint)Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero), "Query owned job");
            return info.active;
        }
        static IntPtr FileHandle(string path, bool input) {
            var security = new Security { Length=(uint)Marshal.SizeOf(typeof(Security)), Inherit=1 };
            IntPtr handle = CreateFileW(path, input ? 0x80000000u : 0x40000000u, 1u | 2u, ref security, input ? 3u : 1u, 0x80, IntPtr.Zero);
            if (handle == new IntPtr(-1)) throw Error("Open owned process stream");
            return handle;
        }
        static string Quote(string text) {
            if (text.IndexOf('\0') >= 0) throw new ArgumentException("NUL command argument");
            var result = new StringBuilder("\""); int slashes = 0;
            foreach (char c in text) {
                if (c == '\\') { slashes++; continue; }
                result.Append('\\', c == '"' ? slashes * 2 + 1 : slashes);
                result.Append(c); slashes = 0;
            }
            result.Append('\\', slashes * 2); return result.Append('"').ToString();
        }
        public static Outcome Run(string exe, string[] args, string cwd, string stdout, string stderr, int timeoutSeconds, bool injectFailure) {
            if (timeoutSeconds < 1 || timeoutSeconds > 210) throw new ArgumentOutOfRangeException("timeoutSeconds");
            var result = new Outcome(); var clock = Stopwatch.StartNew();
            IntPtr job=IntPtr.Zero, list=IntPtr.Zero, handleArray=IntPtr.Zero;
            IntPtr input=IntPtr.Zero, output=IntPtr.Zero, error=IntPtr.Zero;
            bool attributesInitialized=false; ProcessInfo process = new ProcessInfo();
            try {
                job=CreateJobObjectW(IntPtr.Zero, null);
                if (job == IntPtr.Zero) throw Error("Create private job");
                var limits = new ExtendedLimit(); limits.basic.flags=0x2000;
                Check(SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf(typeof(ExtendedLimit))), "Set kill-on-close ownership");
                input=FileHandle("NUL", true); output=FileHandle(stdout, false); error=FileHandle(stderr, false);
                IntPtr size=IntPtr.Zero;
                InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
                if (size == IntPtr.Zero) throw Error("Size process attributes");
                list=Marshal.AllocHGlobal(size);
                Check(InitializeProcThreadAttributeList(list, 1, 0, ref size), "Initialize process attributes");
                attributesInitialized=true;
                handleArray=Marshal.AllocHGlobal(IntPtr.Size * 3);
                Marshal.WriteIntPtr(handleArray, 0, input); Marshal.WriteIntPtr(handleArray, IntPtr.Size, output); Marshal.WriteIntPtr(handleArray, IntPtr.Size * 2, error);
                Check(UpdateProcThreadAttribute(list, 0, new IntPtr(0x20002), handleArray, new IntPtr(IntPtr.Size * 3), IntPtr.Zero, IntPtr.Zero), "Restrict inherited handles");
                var startup = new StartupEx(); startup.startup.cb=(uint)Marshal.SizeOf(typeof(StartupEx));
                startup.startup.flags=0x100; startup.startup.stdin=input; startup.startup.stdout=output; startup.startup.stderr=error; startup.attributes=list;
                var command = new StringBuilder(Quote(exe));
                foreach (string argument in args) command.Append(' ').Append(Quote(argument));
                if (command.Length >= 32767) throw new ArgumentException("Command line too long");
                Check(CreateProcessW(exe, command, IntPtr.Zero, IntPtr.Zero, true, 0x08000000u | 0x00080000u | 0x4u,
                    IntPtr.Zero, cwd, ref startup, out process), "Create suspended owned process");
                result.Created=true; result.ProcessId=process.pid;
                Check(AssignProcessToJobObject(job, process.process), "Assign suspended process to private job");
                result.Assigned=true;
                if (ResumeThread(process.thread) == UInt32.MaxValue) throw Error("Resume owned process");
                result.Resumed=true;
                if (injectFailure) {
                    // Wait until the self-test child has actually joined this job.
                    while ((Active(job) < 2 || new FileInfo(stdout).Length == 0) && clock.Elapsed.TotalSeconds < 2) Thread.Sleep(20);
                    if (new FileInfo(stdout).Length == 0) throw new InvalidOperationException("Self-test child did not report startup");
                    throw new InvalidOperationException("Intentional ownership self-test exception");
                }
                while (true) {
                    uint wait=WaitForSingleObject(process.process, 50);
                    if (wait == 0) {
                        uint code; Check(GetExitCodeProcess(process.process, out code), "Read owned process exit"); result.ExitCode=code; break;
                    }
                    if (wait == UInt32.MaxValue) throw Error("Wait for owned process");
                    if (clock.Elapsed.TotalSeconds >= timeoutSeconds) { result.TimedOut=true; break; }
                }
            } catch (Exception e) { result.Error=e.Message; }
            finally {
                try {
                    // If assignment failed, this suspended process does not belong
                    // to the job. Close that exact ownership gap before job queries.
                    if (process.process != IntPtr.Zero && !result.Assigned)
                        Check(TerminateProcess(process.process, 137), "Terminate unassigned suspended owned process");
                    if (job != IntPtr.Zero) {
                        result.ActiveBeforeCleanup=Active(job);
                        Check(TerminateJobObject(job, 137), "Terminate only owned job descendants");
                        var cleanup=Stopwatch.StartNew();
                        while (Active(job) != 0 && cleanup.Elapsed.TotalSeconds < 8) Thread.Sleep(20);
                        result.RemainingProcesses=Active(job);
                    }
                    if (process.process != IntPtr.Zero) {
                        // Assignment failure leaves an unexecuted suspended process;
                        // terminate its exact owned handle, never search for a PID/name.
                        uint wait=WaitForSingleObject(process.process, 2000);
                        if (wait != 0) throw new InvalidOperationException("Owned root process did not exit during cleanup");
                        uint code; Check(GetExitCodeProcess(process.process, out code), "Read final owned process exit");
                        if (!result.ExitCode.HasValue) result.ExitCode=code;
                    }
                    result.CleanupConfirmed=result.RemainingProcesses == 0;
                } catch (Exception e) { result.CleanupError=e.Message; }
                // Even if explicit termination or observation failed, closing our
                // non-inherited job handle applies KILL_ON_JOB_CLOSE to this tree.
                if (process.thread != IntPtr.Zero) CloseHandle(process.thread);
                if (process.process != IntPtr.Zero) CloseHandle(process.process);
                if (job != IntPtr.Zero) CloseHandle(job);
                if (input != IntPtr.Zero) CloseHandle(input);
                if (output != IntPtr.Zero) CloseHandle(output);
                if (error != IntPtr.Zero) CloseHandle(error);
                if (attributesInitialized) DeleteProcThreadAttributeList(list);
                if (list != IntPtr.Zero) Marshal.FreeHGlobal(list);
                if (handleArray != IntPtr.Zero) Marshal.FreeHGlobal(handleArray);
                result.ElapsedSeconds=clock.Elapsed.TotalSeconds;
            }
            return result;
        }
    }
}
'@
}

function Save-Json([string]$Path, $Value) {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 12))
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
}

if ($SelfTest) {
    # This helper performs no model calls. Both modes start a lingering child;
    # the normal parent exits, the timeout/exception parent stays alive.
    $helper = Join-Path $outerRoot 'owned-tree-selftest.exe'
    Add-Type -OutputAssembly $helper -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Reflection;
using System.Threading;
class OwnedTreeSelfTest {
    public static int Main(string[] args) {
        if (args.Length > 0 && args[0] == "child") { Thread.Sleep(60000); return 0; }
        var info = new ProcessStartInfo(Assembly.GetExecutingAssembly().Location, "child");
        info.UseShellExecute=false; info.CreateNoWindow=true;
        using (var child = Process.Start(info)) {
            Console.WriteLine("ownedChild=" + child.Id);
            Console.Error.WriteLine("self-test stderr");
            if (args.Length > 0 && args[0] != "normal") Thread.Sleep(60000);
        }
        return 0;
    }
}
'@
    $outcomes = @()
    foreach ($mode in @('normal', 'timeout', 'exception')) {
        $outcome = [CeruleanCancelLive.OwnedRunner]::Run($helper, @($mode), $outerRoot,
            (Join-Path $outerRoot "$mode.stdout.log"), (Join-Path $outerRoot "$mode.stderr.log"), 2, ($mode -eq 'exception'))
        $outcomes += [pscustomobject]@{ mode = $mode; outcome = $outcome }
    }
    $valid = @($outcomes | Where-Object { -not $_.outcome.CleanupConfirmed -or $_.outcome.RemainingProcesses -ne 0 -or -not $_.outcome.Assigned }).Count -eq 0
    $valid = $valid -and $outcomes[0].outcome.ExitCode -eq 0 -and $outcomes[1].outcome.TimedOut -and $outcomes[2].outcome.Error -like 'Intentional*'
    foreach ($mode in @('normal', 'timeout', 'exception')) {
        $valid = $valid -and (Get-Content -LiteralPath (Join-Path $outerRoot "$mode.stdout.log") -Raw) -match 'ownedChild=\d+'
        $valid = $valid -and (Get-Content -LiteralPath (Join-Path $outerRoot "$mode.stderr.log") -Raw) -match 'self-test stderr'
    }
    $summary = [ordered]@{ status = $(if ($valid) { 'passed' } else { 'failed' }); scope = 'model-free Windows Job Object ownership'; modelCalls = 0; outcomes = $outcomes }
    Save-Json (Join-Path $outerRoot 'cleanup.json') $summary
    $summary | ConvertTo-Json -Depth 12
    if (-not $valid) { throw 'Owned-process cleanup self-test failed; inspect cleanup.json.' }
    return
}

if ([string]::IsNullOrWhiteSpace($CodexExecutable) -or [string]::IsNullOrWhiteSpace($CodexModel)) { throw 'Explicit Codex executable and model required.' }
$testPath = (Get-Item -LiteralPath $TestExecutable).FullName
$codexPath = (Get-Item -LiteralPath $CodexExecutable).FullName
if (-not [IO.File]::Exists($testPath) -or -not [IO.File]::Exists($codexPath)) { throw 'Native executable paths must exist.' }
if ([IO.Path]::GetExtension($testPath) -ne '.exe' -or [IO.Path]::GetExtension($codexPath) -ne '.exe') { throw 'Only native executable files are supported.' }
$childRoot = Join-Path $outerRoot 'test'
$null = New-Item -ItemType Directory -Path $childRoot
$environmentNames = @('CERULEAN_CANCEL_LIVE_DIR', 'CERULEAN_CODEX_EXE', 'CERULEAN_CODEX_MODEL', 'CERULEAN_CANCEL_LIVE_OWNED_JOB', 'CERULEAN_CANCEL_RECOVERY_SOURCE')
$savedEnvironment = @{}
foreach ($name in $environmentNames) { $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    [Environment]::SetEnvironmentVariable('CERULEAN_CANCEL_LIVE_DIR', $childRoot, 'Process')
    [Environment]::SetEnvironmentVariable('CERULEAN_CODEX_EXE', $codexPath, 'Process')
    [Environment]::SetEnvironmentVariable('CERULEAN_CODEX_MODEL', $CodexModel, 'Process')
    [Environment]::SetEnvironmentVariable('CERULEAN_CANCEL_LIVE_OWNED_JOB', '1', 'Process')
    $testName = 'host::codex_cancel_live::live_codex_cancel_once_and_reopen'
    if ($ReadOnlyRecoverySource) {
        [Environment]::SetEnvironmentVariable('CERULEAN_CANCEL_RECOVERY_SOURCE', $ReadOnlyRecoverySource, 'Process')
        $testName = 'host::codex_early_cancel_live::recover_original_empty_interruption_readonly'
    }
    $outcome = [CeruleanCancelLive.OwnedRunner]::Run($testPath, @('--exact', $testName, '--ignored', '--nocapture', '--test-threads=1'),
        $projectRoot, (Join-Path $outerRoot 'stdout.log'), (Join-Path $outerRoot 'stderr.log'), 210, $false)
} finally {
    foreach ($name in $environmentNames) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process') }
}
$summary = [ordered]@{ at = [DateTime]::UtcNow.ToString('o'); scope = 'one exact live cancellation test in private Windows Job Object'; testName = $testName; outcome = $outcome; reportDirectory = $childRoot }
if (Test-Path -LiteralPath (Join-Path $childRoot 'result.json')) {
    try { $summary['testResultStatus'] = (Get-Content -LiteralPath (Join-Path $childRoot 'result.json') -Raw | ConvertFrom-Json).status }
    catch { $summary['testResultReadError'] = $_.Exception.Message }
}
Save-Json (Join-Path $outerRoot 'cleanup.json') $summary
$summary | ConvertTo-Json -Depth 12
if (-not $outcome.CleanupConfirmed -or $outcome.TimedOut -or $outcome.Error -or $outcome.ExitCode -ne 0) {
    throw 'Owned live test failed or timed out; inspect cleanup.json and captured output. No retry was sent.'
}
