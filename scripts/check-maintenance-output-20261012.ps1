$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=Join-Path $root ('reports\maintenance-output-native-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $run
$source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'maintenance-window.ps1'))
$code=[regex]::Match($source,"(?s)-TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
if(-not $code.Contains('class CeruleanMaintenanceOperation')){throw 'Product capture class missing'}
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Windows.Forms -TypeDefinition $code
$exe=Join-Path $run 'owned-output-fixture.exe'
Add-Type -OutputAssembly $exe -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Threading;
using System.Runtime.InteropServices;
public static class Fixture {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Startup {
        public int cb; public string reserved,desktop,title; public int x,y,xsize,ysize,xchars,ychars,fill,flags;
        public short show,reserved2; public IntPtr reservedPointer,input,output,error;
    }
    [StructLayout(LayoutKind.Sequential)] struct Info { public IntPtr process,thread; public int pid,tid; }
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcess(string app,System.Text.StringBuilder command,IntPtr pa,IntPtr ta,bool inherit,int flags,IntPtr env,string directory,ref Startup startup,out Info info);
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int n);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    public static int Main(string[] args) {
        Console.OutputEncoding=new System.Text.UTF8Encoding(false);
        if (args[0] == "unicode") { Console.Out.WriteLine("\u9e48\u9e55-\u4e2d\u6587-\ud83d\ude80"); Console.Error.WriteLine("\u9519\u8bef\u4fe1\u606f"); return 0; }
        if (args[0] == "child") {
            Thread.Sleep(1800);
            Console.Out.WriteLine("late-child-output");
            Console.Error.WriteLine("late-child-error");
            return 0;
        }
        if (args[0] == "parent") {
            Console.Out.WriteLine("parent-output");
            var startup=new Startup { flags=0x100,input=GetStdHandle(-10),output=GetStdHandle(-11),error=GetStdHandle(-12) };
            startup.cb=Marshal.SizeOf(startup); Info info;
            var command=new System.Text.StringBuilder("\""+typeof(Fixture).Assembly.Location+"\" child");
            if(!CreateProcess(null,command,IntPtr.Zero,IntPtr.Zero,true,0x08000000,IntPtr.Zero,null,ref startup,out info)) throw new System.ComponentModel.Win32Exception();
            Console.Out.WriteLine("child-pid=" + info.pid);
            CloseHandle(info.thread);CloseHandle(info.process);
            return 7;
        }
        if (args[0] == "large") {
            var a=System.Threading.Tasks.Task.Run(() => { for(int i=0;i<20000;i++) Console.Out.WriteLine("stdout-"+i); });
            var b=System.Threading.Tasks.Task.Run(() => { for(int i=0;i<20000;i++) Console.Error.WriteLine("stderr-"+i); });
            System.Threading.Tasks.Task.WaitAll(a,b);
            return 0;
        }
        return 2;
    }
}
'@
$results=[Collections.Generic.List[object]]::new()
foreach($mode in @('parent','large','unicode')){
    $out=Join-Path $run ($mode+'.stdout');$err=Join-Path $run ($mode+'.stderr')
    $capture=[CeruleanMaintenanceOperation]::Start($exe,$mode,$out,$err)
    $handle=$capture.Process.Handle
    if(-not $capture.Process.WaitForExit(15000)){throw 'Original fixture still running; inspect, do not repeat'}
    $pendingAtParentExit=-not $capture.Completion.IsCompleted
    if(-not $capture.Completion.Wait(15000)){throw 'Original capture still pending; inspect, do not repeat'}
    $null=$capture.Completion.GetAwaiter().GetResult()
    # Exclusive open proves our capture task closed both writers before completion.
    foreach($path in @($out,$err)){$stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None);$stream.Dispose()}
    $stdout=[IO.File]::ReadAllText($out);$stderr=[IO.File]::ReadAllText($err)
    if($mode -eq 'parent'){
        if(-not $pendingAtParentExit -or $capture.Process.ExitCode -ne 7 -or -not $stdout.Contains('late-child-output') -or -not $stderr.Contains('late-child-error')){throw 'Inherited child stream lifetime not preserved'}
    }elseif($mode -eq 'large'){
        if($capture.Process.ExitCode -ne 0 -or ($stdout.Split([char]10).Length-1) -ne 20000 -or ($stderr.Split([char]10).Length-1) -ne 20000){throw 'Concurrent streams incomplete'}
    }else{if($stdout.Trim() -cne '鹈鹕-中文-🚀' -or $stderr.Trim() -cne '错误信息'){throw 'UTF-8 native output corrupted'}}
    $results.Add(@{mode=$mode;exit=$capture.Process.ExitCode;capturePendingAtParentExit=$pendingAtParentExit;writersClosed=$true;outputBytes=(Get-Item $out).Length;errorBytes=(Get-Item $err).Length})
    $capture.Process.Dispose()
}
$badOut=Join-Path $run 'failed-start.stdout';$badErr=Join-Path $run 'failed-start.stderr';$rejected=$false
try{$null=[CeruleanMaintenanceOperation]::Start((Join-Path $run 'absent.exe'),'',$badOut,$badErr)}catch{$rejected=$true}
if(-not $rejected){throw 'Missing executable unexpectedly started'}
foreach($path in @($badOut,$badErr)){$stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None);$stream.Dispose()}
$report=@{status='passed';run=$run;productScriptSha256=(Get-FileHash (Join-Path $PSScriptRoot 'maintenance-window.ps1')).Hash.ToLowerInvariant();controlledNativeFixture=$true;actualUpgradeAccepted=$false;failedStartClosedWriters=$true;checks=$results}
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($report|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
$report|ConvertTo-Json -Depth 6 -Compress
