using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;

// Reuses only the exact private scheduler Job containing this CI process.
// The original guard continues to enforce CPU, affinity and the system reserve.
public static class CeruleanCiCoreJob {
    const ulong GiB = 1024UL * 1024 * 1024;
    static IntPtr job;
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
    [StructLayout(LayoutKind.Sequential)] struct HandleEntry {
        public IntPtr Handle;
        public UIntPtr HandleCount, PointerCount;
        public uint Access, Type, Attributes, Reserved;
    }
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool DuplicateHandle(IntPtr source, IntPtr handle, IntPtr target, out IntPtr copy, uint access, bool inherit, uint options);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool IsProcessInJob(IntPtr process, IntPtr candidate, out bool assigned);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr handle, int kind, IntPtr value, uint size, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr handle, int kind, IntPtr value, uint size);
    [DllImport("ntdll.dll")] static extern int NtQueryInformationProcess(IntPtr process, int kind, IntPtr info, uint size, out uint returned);
    static void Check(bool success) { if(!success) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    static T Read<T>(IntPtr handle, int kind) where T:struct {
        int size=Marshal.SizeOf(typeof(T));
        IntPtr memory=Marshal.AllocHGlobal(size);
        try {
            Check(QueryInformationJobObject(handle,kind,memory,(uint)size,IntPtr.Zero));
            return (T)Marshal.PtrToStructure(memory,typeof(T));
        } finally { Marshal.FreeHGlobal(memory); }
    }
    public static void AttachOwnedGuard(uint pid) {
        if(job!=IntPtr.Zero) throw new InvalidOperationException("Already attached");
        IntPtr owner=OpenProcess(0x440,false,pid);
        Check(owner!=IntPtr.Zero);
        IntPtr buffer=Marshal.AllocHGlobal(1024*1024);
        try {
            uint returned;
            int status=NtQueryInformationProcess(owner,51,buffer,1024*1024,out returned);
            if(status!=0 || returned<16) throw new InvalidOperationException("Owned guard handles unavailable");
            long count=Marshal.ReadInt64(buffer);
            int size=Marshal.SizeOf(typeof(HandleEntry));
            if(count<1 || count>4096 || 16+count*size>returned) throw new InvalidOperationException("Invalid owned handle count");
            for(int index=0;index<count;index++) {
                var entry=(HandleEntry)Marshal.PtrToStructure(IntPtr.Add(buffer,16+index*size),typeof(HandleEntry));
                IntPtr copy;
                if(!DuplicateHandle(owner,entry.Handle,Process.GetCurrentProcess().Handle,out copy,0,false,2)) continue;
                bool keep=false;
                try {
                    bool assigned;
                    if(!IsProcessInJob(Process.GetCurrentProcess().Handle,copy,out assigned) || !assigned) continue;
                    Limits limits=Read<Limits>(copy,9);
                    Cpu cpu=Read<Cpu>(copy,15);
                    if(limits.JobMemory.ToUInt64()!=3*GiB || limits.Basic.PriorityClass!=0x4000 || cpu.Flags!=5 || cpu.Rate!=2000) continue;
                    if(job!=IntPtr.Zero) throw new InvalidOperationException("Ambiguous owned build Job");
                    job=copy;
                    keep=true;
                } finally { if(!keep) CloseHandle(copy); }
            }
            if(job==IntPtr.Zero) throw new InvalidOperationException("Exact owned 3 GiB build Job missing");
        } catch { Close(); throw; }
        finally { Marshal.FreeHGlobal(buffer); CloseHandle(owner); }
    }
    public static void ChangeMemory(ulong expected, ulong next) {
        if(!((expected==3*GiB && next==4*GiB) || (expected==4*GiB && next==3*GiB)))
            throw new InvalidOperationException("Only the authorized 3/4 GiB transition is supported");
        Limits original=Read<Limits>(job,9);
        Cpu cpu=Read<Cpu>(job,15);
        if(original.JobMemory.ToUInt64()!=expected || cpu.Flags!=5 || cpu.Rate!=2000)
            throw new InvalidOperationException("Owned policy changed");
        Limits updated=original;
        updated.JobMemory=new UIntPtr(next);
        int size=Marshal.SizeOf(typeof(Limits));
        IntPtr memory=Marshal.AllocHGlobal(size);
        try {
            Marshal.StructureToPtr(updated,memory,false);
            Check(SetInformationJobObject(job,9,memory,(uint)size));
        } finally { Marshal.FreeHGlobal(memory); }
        Limits actual=Read<Limits>(job,9);
        Cpu actualCpu=Read<Cpu>(job,15);
        if(actual.JobMemory.ToUInt64()!=next || actual.Basic.Flags!=original.Basic.Flags || actual.Basic.Affinity!=original.Basic.Affinity || actual.Basic.PriorityClass!=original.Basic.PriorityClass || actualCpu.Flags!=cpu.Flags || actualCpu.Rate!=cpu.Rate)
            throw new InvalidOperationException("Temporary policy readback mismatch");
    }
    public static ulong PeakMemoryBytes() { return Read<Limits>(job,9).PeakJobMemory.ToUInt64(); }
    public static void Close() { if(job!=IntPtr.Zero) { CloseHandle(job); job=IntPtr.Zero; } }
}
