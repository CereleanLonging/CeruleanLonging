param([switch]$ProbeOnly)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'This test-token helper is limited to disposable GitHub-hosted runners'
}
if(-not $ProbeOnly -and (-not $env:CERULEAN_CACHE_SESSION -or $env:CERULEAN_BUILD_SCHEDULE_ACTIVE -ne '1')) {
    throw 'Desktop tests must remain inside the managed build job'
}
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;

public static class StandardDesktopTests {
    [StructLayout(LayoutKind.Sequential)] struct SidAttributes { public IntPtr Sid; public uint Attributes; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct StartupInfo {
        public uint Size;
        public string Reserved, Desktop, Title;
        public uint X, Y, Width, Height, XChars, YChars, Fill, Flags;
        public ushort Show, ReservedSize;
        public IntPtr ReservedBytes, Input, Output, Error;
    }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr Process, Thread; public uint Pid, Tid; }
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int kind);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr address);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle, uint timeout);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr handle, out uint code);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool CreateRestrictedToken(IntPtr original, uint flags,
        uint disabledCount, ref SidAttributes disabled, uint privilegeCount, IntPtr privileges,
        uint restrictedCount, IntPtr restricted, out IntPtr token);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool ConvertStringSidToSid(string text, out IntPtr sid);
    [DllImport("advapi32.dll")] static extern uint GetLengthSid(IntPtr sid);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool SetTokenInformation(IntPtr token, int kind, ref SidAttributes value, uint length);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, out int value, int size, out int returned);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcessAsUser(
        IntPtr token, string program, StringBuilder arguments, IntPtr processAttributes, IntPtr threadAttributes,
        bool inherit, uint flags, IntPtr environment, string directory, ref StartupInfo startup, out ProcessInfo process);

    static void Check(bool condition, string operation) {
        if(!condition) throw new Win32Exception(Marshal.GetLastWin32Error(), operation);
    }
    public static int Run(string cargo, string directory, bool probeOnly) {
        IntPtr original=IntPtr.Zero, token=IntPtr.Zero, admin=IntPtr.Zero, medium=IntPtr.Zero;
        ProcessInfo process=new ProcessInfo();
        try {
            Check(OpenProcessToken(GetCurrentProcess(),0x8B,out original),"Read current test token");
            Check(ConvertStringSidToSid("S-1-5-32-544",out admin),"Create administrator SID");
            SidAttributes disabled=new SidAttributes { Sid=admin };
            Check(CreateRestrictedToken(original,1,1,ref disabled,0,IntPtr.Zero,0,IntPtr.Zero,out token),"Restrict test token");
            Check(ConvertStringSidToSid("S-1-16-8192",out medium),"Create medium integrity label");
            SidAttributes label=new SidAttributes { Sid=medium, Attributes=0x20 };
            Check(SetTokenInformation(token,25,ref label,(uint)Marshal.SizeOf(typeof(SidAttributes))+GetLengthSid(medium)),"Set test integrity");
            int elevated, uiAccess, returned;
            Check(GetTokenInformation(token,20,out elevated,4,out returned),"Read test elevation");
            Check(GetTokenInformation(token,26,out uiAccess,4,out returned),"Read test UI access");
            using(WindowsIdentity identity=new WindowsIdentity(token)) {
                if(elevated!=0 || uiAccess!=0 || identity.User!=WindowsIdentity.GetCurrent().User
                    || new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator))
                    throw new InvalidOperationException("Test token must preserve the user with ordinary privileges");
            }
            Console.WriteLine("Standard desktop test token verified with medium integrity, no elevation and no UI access.");
            StartupInfo startup=new StartupInfo { Size=(uint)Marshal.SizeOf(typeof(StartupInfo)), Flags=0x100,
                Input=GetStdHandle(-10), Output=GetStdHandle(-11), Error=GetStdHandle(-12) };
            foreach(IntPtr handle in new[]{startup.Input,startup.Output,startup.Error}) {
                if(handle!=IntPtr.Zero && handle!=new IntPtr(-1))
                    Check(SetHandleInformation(handle,1,1),"Inherit owned test output");
            }
            string program=cargo;
            string arguments="test -p cerulean-desktop --features custom-protocol --locked";
            if(probeOnly) {
                program=Environment.GetFolderPath(Environment.SpecialFolder.System)+"\\WindowsPowerShell\\v1.0\\powershell.exe";
                arguments="-NoProfile -NonInteractive -Command \"$p=[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()); if($p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){exit 1}; 'Ordinary test child confirmed'\"";
            }
            StringBuilder command=new StringBuilder("\""+program+"\" "+arguments);
            Check(CreateProcessAsUser(token,program,command,IntPtr.Zero,IntPtr.Zero,true,0x08000000,
                IntPtr.Zero,directory,ref startup,out process),"Launch standard desktop tests");
            if(WaitForSingleObject(process.Process,0xFFFFFFFF)!=0) throw new Win32Exception("Wait for desktop tests");
            uint code;
            Check(GetExitCodeProcess(process.Process,out code),"Read desktop test exit");
            return unchecked((int)code);
        } finally {
            if(process.Thread!=IntPtr.Zero) CloseHandle(process.Thread);
            if(process.Process!=IntPtr.Zero) CloseHandle(process.Process);
            if(token!=IntPtr.Zero) CloseHandle(token);
            if(original!=IntPtr.Zero) CloseHandle(original);
            if(admin!=IntPtr.Zero) LocalFree(admin);
            if(medium!=IntPtr.Zero) LocalFree(medium);
        }
    }
}
'@
$root=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$cargo=(Get-Command cargo.exe -CommandType Application).Source
exit [StandardDesktopTests]::Run($cargo,$root,[bool]$ProbeOnly)
