using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;

public static class CeruleanStandardTestProcess {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct StartupInfo {
        public uint Size;
        public string Reserved, Desktop, Title;
        public uint X, Y, Width, Height, XChars, YChars, Fill, Flags;
        public ushort Show, ReservedSize;
        public IntPtr ReservedBytes, Input, Output, Error;
    }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr Process, Thread; public uint Pid, Tid; }
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int kind);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle, uint timeout);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr handle, out uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr handle, uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr handle);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, out int value, int size, out int returned);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcessWithLogonW(
        string user, string domain, string password, uint logonFlags, string program, StringBuilder arguments,
        uint flags, IntPtr environment, string directory, ref StartupInfo startup, out ProcessInfo process);

    static void Check(bool condition, string operation) {
        if(!condition) throw new Win32Exception(Marshal.GetLastWin32Error(), operation);
    }
    public static int Run(string user, string password, string sid, string program, string directory, bool probeOnly) {
        ProcessInfo process=new ProcessInfo();
        IntPtr token=IntPtr.Zero;
        bool resumed=false;
        try {
            StartupInfo startup=new StartupInfo { Size=(uint)Marshal.SizeOf(typeof(StartupInfo)), Flags=0x100,
                Input=GetStdHandle(-10), Output=GetStdHandle(-11), Error=GetStdHandle(-12) };
            foreach(IntPtr handle in new[]{startup.Input,startup.Output,startup.Error}) {
                if(handle!=IntPtr.Zero && handle!=new IntPtr(-1))
                    Check(SetHandleInformation(handle,1,1),"Inherit owned test output");
            }
            string arguments=probeOnly
                ? "-NoProfile -NonInteractive -Command \"$p=[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()); if($p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){exit 1}; 'Ordinary test child confirmed'\""
                : "--test-threads=1";
            StringBuilder command=new StringBuilder("\""+program+"\" "+arguments);
            Check(CreateProcessWithLogonW(user,".",password,1,program,command,0x08000404,
                IntPtr.Zero,directory,ref startup,out process),"Create suspended ordinary-account tests");
            // WindowsPrincipal duplicates a primary token when checking group membership.
            // TOKEN_QUERY alone lets us read elevation but makes IsInRole fail with access denied.
            Check(OpenProcessToken(process.Process,0x0008 | 0x0002,out token),"Read native test token");
            int elevated, uiAccess, returned;
            Check(GetTokenInformation(token,20,out elevated,4,out returned),"Read test elevation");
            Check(GetTokenInformation(token,26,out uiAccess,4,out returned),"Read test UI access");
            using(WindowsIdentity identity=new WindowsIdentity(token)) {
                if(elevated!=0 || uiAccess!=0 || identity.User.Value!=sid
                    || new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator))
                    throw new InvalidOperationException("Test process must use the exact ordinary fixture account");
            }
            CeruleanCiCoreJob.AssignSuspendedChild(process.Process);
            Check(ResumeThread(process.Thread)!=0xFFFFFFFF,"Resume ordinary-account tests");
            resumed=true;
            if(WaitForSingleObject(process.Process,0xFFFFFFFF)!=0) throw new Win32Exception("Wait for desktop tests");
            uint code;
            Check(GetExitCodeProcess(process.Process,out code),"Read desktop test exit");
            return unchecked((int)code);
        } finally {
            if(process.Process!=IntPtr.Zero && !resumed) {
                TerminateProcess(process.Process,1);
                WaitForSingleObject(process.Process,5000);
            }
            if(token!=IntPtr.Zero) CloseHandle(token);
            if(process.Thread!=IntPtr.Zero) CloseHandle(process.Thread);
            if(process.Process!=IntPtr.Zero) CloseHandle(process.Process);
        }
    }
}
