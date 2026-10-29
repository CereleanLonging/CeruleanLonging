$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$report=Join-Path $root 'reports/signature-offline-audit-120-20261013.json'
if(Test-Path -LiteralPath $report){throw 'Refuse to overwrite existing audit'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CeruleanOfflineTrust120 {
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
 struct FileInfo { public UInt32 size; [MarshalAs(UnmanagedType.LPWStr)] public string path; public IntPtr handle; public IntPtr subject; }
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
 struct TrustData { public UInt32 size; public IntPtr policy; public IntPtr sip; public UInt32 ui; public UInt32 revocation; public UInt32 union; public IntPtr file; public UInt32 stateAction; public IntPtr state; public IntPtr url; public UInt32 flags; public UInt32 context; }
 [DllImport("wintrust.dll", ExactSpelling=true, CharSet=CharSet.Unicode)]
 static extern UInt32 WinVerifyTrust(IntPtr hwnd, [In] ref Guid action, [In,Out] ref TrustData data);
 public static string Check(string path) {
  var f=new FileInfo { size=(UInt32)Marshal.SizeOf(typeof(FileInfo)), path=path };
  var ptr=Marshal.AllocHGlobal(Marshal.SizeOf(typeof(FileInfo)));
  Marshal.StructureToPtr(f,ptr,false);
  var d=new TrustData { size=(UInt32)Marshal.SizeOf(typeof(TrustData)), ui=2, revocation=0, union=1, file=ptr, stateAction=1, flags=0x1010 };
  var action=new Guid("00AAC56B-CD44-11d0-8CC2-00C04FC295EE");
  try { return "0x"+WinVerifyTrust(new IntPtr(-1),ref action,ref d).ToString("X8"); }
  finally { d.stateAction=2; WinVerifyTrust(new IntPtr(-1),ref action,ref d); Marshal.DestroyStructure(ptr,typeof(FileInfo)); Marshal.FreeHGlobal(ptr); }
 }
}
'@
Add-Type -AssemblyName System.Security
function Hash([string]$path){(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
function PublicCertificate($cert){
 if(-not $cert){return $null}
 @{subject=$cert.Subject;issuer=$cert.Issuer;thumbprint=$cert.Thumbprint;serialNumber=$cert.SerialNumber;notBefore=$cert.NotBefore.ToUniversalTime().ToString('o');notAfter=$cert.NotAfter.ToUniversalTime().ToString('o');hasPrivateKey=$cert.HasPrivateKey;selfIssued=($cert.Subject -ceq $cert.Issuer);eku=@($cert.Extensions|Where-Object {$_.Oid.Value -eq '2.5.29.37'}|ForEach-Object {$_.Format($false)})}
}
function EmbeddedSignature([string]$path){
 $bytes=[IO.File]::ReadAllBytes($path)
 if($bytes.Length -lt 128 -or $bytes[0] -ne 77 -or $bytes[1] -ne 90){return @{isPe=$false}}
 $pe=[BitConverter]::ToInt32($bytes,0x3c)
 if($pe -lt 0 -or ($pe+152) -gt $bytes.Length){throw 'Invalid PE header'}
 $optional=$pe+24;$magic=[BitConverter]::ToUInt16($bytes,$optional)
 $directory=if($magic -eq 0x20b){$optional+112}elseif($magic -eq 0x10b){$optional+96}else{throw 'Unsupported PE magic'}
 $offset=[BitConverter]::ToUInt32($bytes,$directory+32);$length=[BitConverter]::ToUInt32($bytes,$directory+36)
 $result=@{isPe=$true;certificateTableOffset=$offset;certificateTableBytes=$length;embeddedSignaturePresent=($offset -gt 0 -and $length -gt 0);signers=@();timestamps=@()}
 if($offset -eq 0 -or $length -eq 0){return $result}
 if(([long]$offset+$length) -gt $bytes.Length -or $length -lt 8){throw 'Invalid PE certificate table'}
 $cursor=[long]$offset
 while($cursor -lt ([long]$offset+$length)){
  $size=[BitConverter]::ToUInt32($bytes,[int]$cursor);$kind=[BitConverter]::ToUInt16($bytes,[int]$cursor+6)
  if($size -lt 8 -or ($cursor+$size) -gt ($offset+$length)){throw 'Invalid WIN_CERTIFICATE'}
  if($kind -eq 2){
   $cms=[Security.Cryptography.Pkcs.SignedCms]::new();$payload=[byte[]]::new($size-8);[Array]::Copy($bytes,$cursor+8,$payload,0,$payload.Length);$cms.Decode($payload)
   $crypto=$true;$cryptoError=$null;try{$cms.CheckSignature($true)}catch{$crypto=$false;$cryptoError=$_.Exception.Message}
   foreach($signer in $cms.SignerInfos){
    $result.signers+=@{certificate=(PublicCertificate $signer.Certificate);cmsCryptographicSignatureValid=$crypto;cryptographicError=$cryptoError;digestAlgorithm=$signer.DigestAlgorithm.Value;unsignedAttributeOids=@($signer.UnsignedAttributes|ForEach-Object {$_.Oid.Value})}
    foreach($counter in $signer.CounterSignerInfos){$result.timestamps+=@{kind='PKCS7-counter-signature';certificate=(PublicCertificate $counter.Certificate)}}
    foreach($attribute in $signer.UnsignedAttributes){if($attribute.Oid.Value -eq '1.3.6.1.4.1.311.3.3.1'){foreach($value in $attribute.Values){$stamp=[Security.Cryptography.Pkcs.SignedCms]::new();$stamp.Decode($value.RawData);foreach($timestampSigner in $stamp.SignerInfos){$result.timestamps+=@{kind='RFC3161-token';certificate=(PublicCertificate $timestampSigner.Certificate)}}}}}
   }
  }
  $cursor+=([Math]::Floor(($size+7)/8)*8)
 }
 return $result
}
$trustCache=@{}
function InspectArtifact([string]$path){
 $file=Get-Item -LiteralPath $path;$before=Hash $path
 if(-not $trustCache.ContainsKey($before)){$trustCache[$before]=@{winVerifyTrustCacheOnly=[CeruleanOfflineTrust120]::Check($path);pe=(EmbeddedSignature $path)}}
 $version=[Diagnostics.FileVersionInfo]::GetVersionInfo($path)
 $after=Hash $path
 @{path=$path;bytes=$file.Length;sha256=$before;unchangedDuringRead=($before -ceq $after);fileVersion=$version.FileVersion;productVersion=$version.ProductVersion;lastWriteUtc=$file.LastWriteTimeUtc.ToString('o');signature=$trustCache[$before]}
}
$certificates=@()
foreach($store in @('Cert:\CurrentUser\My','Cert:\LocalMachine\My')){
 foreach($cert in @(Get-ChildItem -LiteralPath $store -CodeSigningCert)){$certificates+=@{store=$store;certificate=(PublicCertificate $cert)}}
}
$trustLocations=@()
foreach($store in @('Cert:\CurrentUser\Root','Cert:\CurrentUser\TrustedPublisher','Cert:\CurrentUser\TrustedPeople','Cert:\LocalMachine\Root','Cert:\LocalMachine\TrustedPublisher','Cert:\LocalMachine\TrustedPeople')){
 foreach($cert in @(Get-ChildItem -LiteralPath $store)){
  if($cert.Thumbprint -in @($certificates|ForEach-Object {$_.certificate.thumbprint})){$trustLocations+=@{store=$store;certificate=(PublicCertificate $cert)}}
 }
}
$packages=@()
foreach($directory in @(Get-ChildItem -LiteralPath (Join-Path $root 'reports') -Directory -Filter 'p6-windows-package*')){
 $manifestPath=Join-Path $directory.FullName 'manifest.json';if(-not(Test-Path -LiteralPath $manifestPath)){continue}
 $manifestHash=Hash $manifestPath;$manifest=Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8|ConvertFrom-Json
 $artifacts=@();foreach($expected in $manifest.artifacts){
  $observation=InspectArtifact (Join-Path $directory.FullName $expected.path)
  $artifacts+=@{expected=$expected;observed=$observation;bytesMatch=($expected.bytes -eq $observation.bytes);sha256Match=($expected.sha256 -ceq $observation.sha256)}
 }
 $inputs=@();foreach($expected in $manifest.buildInputs){$p=Join-Path $root $expected.path;$actual=if(Test-Path -LiteralPath $p){Hash $p}else{$null};$inputs+=@{path=$expected.path;packageBuildHash=$expected.sha256;currentHash=$actual;matches=($actual -ceq $expected.sha256)}}
 $packages+=@{path=$directory.FullName;version=$manifest.version;manifestSha256=$manifestHash;manifestUnchangedDuringRead=($manifestHash -ceq (Hash $manifestPath));declaredReleaseAccepted=$manifest.releaseAccepted;declaredChannel=$manifest.channel;artifacts=$artifacts;currentBuildInputComparisons=$inputs}
}
$releases=@()
foreach($folder in @('target/release','target/release/bundle/nsis','apps/desktop/src-tauri/binaries')){
 foreach($file in @(Get-ChildItem -LiteralPath (Join-Path $root $folder) -File -Filter '*.exe' -ErrorAction SilentlyContinue)){$releases+=InspectArtifact $file.FullName}
}
$archive=Join-Path $root 'reports/p6-output-recovery-sources-20261012';$sourceDifferences=@();$sourceCompared=0
foreach($folder in @('apps','crates','scripts')){
 foreach($frozen in @(Get-ChildItem -LiteralPath (Join-Path $archive $folder) -Recurse -File)){
  $relative=$frozen.FullName.Substring($archive.Length+1);$current=Join-Path $root $relative
  $old=Hash $frozen.FullName;$actual=if(Test-Path -LiteralPath $current){Hash $current}else{$null};$sourceCompared++
  if($actual -cne $old){$sourceDifferences+=@{path=$relative;frozen018Hash=$old;currentHash=$actual}}
 }
}
$scriptSignedBlocks=@();foreach($file in @(Get-ChildItem -LiteralPath (Join-Path $root 'scripts') -File -Filter '*.ps1')){if(Select-String -LiteralPath $file.FullName -SimpleMatch '# SIG # Begin signature block' -Quiet){$scriptSignedBlocks+=@{path=$file.FullName;sha256=(Hash $file.FullName)}}}
$result=@{at=[DateTimeOffset]::Now.ToString('o');scope='Read-only current artifact, certificate metadata and source comparison; no signing, trust changes, key export or executable launch';networkPolicy='WinVerifyTrust WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE (0x1010); no online OCSP/CRL/AIA or timestamp requests; SignedCms.CheckSignature(true) is mathematical verification only';revocationAccepted=$false;getAuthenticodeSignatureUsed=$false;winVerifyTrustResultsMeaning=@{'0x00000000'='Windows offline policy accepted using existing local cache/trust, revocation unverified';'0x800B0100'='TRUST_E_NOSIGNATURE';'0x800B0109'='CERT_E_UNTRUSTEDROOT';'0x80096010'='TRUST_E_BAD_DIGEST'};certificates=$certificates;certificateTrustLocations=$trustLocations;packages=$packages;releaseArtifacts=$releases;frozen018SourceCompared=$sourceCompared;frozen018SourceDifferences=$sourceDifferences;embeddedSignedScriptBlocks=$scriptSignedBlocks;limits=@('No private key accessed/exported; HasPrivateKey is only a public API availability flag','Certificate Subject=Issuer means self-issued; commercial/public trust is not inferred','No online revocation freshness or external timestamp service verification','Frozen selected-source archive is not a full reproducible-build provenance record','Concurrent changes may occur after this snapshot; each measured artifact was hashed before and after')}
[IO.File]::WriteAllText($report,($result|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
@{report=$report;packages=$packages.Count;releaseArtifacts=$releases.Count;certificates=$certificates.Count;trustLocations=$trustLocations.Count;frozen018SourceDifferences=$sourceDifferences.Count;signatureCodes=@($trustCache.Values|ForEach-Object {$_.winVerifyTrustCacheOnly}|Sort-Object -Unique)}|ConvertTo-Json -Compress
