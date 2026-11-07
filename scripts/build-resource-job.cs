using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

// The scheduling helper joins a private nested Job before starting the build.
// The existing cache runner continues to own process-tree shutdown and cleanup.
public static class CeruleanBuildJob {
    const ulong GiB = 1024UL * 1024 * 1024;
    static IntPtr job;
    [StructLayout(LayoutKind.Sequential)] public struct Memory {
        public uint Length, Load;
        public ulong TotalPhysical, AvailablePhysical, TotalPageFile, AvailablePageFile;
        public ulong TotalVirtual, AvailableVirtual, AvailableExtendedVirtual;
    }
    [StructLayout(LayoutKind.Sequential)] struct Basic {
        public long PerProcessTime, PerJobTime;
        public uint Flags;
        public UIntPtr MinWorkingSet, MaxWorkingSet;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)] struct Io {
        public ulong ReadCount, WriteCount, OtherCount, ReadBytes, WriteBytes, OtherBytes;
    }
    [StructLayout(LayoutKind.Sequential)] struct Limits {
        public Basic Basic;
        public Io Io;
        public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
    }
    [StructLayout(LayoutKind.Sequential)] struct Cpu { public uint Flags, Rate; }
    public sealed class Policy {
        public uint CpuPercent, PriorityClass;
        public ulong AffinityMask, MemoryLimitBytes, ReserveBytes;
        public int CargoJobs;
    }
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GlobalMemoryStatusEx(ref Memory value);
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr handle, int kind, IntPtr value, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr handle, int kind, IntPtr value, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr handle, IntPtr process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessAffinityMask(IntPtr process, out UIntPtr available, out UIntPtr system);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(IntPtr handle, uint code);
    static void Check(bool success) { if(!success) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    static void Set<T>(int kind, T value) where T:struct {
        int size=Marshal.SizeOf(typeof(T)); IntPtr p=Marshal.AllocHGlobal(size);
        try { Marshal.StructureToPtr(value,p,false); Check(SetInformationJobObject(job,kind,p,(uint)size)); }
        finally { Marshal.FreeHGlobal(p); }
    }
    static T Read<T>(int kind) where T:struct {
        int size=Marshal.SizeOf(typeof(T)); IntPtr p=Marshal.AllocHGlobal(size);
        try { Check(QueryInformationJobObject(job,kind,p,(uint)size,IntPtr.Zero)); return (T)Marshal.PtrToStructure(p,typeof(T)); }
        finally { Marshal.FreeHGlobal(p); }
    }
    public static Memory ReadMemory() {
        Memory value=new Memory(); value.Length=(uint)Marshal.SizeOf(typeof(Memory));
        Check(GlobalMemoryStatusEx(ref value)); return value;
    }
    public static ulong Budget(ulong physical, ulong commit) {
        return Budget(physical,commit,GiB);
    }
    public static ulong Budget(ulong physical, ulong commit, ulong minimum) {
        if(minimum < GiB || minimum > 3*GiB) throw new ArgumentOutOfRangeException("minimum");
        // User-authorized admission tolerance: a shortfall of at most 1 GiB
        // below (budget + reserve) does not defer startup. The running reserve
        // guard and the selected Job cap remain unchanged.
        if(physical < minimum || commit < minimum) return 0;
        // Allocate the selected, validated budget, even when more memory is free.
        return minimum;
    }
    public static Policy Configure(ulong budget) {
        if(budget < GiB || budget > 3*GiB) throw new ArgumentOutOfRangeException("budget");
        job=CreateJobObject(IntPtr.Zero,null); Check(job!=IntPtr.Zero);
        UIntPtr available,system; Check(GetProcessAffinityMask(Process.GetCurrentProcess().Handle,out available,out system));
        ulong allowed=available.ToUInt64(),mask=0;
        for(int n=0;n<2 && allowed!=0;n++) { ulong bit=allowed & (~allowed+1);mask|=bit;allowed&=~bit; }
        if(mask==0)throw new InvalidOperationException("No allowed logical processors");
        Limits limits=new Limits();
        limits.Basic.Flags=16|32|512; // AFFINITY | PRIORITY_CLASS | JOB_MEMORY
        limits.Basic.Affinity=new UIntPtr(mask);limits.Basic.PriorityClass=0x4000; // BELOW_NORMAL
        limits.JobMemory=new UIntPtr(budget);Set(9,limits);
        Cpu cpu=new Cpu();cpu.Flags=1|4;cpu.Rate=2000;Set(15,cpu); // 20% hard CPU cap
        Check(AssignProcessToJobObject(job,Process.GetCurrentProcess().Handle));
        Limits actual=Read<Limits>(9);Cpu actualCpu=Read<Cpu>(15);
        if(actual.JobMemory.ToUInt64()!=budget || actual.Basic.Affinity.ToUInt64()!=mask || actual.Basic.PriorityClass!=0x4000 || actualCpu.Flags!=5 || actualCpu.Rate!=2000)
            throw new InvalidOperationException("Build resource limit readback mismatch");
        return new Policy {CpuPercent=20,PriorityClass=actual.Basic.PriorityClass,AffinityMask=mask,MemoryLimitBytes=budget,ReserveBytes=GiB,CargoJobs=1};
    }
    // Windows retains the peak across exited descendants; sampling only live
    // process working sets would miss short compiler/linker allocation spikes.
    public static ulong PeakMemoryBytes() { return Read<Limits>(9).PeakJobMemory.ToUInt64(); }
    public static void Stop() { Check(TerminateJobObject(job,75)); }
    static string Quote(string text) {
        StringBuilder value=new StringBuilder("\"");int slashes=0;
        foreach(char c in text) {
            if(c=='\\'){slashes++;continue;}
            if(c=='\"')value.Append('\\',slashes*2+1).Append(c);
            else value.Append('\\',slashes).Append(c);
            slashes=0;
        }
        return value.Append('\\',slashes*2).Append('"').ToString();
    }
    public static Process Start(string program,string[] args,string directory) {
        var info=new ProcessStartInfo();info.FileName=program;info.WorkingDirectory=directory;
        info.UseShellExecute=false;info.CreateNoWindow=true;
        info.RedirectStandardOutput=true;info.RedirectStandardError=true;
        info.StandardOutputEncoding=Encoding.UTF8;info.StandardErrorEncoding=Encoding.UTF8;
        info.Arguments=string.Join(" ",Array.ConvertAll(args,Quote));
        Process child=new Process();child.StartInfo=info;
        child.OutputDataReceived+=(sender,eventArgs)=>{if(eventArgs.Data!=null)Console.Out.WriteLine(eventArgs.Data);};
        child.ErrorDataReceived+=(sender,eventArgs)=>{if(eventArgs.Data!=null)Console.Error.WriteLine(eventArgs.Data);};
        child.Start();child.BeginOutputReadLine();child.BeginErrorReadLine();return child;
    }
}
