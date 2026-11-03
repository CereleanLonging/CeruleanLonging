param(
    [Parameter(Mandatory=$true)][string]$PackageDirectory,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ManifestSha256,
    [switch]$AllowUnsignedInternal,
    [string]$PublisherThumbprint,
    [switch]$RetainPins
)
$ErrorActionPreference='Stop'
$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
# Review only: no installation, data directory, network, model, or service startup.
# A reviewed digest identifies bytes; it does not establish publisher identity.
if (-not ('CeruleanPackagePin' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class CeruleanPackagePin {
    [StructLayout(LayoutKind.Sequential)] struct Info {
        public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFileW(string name,uint access,uint share,IntPtr sa,uint disposition,uint flags,IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle h,out Info info);
    public static FileStream Open(string path,bool directory) {
        var h=CreateFileW(path,0x80000000,directory?3u:1u,IntPtr.Zero,3,0x02200000,IntPtr.Zero);
        if(h.IsInvalid) {h.Dispose();throw new IOException("Cannot pin package path",Marshal.GetLastWin32Error());}
        Info i;
        if(!GetFileInformationByHandle(h,out i) || (i.Attributes & 0x400)!=0 || ((i.Attributes & 0x10)!=0)!=directory || (!directory && i.Links!=1)) {
            h.Dispose();throw new IOException("Package path is not an ordinary pinned file/directory");
        }
        return new FileStream(h,FileAccess.Read);
    }
    public static FileStream WriteGuard(string path) {
        // Exclude execution while inspecting. Windows requires closing child handles before directory rename.
        var h=CreateFileW(path,0xC0000000,5,IntPtr.Zero,3,0x00200000,IntPtr.Zero);
        if(h.IsInvalid){h.Dispose();throw new IOException("Program is running or unavailable",Marshal.GetLastWin32Error());}
        Info i;
        if(!GetFileInformationByHandle(h,out i) || (i.Attributes & 0x410)!=0 || i.Links!=1){h.Dispose();throw new IOException("Invalid program guard path");}
        return new FileStream(h,FileAccess.ReadWrite);
    }
    public static FileStream ExclusiveLock(string path) {
        var h=CreateFileW(path,0xC0000000,0,IntPtr.Zero,3,0x00200000,IntPtr.Zero);
        if(h.IsInvalid){h.Dispose();throw new IOException("Maintenance lock is unavailable",Marshal.GetLastWin32Error());}
        Info i;
        if(!GetFileInformationByHandle(h,out i) || (i.Attributes & 0x410)!=0 || i.Links!=1 || i.SizeHigh!=0 || i.SizeLow!=0){h.Dispose();throw new IOException("Invalid maintenance lock");}
        return new FileStream(h,FileAccess.ReadWrite);
    }
}
'@
}
$pins=[Collections.Generic.List[IDisposable]]::new()
$child=$null
$childStarted=$false
function HashStream($stream) {
    $stream.Position=0
    $sha=[Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant() }
    finally {$sha.Dispose();$stream.Position=0}
}
function ReadJson($stream) {
    if($stream.Length -gt 1048576){throw 'Package metadata too large'}
    $reader=[IO.StreamReader]::new($stream,[Text.UTF8Encoding]::new($false,$true),$true,4096,$true)
    try{return ($reader.ReadToEnd() | ConvertFrom-Json)}finally{$reader.Dispose();$stream.Position=0}
}
function CapabilityKey($info) {
    if($null -eq $info){throw 'Candidate has no build capabilities'}
    # Typed JSON prevents missing, string-valued or separator-containing fields
    # from comparing equal to actual numeric/array build capabilities.
    return ([ordered]@{
        capabilitySchema=$info.capabilitySchema;product=$info.product;version=$info.version
        os=$info.os;architecture=$info.architecture;environment=$info.environment;protocolVersion=$info.protocolVersion
        upgradeHoldSchema=$info.upgradeHoldSchema;backupSchema=$info.backupSchema;binaryUpdateSchema=$info.binaryUpdateSchema
        dataSchema=$info.dataSchema;backupWriterVersions=@($info.backupWriterVersions)
        dataFormatGuardSchema=$info.dataFormatGuardSchema;readableDataSchemas=@($info.readableDataSchemas)
        maintenanceInterlockSchema=$info.maintenanceInterlockSchema;dataRecoverySchema=$info.dataRecoverySchema
        dataReaderGeneration=$info.dataReaderGeneration
        independentVerifierSchema=$info.independentVerifierSchema;independentVerifierSha256=$info.independentVerifierSha256
    } | ConvertTo-Json -Depth 8 -Compress)
}
function PackageResources($manifest) {
    $entries=@()
    if($manifest.PSObject.Properties.Name -contains 'resources'){$entries=@($manifest.resources)}
    $limits=@{'SQLITE-NOTICES.txt'=1048576;'THIRD-PARTY-NOTICES.txt'=8388608;'RUST-LIBRARY-COPYRIGHT.html'=4194304;'THIRD-PARTY-SOURCES.zip'=16777216;'THIRD-PARTY-MANIFEST.json'=1048576}
    $expected=@()
    if([version]$manifest.version -ge [version]'0.1.22'){$expected=@($limits.Keys)}
    elseif([version]$manifest.version -ge [version]'0.1.21' -or $entries.Count -gt 0){$expected=@('SQLITE-NOTICES.txt')}
    if([version]$manifest.version -ge [version]'0.1.39'){
        $expected+=@('cerulean-verifier.exe');$limits['cerulean-verifier.exe']=33554432
        if($manifest.coreBuildInfo.independentVerifierSchema -ne 1 -or $manifest.coreBuildInfo.independentVerifierSha256 -cnotmatch '^[a-f0-9]{64}$'){throw 'Independent verifier build binding missing'}
        $tool=@($entries | Where-Object path -ceq 'cerulean-verifier.exe')
        if($tool.Count -ne 1 -or $tool[0].sha256 -cne $manifest.coreBuildInfo.independentVerifierSha256){throw 'Verifier and core binding differ'}
    }
    if($entries.Count -ne $expected.Count){throw 'Required notice resource missing or unexpected'}
    foreach($name in $expected){
        $matches=@($entries | Where-Object {$_.path -ceq $name})
        if($matches.Count -ne 1){throw 'Required notice resource missing, duplicate or unexpected'}
        $entry=$matches[0]
        if(($entry.bytes -isnot [int] -and $entry.bytes -isnot [long]) -or $entry.bytes -le 0 -or $entry.bytes -gt $limits[$name] -or $entry.sha256 -cnotmatch '^[a-f0-9]{64}$'){throw 'Notice size or digest declaration invalid'}
    }
    return $entries
}
try {
    $directory=[IO.Path]::GetFullPath($PackageDirectory)
    # Pin every directory component against rename while using names for signatures/process launch.
    $chain=[Collections.Generic.List[string]]::new()
    $cursor=[IO.DirectoryInfo]::new($directory)
    while($null -ne $cursor){$chain.Insert(0,$cursor.FullName);$cursor=$cursor.Parent}
    foreach($entry in $chain){$pins.Add([CeruleanPackagePin]::Open($entry,$true))}
    $manifestFile=[CeruleanPackagePin]::Open((Join-Path $directory 'manifest.json'),$false);$pins.Add($manifestFile)
    $digest=HashStream $manifestFile
    if($digest -cne $ManifestSha256.ToLowerInvariant()){throw 'Reviewed manifest digest changed'}
    $manifest=ReadJson $manifestFile
    if($manifest.schema -ne 1 -or $manifest.product -cne 'CeruleanLonging' -or $manifest.identifier -cne 'local.ceruleanlonging.workbench' -or $manifest.target -cne 'x86_64-pc-windows-msvc' -or $manifest.version -notmatch '^\d+\.\d+\.\d+$'){throw 'Unsupported package identity or schema'}
    $cap=$manifest.coreBuildInfo
    if($null -eq $cap -or $cap.capabilitySchema -ne 1 -or $cap.version -cne $manifest.version -or $cap.product -cne $manifest.product -or $cap.os -cne 'windows' -or $cap.architecture -cne 'x86_64' -or $cap.environment -cne 'msvc' -or $cap.protocolVersion -ne 1 -or $cap.upgradeHoldSchema -ne 1 -or $cap.backupSchema -ne 1 -or @($cap.backupWriterVersions).Count -eq 0 -or $cap.backupWriterVersions -cnotcontains $cap.version){throw 'Required candidate capabilities missing'}
    if($AllowUnsignedInternal){
        if($manifest.channel -cne 'internal-validation' -or $manifest.releaseAccepted -ne $false){throw 'Internal exception applies only to unreleased internal packages'}
    }else{
        if($manifest.releaseAccepted -ne $true -or $PublisherThumbprint -notmatch '^[a-fA-F0-9]{40}$'){throw 'Public release requires acceptance and an expected publisher certificate'}
    }
    $names=@('setup.exe','CeruleanLonging.exe','cerulean-runtime.exe')
    if(@($manifest.artifacts).Count -ne 3){throw 'Expected exactly three package artifacts'}
    $verified=@()
    foreach($name in $names){
        $entries=@($manifest.artifacts | Where-Object {$_.path -ceq $name})
        if($entries.Count -ne 1){throw 'Missing or duplicate package artifact'}
        $entry=$entries[0];$path=Join-Path $directory $name
        $file=[CeruleanPackagePin]::Open($path,$false);$pins.Add($file)
        if($entry.sha256 -notmatch '^[a-f0-9]{64}$' -or $file.Length -ne $entry.bytes -or (HashStream $file) -cne $entry.sha256){throw "Artifact integrity mismatch: $name"}
        $sig=Get-AuthenticodeSignature -LiteralPath $path
        $thumb=if($sig.SignerCertificate){$sig.SignerCertificate.Thumbprint}else{$null}
        if($AllowUnsignedInternal){
            if($sig.Status.ToString() -notin @('NotSigned','Valid')){throw "Invalid internal artifact signature: $name"}
        }elseif($sig.Status.ToString() -ne 'Valid' -or $thumb -ine $PublisherThumbprint){throw "Publisher verification failed: $name"}
        if($name -ceq 'cerulean-runtime.exe' -and $entry.sha256 -cne $manifest.coreSha256){throw 'Core digest differs from package runtime'}
        $verified+=@{path=$name;sha256=$entry.sha256;bytes=$file.Length;signatureStatus=$sig.Status.ToString();signerThumbprint=$thumb}
    }
    $resourceEntries=@(PackageResources $manifest)
    foreach($entry in $resourceEntries){
        $notice=[CeruleanPackagePin]::Open((Join-Path $directory $entry.path),$false);$pins.Add($notice)
        if($notice.Length -ne $entry.bytes -or (HashStream $notice) -cne $entry.sha256){throw 'Notice integrity mismatch'}
    }
    # Only the explicitly trusted, pinned candidate is executed; never run an installer here.
    $start=[Diagnostics.ProcessStartInfo]::new()
    $start.FileName=Join-Path $directory 'cerulean-runtime.exe'
    $start.Arguments='--build-info';$start.WorkingDirectory=$directory
    $start.UseShellExecute=$false;$start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
    $child=[Diagnostics.Process]::new();$child.StartInfo=$start
    if(-not $child.Start()){throw 'Candidate capability probe did not start'}
    $childStarted=$true
    # Fixed-size reads avoid retaining unbounded output from a broken candidate.
    $stdout=[char[]]::new(65537);$stderr=[char[]]::new(8193)
    $outTask=$child.StandardOutput.ReadBlockAsync($stdout,0,$stdout.Length)
    $errTask=$child.StandardError.ReadBlockAsync($stderr,0,$stderr.Length)
    if(-not $child.WaitForExit(10000)){throw 'Candidate capability probe timed out'}
    if(-not $outTask.Wait(1000) -or -not $errTask.Wait(1000)){throw 'Candidate output did not close'}
    if($child.ExitCode -ne 0 -or $outTask.Result -eq $stdout.Length -or $errTask.Result -ne 0){throw 'Candidate capability probe failed or output exceeded bounds'}
    $actual=([string]::new($stdout,0,$outTask.Result)) | ConvertFrom-Json
    if((CapabilityKey $actual) -cne (CapabilityKey $cap)){throw 'Actual runtime capabilities differ from manifest'}
    $review=[ordered]@{status='passed';packageDirectory=$directory;manifestSha256=$digest;version=$manifest.version;capabilities=$actual;artifacts=$verified;resources=$resourceEntries;trustMode=if($AllowUnsignedInternal){'explicit-reviewed-internal'}else{'accepted-publisher-signed'};installed=$false;dataOpened=$false;releaseAccepted=($manifest.releaseAccepted -eq $true -and -not $AllowUnsignedInternal)}
    if($RetainPins){
        # In-process transaction callers own and must dispose these exact handles.
        [pscustomobject]@{Review=$review;Pins=$pins.ToArray()}
        $pins.Clear()
    }else{$review | ConvertTo-Json -Depth 12 -Compress}
}finally{
    if($null -ne $child){if($childStarted -and -not $child.HasExited){$child.Kill();$child.WaitForExit(5000) | Out-Null};$child.Dispose()}
    for($i=$pins.Count-1;$i -ge 0;$i--){$pins[$i].Dispose()}
}
